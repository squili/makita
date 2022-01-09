// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::sync::Arc;
use axum::body::Bytes;
use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum::response::IntoResponse;
use hmac::{Hmac, Mac};
use log::info;
use crate::api::utils::{api_map, ApiError, serialize_response};
use crate::ApiContext;
use serde::{Serialize, Deserialize};
use serenity::model::id::UserId;
use sha2::Sha256;
use crate::api::auth::check;
use crate::modules::{AdminPermissionData, WebPermissionLevel};
use crate::updates::{check_update, do_update_from_action, GIT_META};
use crate::utils::SqlId;

#[derive(Deserialize, Serialize)]
pub struct AdminListEntry {
    id: UserId,
    manage_admins: bool,
    manage_instance: bool,
    bypass_permissions: bool,
}

pub async fn get_admins(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_manage_admins(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    Ok(serialize_response(ctx.permissions.admin_cache.read().await.iter().map(|(id, data)| {
        AdminListEntry {
            id: *id,
            manage_admins: data.manage_admins,
            manage_instance: data.manage_instance,
            bypass_permissions: data.bypass_permissions,
        }
    }).collect::<Vec<AdminListEntry>>()))
}

#[derive(Deserialize)]
pub struct PatchAdminsData {
    update_admins: Option<Vec<AdminListEntry>>,
    remove_admins: Option<Vec<UserId>>,
}

pub async fn patch_admins(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap, Json(data): Json<PatchAdminsData>) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_manage_admins(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    if let Some(inner) = data.update_admins {
        for entry in inner {
            ctx
                .permissions
                .admin_cache
                .write()
                .await
                .insert(entry.id, AdminPermissionData {
                    manage_admins: entry.manage_admins,
                    manage_instance: entry.manage_instance,
                    bypass_permissions: entry.bypass_permissions
                });
            sqlx::query("insert into Admins (id, manage_admins, manage_instance, bypass_permissions)\
                    values ($1, $2, $3, $4) on conflict on constraint admins_pkey do update set manage_admins = $2, manage_instance = $3, bypass_permissions = $4")
                .bind(SqlId(entry.id))
                .bind(entry.manage_admins)
                .bind(entry.manage_instance)
                .bind(entry.bypass_permissions)
                .execute(&ctx.pool)
                .await
                .map_err(api_map!())?;
        }
    }

    if let Some(inner) = data.remove_admins {
        for entry in inner {
            ctx.permissions.admin_cache.write().await.remove(&entry);
            sqlx::query("delete from Admins where id = $1")
                .bind(SqlId(entry))
                .execute(&ctx.pool)
                .await
                .map_err(api_map!())?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_update(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_manage_admins(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    Ok(match check_update().await? {
        Some(update_info) => {
            serialize_response(&update_info)
        }
        None => {
            serialize_response(())
        }
    })
}

pub async fn post_update(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_manage_admins(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    let action = check_update().await?.ok_or(ApiError::BadRequest("No updates available"))?;
    info!("update started");
    do_update_from_action(action).await?;
    ctx.updates.restart().await?;
    info!("restarting");

    Ok(StatusCode::NO_CONTENT)
}

pub async fn restart(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_manage_admins(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    ctx.updates.restart().await?;
    info!("restarting");

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_sudo(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_bypass_permissions(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    Ok(serialize_response(ctx.permissions.sudo_users.read().await.clone()))
}

#[derive(Deserialize)]
pub struct PostSudoData {
    state: bool,
}

pub async fn post_sudo(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap, Json(data): Json<PostSudoData>) -> Result<impl IntoResponse, ApiError> {
    let session = check(&ctx, &headers, None, &WebPermissionLevel::None).await?;

    if !ctx.permissions.get_admin_bypass_permissions(&session.user.id).await {
        return Err(ApiError::MissingPermission);
    }

    let mut handle = ctx.permissions.sudo_users.write().await;
    match data.state {
        true => handle.insert(session.user.id),
        false => handle.remove(&session.user.id),
    };
    drop(handle);

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct GithubHasALargePayloadForWebhooksButWeReallyDontNeedTheEntireThingJustThisOneFieldWhichIsWhyWeDontUseTheOctocrabModelStruct {
    action: String,
}

pub async fn github_webhook(Extension(ctx): Extension<Arc<ApiContext>>, headers: HeaderMap, body: Bytes) -> Result<impl IntoResponse, ApiError> {
    if GIT_META.is_none() {
        return Err(ApiError::BadRequest("Updates disabled"));
    }

    // only want to process events of type "workflow_job"
    if headers
        .get("X-GitHub-Event")
        .ok_or(ApiError::BadRequest("Missing event type"))?
        .to_str()
        .map_err(|_| ApiError::BadRequest("Invalid event type"))? != "workflow_job" {
        return Ok(StatusCode::NO_CONTENT);
    }

    let secret = ctx.config.github_webhook_secret.as_ref().ok_or(ApiError::BadRequest("Github webhooks disabled"))?.as_bytes();

    let mut signature = [0_u8; 32];
    hex::decode_to_slice(
        headers
            .get("X-Hub-Signature-256")
            .ok_or(ApiError::BadRequest("Missing signature"))?
            .as_bytes()
            .split_at(7).1,
        &mut signature
    ).map_err(api_map!())?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(&*body);
    if mac.verify_slice(&signature).is_err() {
        return Err(ApiError::BadRequest("Invalid signature"));
    }

    let data: GithubHasALargePayloadForWebhooksButWeReallyDontNeedTheEntireThingJustThisOneFieldWhichIsWhyWeDontUseTheOctocrabModelStruct
        = serde_json::from_str(&*String::from_utf8_lossy(&*body)).map_err(api_map!())?;

    if data.action == "completed" {
        let action = check_update().await?.ok_or(ApiError::BadRequest("No updates available"))?;
        info!("update started");
        do_update_from_action(action).await?;
        ctx.updates.restart().await?;
        info!("restarting");
    }

    Ok(StatusCode::NO_CONTENT)
}
