// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::invite_url;
use crate::prelude::*;
use crate::utils::{defer_command, BotContext};
use crate::Config;
use anyhow::Result;
use axum::body::Bytes;
use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use hmac::Hmac;
use hmac::Mac;
use semver::Version;
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use sha2::Sha256;
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::fs;
use tokio::sync::mpsc;

#[derive(Serialize)]
pub struct GitMeta {
    pub tag: &'static str,
    pub commit: &'static str,
    pub repo: &'static str,
}

pub static GIT_META: Option<GitMeta> = init_git_data();

const fn init_git_data() -> Option<GitMeta> {
    match (
        option_env!("GIT_TAG"),
        option_env!("GIT_COMMIT"),
        option_env!("GIT_REPO"),
    ) {
        (Some(tag), Some(commit), Some(repo)) => Some(GitMeta { tag, commit, repo }),
        _ => None,
        // _ => Some(GitMeta { tag: "v0.0.0", commit: "1234567", repo: "squili/makita" }), // kept here for local update testing
    }
}

pub fn current_version() -> &'static str {
    match &GIT_META {
        None => env!("CARGO_PKG_VERSION"),
        Some(meta) => meta.tag,
    }
}

pub static RESTARTING: AtomicBool = AtomicBool::new(false);

pub async fn start_update_server(
    config: Arc<Config>,
    updates: Arc<UpdatesModule>,
) -> Result<(), anyhow::Error> {
    if let Some(addr) = config.host_addr {
        let app = Router::new()
            .route("/", post(github_webhook))
            .layer(Extension(config.clone()))
            .layer(Extension(updates));

        tokio::spawn(async move {
            axum::Server::bind(&addr)
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
    }

    Ok(())
}

#[derive(Deserialize)]
struct GithubPayload {
    action: String,
}

async fn github_webhook(
    Extension(config): Extension<Arc<Config>>,
    Extension(updates): Extension<Arc<UpdatesModule>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let meta = GIT_META
        .as_ref()
        .ok_or((StatusCode::FORBIDDEN, "Updates disabled"))?;

    // only want to process events of type "workflow_job"
    if headers
        .get("X-GitHub-Event")
        .ok_or((StatusCode::BAD_REQUEST, "Missing event type"))?
        .to_str()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid event type"))?
        != "workflow_job"
    {
        return Ok((StatusCode::OK, "Ignored event type"));
    }

    let secret = config
        .github_webhook_secret
        .as_ref()
        .ok_or((StatusCode::FORBIDDEN, "Github webhooks disabled"))?
        .as_bytes();

    let mut signature = [0_u8; 32];
    hex::decode_to_slice(
        headers
            .get("X-Hub-Signature-256")
            .ok_or((StatusCode::BAD_REQUEST, "Missing signature"))?
            .as_bytes()
            .split_at(7)
            .1,
        &mut signature,
    )
    .map_err(|_| (StatusCode::BAD_REQUEST, "Corrupted signature"))?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(&body);
    if mac.verify_slice(&signature).is_err() {
        return Err((StatusCode::BAD_REQUEST, "Invalid signature"));
    }

    let data: GithubPayload = serde_json::from_str(&String::from_utf8_lossy(&body))
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid payload"))?;

    if data.action == "completed" {
        let (owner, repo) = meta.repo.split_once('/').unwrap();
        let latest = octocrab::instance()
            .repos(owner, repo)
            .releases()
            .get_latest()
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed getting repository",
                )
            })?;

        let local_version = Version::parse(&meta.tag.chars().skip(1).collect::<String>())
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid local version"))?;
        let remote_version =
            Version::parse(&latest.tag_name.chars().skip(1).collect::<String>())
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid remote version"))?;

        return if remote_version > local_version {
            let asset_url = latest
                .assets
                .into_iter()
                .find(|s| s.name == "makita")
                .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Couldn't find asset"))?
                .browser_download_url;

            info!("update started");
            let executable = env::current_exe().unwrap().to_str().unwrap().to_string();
            let bytes = reqwest::get(asset_url)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed requesting update asset",
                    )
                })?
                .bytes()
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed downloading update asset",
                    )
                })?;
            fs::write(executable.to_string() + ".part", bytes)
                .await
                .unwrap();
            fs::rename(&executable, executable.to_string() + ".old")
                .await
                .unwrap();
            fs::rename(executable.to_string() + ".part", &executable)
                .await
                .unwrap();

            let mut permissions = fs::metadata(&executable).await.unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).await.unwrap();

            info!("restarting");
            updates.restart().await.unwrap();
            Ok((StatusCode::OK, "Update started"))
        } else {
            Err((StatusCode::BAD_REQUEST, "No update available"))
        };
    }

    Ok((StatusCode::OK, "Ignored action value"))
}

pub struct UpdatesModule {
    shutdown_tx: mpsc::Sender<()>,
    application_id: u64,
}

impl UpdatesModule {
    pub fn new(shutdown_tx: mpsc::Sender<()>, application_id: u64) -> Self {
        Self {
            shutdown_tx,
            application_id,
        }
    }

    pub async fn info_command(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        interaction.create_followup_message(&ctx, |builder| {
            builder.embed(|embed| {
                embed
                    .field("Bot Info", "Created by <@719046554744520754>\nLicensed under [AGPL](https://www.gnu.org/licenses/#AGPL)", false)
                    .field("Links", format!("[Docs](https://squili.github.io/makita-docs/)\n[Donate](https://donate.squi.live)\n\
                    [Server](https://discord.gg/SWMKshyutT)\n[Invite]({})",
                                            invite_url!(self.application_id)), false)
                    .field("Build Info", match &GIT_META {
                        Some(meta) =>
                            format!("Tag: [{}](https://github.com/{}/releases/tag/{})\nCommit: [{}](https://github.com/{}/commit/{})\nRepo: [{}](https://github.com/{})",
                                    meta.tag, meta.repo, meta.tag, &meta.commit[0..7], meta.repo, meta.commit, meta.repo, meta.repo),
                        None => format!("Local Build\nPackage Version: v{}", env!("CARGO_PKG_VERSION"))
                    }, false)
            })
        }).await?;

        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        RESTARTING.store(true, Ordering::SeqCst);
        self.shutdown_tx.send(()).await?;
        Ok(())
    }
}
