// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::sync::Arc;
use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum::response::IntoResponse;
use crate::api::utils::{api_map, ApiError, serialize_response};
use crate::ApiContext;
use serde::{Serialize, Deserialize};
use serenity::model::id::UserId;
use crate::api::auth::check;
use crate::modules::{AdminPermissionData, WebPermissionLevel};
use crate::updates::{check_update, do_update_from_action};
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

pub async fn patch_admins(Extension(ctx): Extension<Arc<ApiContext>>, Json(data): Json<PatchAdminsData>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
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
    do_update_from_action(action).await?;
    ctx.updates.restart().await?;

    Ok(StatusCode::NO_CONTENT)
}
