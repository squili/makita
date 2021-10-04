// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use anyhow::{Result, Error};
use crate::handler::Handler;
use serenity::client::Context;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use crate::decode;
use crate::error::BotError;
use serenity::model::interactions::message_component::MessageComponentInteraction;
use crate::custom_ids::{parse_custom_id, CustomIdType};
use crate::modules::PermissionType;

macro ensure_guild {
    ($interaction: expr, $command: expr) => {
        match $interaction.guild_id {
            Some(_) => $command,
            None => Err(Error::new(BotError::GuildOnly))
        }
    }
}

macro ensure_permission_base {
    ($ctx: expr, $cache: expr, $interaction: expr, $application_id: expr, $permission: path, $command: expr) => {
        ensure_guild!(
            $interaction,
            if let Some(missing) = $cache.check(
                &$ctx,
                &$permission,
                &$interaction.guild_id.unwrap(),
                &$interaction.user.id,
                &$interaction.member.as_ref().ok_or(BotError::CacheMissing)?.roles).await?
            {
                Err(Error::new(BotError::Permissions(*missing)))
            } else {
                $command
            }
        )
    }
}

pub async fn chat_input_router(handler: &Handler, ctx: &Context, interaction: &ApplicationCommandInteraction) -> Result<()> {
    macro ensure_permission {
        ($permission: path, $command: expr) => {
            ensure_permission_base!(ctx, handler.permissions, interaction, handler.application_id, $permission, $command)
        }
    }

    let (path, args) = decode::process(&interaction.data);

    match path.as_str() {
        "makita sudo" => handler.permissions.makita_sudo(ctx, interaction).await,
        "permissions list" => ensure_permission!(PermissionType::ManagePermissions, handler.permissions.permissions_list(ctx, interaction).await),
        "permissions set" => ensure_permission!(PermissionType::ManagePermissions, handler.permissions.permissions_set(ctx, interaction, args).await),
        "permissions add" => ensure_permission!(PermissionType::ManagePermissions, handler.permissions.permissions_add(ctx, interaction, args).await),
        "permissions remove" => ensure_permission!(PermissionType::ManagePermissions, handler.permissions.permissions_remove(ctx, interaction, args).await),
        "previews add" => ensure_permission!(PermissionType::ManagePreviews, handler.previews_module.previews_add(ctx, interaction, args).await),
        "previews remove" => ensure_permission!(PermissionType::ManagePreviews, handler.previews_module.previews_remove(ctx, interaction, args).await),
        "previews list" => ensure_permission!(PermissionType::ManagePreviews, handler.previews_module.previews_list(ctx, interaction).await),
        "previews view" => handler.previews_module.previews_view(ctx, interaction, args).await,
        _ => Ok(())
    }
}

pub async fn component_router(handler: &Handler, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
    macro ensure_permission {
        ($permission: path, $command: expr) => {
            ensure_permission_base!(ctx, handler.permissions, interaction, handler.application_id, $permission, $command)
        }
    }

    let (ty, _args) = parse_custom_id(&interaction.data.custom_id)?;

    use CustomIdType::*;
    match ty {
        ListPermissions => ensure_permission!(PermissionType::ManagePermissions, handler.permissions.permissions_list_component(ctx, interaction).await),
    }
}

pub async fn user_router(_handler: &Handler, _ctx: &Context, _command: &ApplicationCommandInteraction) -> Result<()> { Ok(()) }
pub async fn message_router(_handler: &Handler, _ctx: &Context, _command: &ApplicationCommandInteraction) -> Result<()> { Ok(()) }