// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use anyhow::{Result, Error};
use serenity::model::guild::Role;
use crate::handler::{Handler, handler_log};
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use crate::decode;
use crate::error::BotError;
use serenity::model::interactions::message_component::MessageComponentInteraction;
use crate::custom_ids::{parse_custom_id, CustomIdType};
use crate::modules::PermissionType;
use crate::utils::BotContext;
use crate::macros::debug;
use serenity::model::interactions::{Interaction, InteractionApplicationCommandCallbackDataFlags, InteractionResponseType};
use serenity::utils::Color;

macro ensure_guild {
    ($interaction: expr, $command: expr) => {
        match $interaction.guild_id {
            Some(_) => $command,
            None => Err(Error::new(BotError::GuildOnly))
        }
    }
}

macro ensure_permission_base {
    ($ctx: expr, $cache: expr, $interaction: expr, $application_id: expr, $permission: ident, $command: expr) => {
        ensure_guild!(
            $interaction,
            {
                let guild_id = $interaction.guild_id.unwrap();
                let roles = &$interaction.member.as_ref().ok_or(BotError::CacheMissing)?.roles;
                let mut upgraded_roles = Vec::new();
                let owner = $ctx.cache.guild_field(&guild_id, |g| {
                    for role in roles {
                        match g.roles.get(&role) {
                            Some(data) => upgraded_roles.push(data.clone()),
                            None => {}
                        }
                    }
                    g.owner_id
                }).ok_or(BotError::CacheMissing)?;
                if let Some(missing) = $cache.check(
                    &PermissionType::$permission,
                    &guild_id,
                    &owner,
                    &$interaction.user.id,
                    &upgraded_roles).await
                {
                    $interaction.create_interaction_response($ctx, |r|
                        r.kind(InteractionResponseType::ChannelMessageWithSource).interaction_response_data(|d|
                            d.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL).create_embed(|e|
                                e.description(format!("Missing permission `{}`", missing.as_display())).color(Color::RED)))).await?;
                    Ok(())
                } else {
                    $command
                }
            }
        )
    }
}

macro ensure_owner_base {
    ($interaction: expr, $handler: expr, $command: expr) => {
        if $interaction.user.id == $handler.owner_id {
            $command
        } else {
            Err(Error::new(BotError::OwnerOnly($interaction.user.id)))
        }
    }
}

pub async fn chat_input_router(handler: &Handler, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
    macro ensure_permission {
        ($permission: ident, $command: expr) => {
            ensure_permission_base!(ctx, handler.permissions, interaction, handler.application_id, $permission, $command)
        }
    }

    macro ensure_owner {
        ($command: expr) => {
            ensure_owner_base!(interaction, handler, $command)
        }
    }

    let (path, args) = decode::process(&interaction.data);

    debug!("received command {}", path);

    match path.as_str() {
        "info" => handler.updates.info_command(ctx, interaction).await,
        "makita sudo" => ensure_owner!(handler.permissions.makita_sudo(ctx, interaction).await),
        "makita checkupdates" => ensure_owner!(handler.updates.check_command(ctx, interaction).await),
        "makita update" => ensure_owner!(handler.updates.update_command(ctx, interaction).await),
        "makita restart" => ensure_owner!(handler.updates.restart_command(ctx, interaction).await),
        "makita debug" => ensure_owner!(crate::utils::debug_command(ctx, interaction).await),
        "permissions list" => ensure_permission!(ManagePermissions, handler.permissions.permissions_list(ctx, interaction).await),
        "permissions set" => ensure_permission!(ManagePermissions, handler.permissions.permissions_set(ctx, interaction, args).await),
        "permissions add" => ensure_permission!(ManagePermissions, handler.permissions.permissions_add(ctx, interaction, args).await),
        "permissions remove" => ensure_permission!(ManagePermissions, handler.permissions.permissions_remove(ctx, interaction, args).await),
        "previews add" => ensure_permission!(ManagePreviews, handler.previews.previews_add(ctx, interaction, args).await),
        "previews remove" => ensure_permission!(ManagePreviews, handler.previews.previews_remove(ctx, interaction, args).await),
        "previews list" => ensure_permission!(ManagePreviews, handler.previews.previews_list(ctx, interaction).await),
        "previews archive" => ensure_permission!(ManagePreviews, handler.previews.previews_archive(ctx, interaction, args).await),
        "previews view" => handler.previews.previews_view(ctx, interaction, args).await,
        _ => Ok(())
    }
}

pub async fn component_router(handler: &Handler, ctx: &BotContext, interaction: &MessageComponentInteraction) -> Result<()> {
    macro ensure_permission {
        ($permission: ident, $command: expr) => {
            ensure_permission_base!(ctx, handler.permissions, interaction, handler.application_id, $permission, $command)
        }
    }

    macro ensure_owner {
        ($command: expr) => {
            ensure_owner_base!(interaction, handler, $command)
        }
    }

    let (ty, _args) = parse_custom_id(&interaction.data.custom_id)?;

    debug!("received component with id {}", interaction.data.custom_id);

    use CustomIdType::*;
    match ty {
        Debug => ensure_owner!(crate::utils::debug_component(ctx, interaction).await),
        ListPermissions => ensure_permission!(ManagePermissions, handler.permissions.permissions_list_component(ctx, interaction).await),
    }
}

pub async fn message_router(handler: &Handler, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
    macro ensure_permission {
        ($permission: ident, $command: expr) => {
            ensure_permission_base!(ctx, handler.permissions, interaction, handler.application_id, $permission, $command)
        }
    }

    let message = interaction.data.resolved.messages.iter().next().ok_or_else(|| BotError::Internal(13))?.1;

    match interaction.data.name.as_str() {
        "Archive" => ensure_permission!(CreateArchive, handler.previews.previews_archive_context(ctx, interaction, message).await),
        _ => Ok(())
    }
}

pub async fn user_router(_handler: &Handler, _ctx: &BotContext, _interaction: &ApplicationCommandInteraction) -> Result<()> { Ok(()) }
