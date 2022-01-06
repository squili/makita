// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::borrow::Borrow;
use crate::prelude::*;
use std::collections::HashMap;
use std::lazy::SyncLazy;
use std::str::FromStr;
use axum::extract::{Extension, Path, Query};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Redirect};
use serde::{Serialize, Deserialize};
use crate::api::utils::{ApiError, api_map, serialize_response};
use serenity::http::{GuildPagination, Http};
use serenity::model::id::{GuildId, UserId};
use anyhow::Error as AnyhowError;
use axum::Json;
use serde_json::{Map, Value};
use crate::modules::{Session, WebPermissionLevel};

static REQWEST_CLIENT: SyncLazy<reqwest::Client> = SyncLazy::new(|| {
    reqwest::Client::builder()
        .user_agent(match &crate::modules::updates::GIT_META {
            Some(meta) => format!("DiscordBot (https://github.com/{}, {})", meta.repo, meta.tag),
            None => format!("DiscordBot (https://github.com/squili/makita, {} (Local))", env!("CARGO_PKG_VERSION"))
        })
        .build()
        .expect("build oauth client")
});

pub async fn redirect(Extension(ctx): Extension<Arc<ApiContext>>) -> impl IntoResponse {
    let redirect_url = format!(
        "https://discord.com/api/oauth2/authorize?client_id={}&prompt=consent&redirect_uri={}/auth/callback&response_type=code&scope=identify%20guilds",
        ctx.config.client_id, ctx.config.api_url);
    Redirect::temporary(Uri::from_str(&redirect_url).unwrap())
}

#[derive(Serialize)]
struct TokenRequest {
    client_id: String,
    client_secret: String,
    grant_type: &'static str,
    code: String,
    redirect_uri: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    error: Option<String>,
    error_description: Option<String>,
    access_token: Option<String>,
}

pub async fn callback(
    Extension(ctx): Extension<Arc<ApiContext>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let code = params.get("code").ok_or(ApiError::BadRequest("missing code parameter"))?;

    let resp: TokenResponse = REQWEST_CLIENT
        .post("https://discord.com/api/oauth2/token")
        .form(&TokenRequest {
            client_id: ctx.config.client_id.to_string(),
            client_secret: ctx.config.client_secret.clone(),
            grant_type: "authorization_code",
            code: code.clone(),
            redirect_uri: format!("{}/auth/callback", ctx.config.api_url)
        })
        .send()
        .await
        .map_err(api_map!())?
        .json()
        .await
        .map_err(api_map!())?;

    let token = match resp.access_token {
        None => {
            // there was an error
            return Err(ApiError::Internal(AnyhowError::msg(match (resp.error, resp.error_description) {
                (Some(error), Some(description)) => format!("Discord Error: {} - {}", error, description),
                _ => s!("Discord Error: No further info")
            })));
        }
        Some(token) => token,
    };

    let http = Http::new_with_token(&format!("Bearer {}", token));

    let user = http.get_current_user().await.map_err(api_map!())?;
    let mut guilds = Vec::new();
    let mut paginated = GuildId(0);
    loop {
        let chunk = http.get_guilds(Some(&GuildPagination::After(paginated)), Some(100)).await.map_err(api_map!())?;
        let stop = chunk.len() < 100;
        guilds.extend(chunk);
        if stop {
            break;
        }
        paginated = guilds.last().unwrap().id;
    }

    let token = ctx.auth.new_session(user, guilds).await.map_err(api_map!())?;

    Ok(Redirect::temporary(Uri::from_str(&format!("{}/authenticate?token={}", ctx.config.frontend_url, token)).unwrap()))
}

pub async fn check(ctx: &Arc<ApiContext>, headers: &HeaderMap, guild_id: Option<&GuildId>, permission: &WebPermissionLevel) -> Result<Arc<Session>, ApiError> {
    let token = headers
        .get("authorization")
        .ok_or(ApiError::BadRequest("Missing `authorization` header"))?
        .to_str()
        .map_err(|_| ApiError::BadRequest("Invalid character in `authorization` header"))?;

    let session = ctx.auth.query(token, guild_id).await.map_err(|err| err.as_api_error())?;
    if let WebPermissionLevel::None = permission {} else {
        ctx.auth.query_permission(ctx, &session, guild_id.unwrap(), permission).await.map_err(|err| err.as_api_error())?;
    }
    Ok(session)
}

#[derive(Serialize)]
struct SessionInfoResponse {
    id: UserId,
    name: String,
    icon: String,
    guilds: Vec<SessionInfoResponseInner>
}

#[derive(Serialize)]
struct SessionInfoResponseInner {
    id: GuildId,
    name: String,
    icon: Option<String>,
    viewer: bool,
    editor: bool,
}

pub async fn session(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;
    let user = session.user.id.to_user(&*ctx).await.map_err(|_| ApiError::CacheMissing)?;
    let mut guilds = Vec::new();
    let handle = session.user.guilds.read().await;
    let iter = handle.keys().cloned().collect::<Vec<GuildId>>();
    drop(handle);
    for guild_id in iter {
        if let Some((name, icon)) = ctx.cache.guild_field(guild_id, |guild| {(guild.name.clone(), guild.icon_url())}) {
            let level = ctx.auth.query_permission(ctx.clone().borrow(), &session, &guild_id, &WebPermissionLevel::None)
                .await.map_err(|e| e.as_api_error())?;
            if level == WebPermissionLevel::None {
                continue
            }
            guilds.push(SessionInfoResponseInner {
                id: guild_id, name, icon,
                viewer: level >= WebPermissionLevel::Viewer,
                editor: level == WebPermissionLevel::Editor,
            })
        }
    }

    Ok(serialize_response(SessionInfoResponse {
        id: session.user.id,
        icon: user.avatar_url().unwrap_or_else(|| user.default_avatar_url()),
        name: user.name,
        guilds
    }))
}

pub async fn clear_sessions(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    check(&ctx, &headers, None, &WebPermissionLevel::None).await?;
    Ok("TODO")
}

#[allow(dead_code)]
pub async fn check_viewer(Extension(ctx): Extension<Arc<ApiContext>>, Path(guild_id): Path<u64>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    check(&ctx, &headers, Some(&GuildId(guild_id)), &WebPermissionLevel::Viewer).await?;
    Ok(Json(Value::Object(Map::new())))
}

#[allow(dead_code)]
pub async fn check_editor(Extension(ctx): Extension<Arc<ApiContext>>, Path(guild_id): Path<u64>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    check(&ctx, &headers, Some(&GuildId(guild_id)), &WebPermissionLevel::Editor).await?;
    Ok(Json(Value::Object(Map::new())))
}
