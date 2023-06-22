// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::custom_ids::{parse_custom_id, CustomIdType};
use crate::debug;
use crate::decode;
use crate::handler::Handler;
use crate::modules::PermissionType;
use crate::prelude::*;
use anyhow::{Error, Result};
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::prelude::interaction::message_component::MessageComponentInteraction;
use serenity::model::prelude::interaction::{InteractionResponseType, MessageFlags};
use serenity::utils::Color;

macro_rules! ensure_guild {
    ($interaction: expr, $command: expr) => {
        match $interaction.guild_id {
            Some(_) => $command,
            None => Err(Error::new(BotError::GuildOnly)),
        }
    };
}

macro_rules! ensure_permission_base {
    ($ctx: expr, $cache: expr, $interaction: expr, $application_id: expr, $permission: ident, $command: expr) => {
        ensure_guild!($interaction, {
            let guild_id = $interaction.guild_id.unwrap();
            let roles = &$interaction
                .member
                .as_ref()
                .ok_or(BotError::CacheMissing)?
                .roles;
            let mut upgraded_roles = Vec::new();
            let owner = $ctx
                .cache
                .guild_field(&guild_id, |g| {
                    for role in roles {
                        match g.roles.get(&role) {
                            Some(data) => upgraded_roles.push(data.clone()),
                            None => {}
                        }
                    }
                    g.owner_id
                })
                .ok_or(BotError::CacheMissing)?;
            if let Some(missing) = $cache
                .check(
                    &PermissionType::$permission,
                    &guild_id,
                    &owner,
                    &$interaction.user.id,
                    &upgraded_roles,
                )
                .await
            {
                $interaction
                    .create_interaction_response($ctx, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|d| {
                                d.flags(MessageFlags::EPHEMERAL).embed(|e| {
                                    e.description(format!(
                                        "Missing permission `{}`",
                                        missing.as_display()
                                    ))
                                    .color(Color::RED)
                                })
                            })
                    })
                    .await?;
                Ok(())
            } else {
                $command
            }
        })
    };
}

pub async fn chat_input_router(
    handler: &Handler,
    ctx: &BotContext,
    interaction: &ApplicationCommandInteraction,
) -> Result<()> {
    macro_rules! ensure_permission {
        ($permission: ident, $command: expr) => {
            ensure_permission_base!(
                ctx,
                handler.permissions,
                interaction,
                handler.application_id,
                $permission,
                $command
            )
        };
    }

    let (path, args) = decode::process(&interaction.data);

    debug!("received command {}", path);

    match path.as_str() {
        "info" => handler.updates.info_command(ctx, interaction).await,
        "permissions list" => ensure_permission!(
            ManagePermissions,
            handler.permissions.permissions_list(ctx, interaction).await
        ),
        "permissions set" => ensure_permission!(
            ManagePermissions,
            handler
                .permissions
                .permissions_set(ctx, interaction, args)
                .await
        ),
        "permissions add" => ensure_permission!(
            ManagePermissions,
            handler
                .permissions
                .permissions_add(ctx, interaction, args)
                .await
        ),
        "permissions remove" => ensure_permission!(
            ManagePermissions,
            handler
                .permissions
                .permissions_remove(ctx, interaction, args)
                .await
        ),
        "previews add" => ensure_permission!(
            ManagePreviews,
            handler.previews.previews_add(ctx, interaction, args).await
        ),
        "previews remove" => ensure_permission!(
            ManagePreviews,
            handler
                .previews
                .previews_remove(ctx, interaction, args)
                .await
        ),
        "previews list" => ensure_permission!(
            ManagePreviews,
            handler.previews.previews_list(ctx, interaction).await
        ),
        "previews archive" => ensure_permission!(
            ManagePreviews,
            handler
                .previews
                .previews_archive(ctx, interaction, args)
                .await
        ),
        "previews view" => handler.previews.previews_view(ctx, interaction, args).await,
        "timeout" => ensure_permission!(
            Timeout,
            handler.utils.timeout_command(ctx, interaction, args).await
        ),
        "untimeout" => ensure_permission!(
            Timeout,
            handler
                .utils
                .untimeout_command(ctx, interaction, args)
                .await
        ),
        _ => Ok(()),
    }
}

pub async fn component_router(
    handler: &Handler,
    ctx: &BotContext,
    interaction: &MessageComponentInteraction,
) -> Result<()> {
    macro_rules! ensure_permission {
        ($permission: ident, $command: expr) => {
            ensure_permission_base!(
                ctx,
                handler.permissions,
                interaction,
                handler.application_id,
                $permission,
                $command
            )
        };
    }

    let (ty, _args) = parse_custom_id(&interaction.data.custom_id)?;

    debug!("received component with id {}", interaction.data.custom_id);

    use CustomIdType::*;
    match ty {
        ListPermissions => ensure_permission!(
            ManagePermissions,
            handler
                .permissions
                .permissions_list_component(ctx, interaction)
                .await
        ),
    }
}

pub async fn message_router(
    handler: &Handler,
    ctx: &BotContext,
    interaction: &ApplicationCommandInteraction,
) -> Result<()> {
    macro_rules! ensure_permission {
        ($permission: ident, $command: expr) => {
            ensure_permission_base!(
                ctx,
                handler.permissions,
                interaction,
                handler.application_id,
                $permission,
                $command
            )
        };
    }

    let message = interaction
        .data
        .resolved
        .messages
        .iter()
        .next()
        .ok_or(BotError::Internal(13))?
        .1;

    match interaction.data.name.as_str() {
        "Archive" => ensure_permission!(
            CreateArchive,
            handler
                .previews
                .previews_archive_context(ctx, interaction, message)
                .await
        ),
        _ => Ok(()),
    }
}

pub async fn user_router(
    _handler: &Handler,
    _ctx: &BotContext,
    _interaction: &ApplicationCommandInteraction,
) -> Result<()> {
    Ok(())
}
