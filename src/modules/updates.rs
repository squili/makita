// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::env;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::{Error, Result};
use semver::Version;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use tokio::sync::mpsc;
use tokio::fs;
use crate::error::BotError;
use crate::utils::{BotContext, FollowupBuilder};
use crate::macros::invite_url;

pub struct GitMeta {
    tag: &'static str,
    commit: &'static str,
    repo: &'static str,
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

pub static RESTARTING: AtomicBool = AtomicBool::new(false);

pub struct UpdateAction {
    download_url: String,
    pub old_version: String,
    pub new_version: String,
}

pub async fn check_update() -> Result<Option<UpdateAction>> {
    match &GIT_META {
        None => Err(Error::msg("Local builds cannot be updated")),
        Some(meta) => {
            let (owner, repo) = meta.repo.split_once("/").ok_or(BotError::Internal(12))?;
            let latest = octocrab::instance()
                .repos(owner, repo)
                .releases()
                .get_latest()
                .await?;

            let local_version = Version::parse(&meta.tag.chars().skip(1).collect::<String>())?;
            let remote_version = Version::parse(&latest.tag_name.chars().skip(1).collect::<String>())?;

            if remote_version > local_version {
                let asset_url = latest.assets.into_iter().find(|s| s.name == "makita")
                    .ok_or_else(|| BotError::Generic("Release assets missing".to_string()))?.browser_download_url;
                Ok(Some(UpdateAction {
                    download_url: asset_url.to_string(),
                    old_version: meta.tag.to_string(),
                    new_version: latest.tag_name,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

pub async fn do_update() -> Result<()> {
    let action = check_update().await?.ok_or_else(|| BotError::Generic("No updates available".to_string()))?;

    let executable = env::current_exe()?.to_str().unwrap().to_string();
    let bytes = reqwest::get(action.download_url).await?.bytes().await?;
    fs::write(executable.to_string() + ".part", bytes).await?;
    fs::rename(&executable, executable.to_string() + ".old").await?;
    fs::rename(executable.to_string() + ".part", &executable).await?;

    let mut permissions = fs::metadata(&executable).await?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).await?;

    Ok(())
}

pub struct UpdatesModule {
    shutdown_tx: mpsc::Sender<()>,
}

impl UpdatesModule {
    pub fn new(shutdown_tx: mpsc::Sender<()>) -> Self {
        Self { shutdown_tx }
    }

    pub async fn info_command(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.create_followup_message(&ctx, |builder| {
            builder.create_embed(|embed| {
                embed
                    .field("Bot Info", "Created by <@719046554744520754>\nLicensed under [AGPL](https://www.gnu.org/licenses/#AGPL)", false)
                    .field("Links", format!("[Docs](https://squili.github.io/makita-docs/)\n[Donate](https://donate.squi.live)\n\
                    [Server](https://discord.gg/SWMKshyutT)\n[Invite]({})",
                                            invite_url!(ctx.http.application_id)), false)
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

    pub async fn check_command(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
        let msg = match check_update().await? {
            Some(update) => format!("Update available from `{}` to `{}`", update.old_version, update.new_version),
            None => "No updates found".to_string()
        };

        FollowupBuilder::new()
            .description(msg)
            .build_command(&ctx.http, interaction)
            .await
    }
    pub async fn update_command(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
        do_update().await?;

        RESTARTING.store(true, Ordering::SeqCst);
        self.shutdown_tx.send(()).await?;

        FollowupBuilder::new()
            .description("Update successful, restarting...")
            .build_command(&ctx.http, interaction)
            .await
    }

    pub async fn restart_command(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
        RESTARTING.store(true, Ordering::SeqCst);
        self.shutdown_tx.send(()).await?;

        FollowupBuilder::new()
            .description("Restarting...")
            .build_command(&ctx.http, interaction)
            .await
    }
}