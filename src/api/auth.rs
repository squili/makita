use std::collections::HashMap;
use std::lazy::Lazy;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use axum::body::{Body, Empty};
use axum::extract::{Extension, FromRequest, Path, Query, RequestParts};
use axum::http::{HeaderMap, Request, Response, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect};
use jsonwebtokens::{Algorithm, Verifier};
use serde::{Serialize, Deserialize};
use crate::api::utils::{ApiContext, ApiError, api_map};
use serde_json::json;
use serenity::http::{GuildPagination, Http};
use serenity::model::id::{GuildId, UserId};
use crate::macros::s;
use crate::modules::updates::GitMeta;
use anyhow::Error as AnyhowError;
use axum::Json;
use axum_debug::debug_handler;
use tower::{MakeService, Service};
use crate::modules::{AuthModule, AuthQueryError, PermissionType, Session};
use log::info;
use serenity::async_trait;
use serenity::model::guild::Guild;

const REQWEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(match &crate::modules::updates::GIT_META {
            Some(meta) => format!("DiscordBot (https://github.com/{}, {})", meta.repo, meta.tag),
            None => format!("DiscordBot (https://github.com/squili/makita, {} (Local))", env!("CARGO_PKG_VERSION"))
        })
        .build()
        .expect("build oauth client")
});

#[debug_handler]
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

#[debug_handler]
pub async fn callback(
    Extension(ctx): Extension<Arc<ApiContext>>,
    Query(params): Query<HashMap<String, String>>
) -> Result<impl IntoResponse, ApiError> {
    let code = params.get("code").ok_or_else(|| ApiError::BadRequest("missing code parameter"))?;

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

    Ok(Redirect::temporary(Uri::from_str(&format!("{}/set_token?token={}", ctx.config.frontend_url, token)).unwrap()))
}

pub async fn check(ctx: &Arc<ApiContext>, headers: &HeaderMap, guild_id: Option<&GuildId>, permission: Option<&PermissionType>) -> Result<Arc<Session>, ApiError> {
    let token = headers
        .get("authorization")
        .ok_or_else(|| ApiError::BadRequest("Missing `authorization` header"))?
        .to_str()
        .map_err(|_| ApiError::BadRequest("Invalid character in `authorization` header"))?;

    Ok(match permission {
        None => ctx.auth.query(token, guild_id).await.map_err(|err| err.as_api_error())?,
        Some(permission) => ctx.auth.query_permission(&ctx, token, guild_id.unwrap(), permission).await.map_err(|err| err.as_api_error())?,
    })
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
}

#[debug_handler]
pub async fn session(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, None).await?;
    let user = session.user.id.to_user(&*ctx).await.map_err(|_| ApiError::CacheMissing)?;
    let mut guilds = Vec::new();
    for guild_id in session.user.guilds.read().await.keys() {
        match ctx.cache.guild_field(guild_id, |guild| {(guild.name.clone(), guild.icon_url().clone())}) {
            None => continue,
            Some(info) => guilds.push(SessionInfoResponseInner {
                id: guild_id.clone(),
                name: info.0,
                icon: info.1,
            })
        }
    }

    Ok(Json(SessionInfoResponse {
        id: session.user.id,
        icon: user.avatar_url().unwrap_or_else(|| user.default_avatar_url()),
        name: user.name,
        guilds
    }))
}
