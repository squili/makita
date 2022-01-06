// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::collections::HashSet;
use std::sync::Arc;
use axum::extract::{Extension, Path};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum::response::IntoResponse;
use serenity::model::id::{ChannelId, GuildId};
use crate::api::auth::check;
use crate::api::utils::{api_map, ApiError, serialize_response};
use crate::ApiContext;
use crate::modules::WebPermissionLevel;
use crate::utils::{OptionalOption, SqlId};
use serde::{Serialize, Deserialize};
use sqlx::Postgres;

#[derive(Serialize)]
struct GetPreviewData {
    auto_channels: Vec<ChannelId>,
    archive_channel: Option<ChannelId>,
}

pub async fn get_previews(Extension(ctx): Extension<Arc<ApiContext>>, Path(guild_id): Path<u64>, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    check(&ctx, &headers, Some(&GuildId(guild_id)), &WebPermissionLevel::Viewer).await?;

    let (auto_channels, archive_channel) = ctx.previews.read_cache(&GuildId(guild_id), |cache| {
        (cache.auto_channels.clone(), cache.archive_channel)
    }).await;

    let data = GetPreviewData { auto_channels, archive_channel };

    Ok(serialize_response(data))
}

#[derive(Deserialize)]
pub struct PatchPreviewData {
    add_auto_channels: Option<Vec<ChannelId>>,
    remove_auto_channels: Option<Vec<ChannelId>>,
    set_archive_channel: OptionalOption<ChannelId>,
}

pub async fn patch_previews(Extension(ctx): Extension<Arc<ApiContext>>, Path(guild_id): Path<u64>, Json(data): Json<PatchPreviewData>, headers: HeaderMap)
    -> Result<impl IntoResponse, ApiError>
{
    check(&ctx, &headers, Some(&GuildId(guild_id)), &WebPermissionLevel::Editor).await?;
    let guild_id = GuildId(guild_id);

    let real_channels = ctx.cache.guild_channels(guild_id).ok_or(ApiError::CacheMissing)?
        .into_iter().map(|(id, _)| id).collect::<HashSet<ChannelId>>();

    ctx.previews.write_cache(&guild_id, |cache| {
        if let Some(inner) = &data.add_auto_channels {
            cache.auto_channels.extend(inner.iter().filter(
                |c| !cache.auto_channels.contains(c) && real_channels.contains(c)
            ).cloned().collect::<Vec<ChannelId>>());
        }
        if let Some(inner) = &data.remove_auto_channels {
            // stuff like this should totally be in the standard library
            cache.auto_channels = cache.auto_channels.iter().filter(|c| !inner.contains(c)).cloned().collect::<Vec<ChannelId>>();
        }
        if let OptionalOption::Present(inner) = data.set_archive_channel {
            cache.archive_channel = inner;
        }
    }).await;

    // we could totally do these in parallel
    if let Some(inner) = data.add_auto_channels {
        for channel in inner {
            if real_channels.contains(&channel) {
                sqlx::query::<Postgres>("insert into PreviewChannels (guild_id, channel_id) values ($1, $2) on conflict do nothing")
                    .bind(SqlId(guild_id))
                    .bind(SqlId(channel))
                    .execute(&ctx.pool)
                    .await
                    .map_err(api_map!())?;
            }
        }
    }

    if let Some(inner) = data.remove_auto_channels {
        for channel in inner {
            sqlx::query::<Postgres>("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
                .bind(SqlId(guild_id))
                .bind(SqlId(channel))
                .execute(&ctx.pool)
                .await
                .map_err(api_map!())?;
        }
    }

    if let OptionalOption::Present(inner) = data.set_archive_channel {
        ctx.previews.update_archive(&ctx.pool, inner, guild_id).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
