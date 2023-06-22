// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::modules::{PermissionsModule, PreviewsModule, UpdatesModule, UtilsModule};
use crate::prelude::*;
use crate::router;
use log::{error, info};
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::channel::{GuildChannel, Message};
use serenity::model::gateway::{Activity, Ready};
use serenity::model::guild::Guild;
use serenity::model::id::{ApplicationId, UserId};
use serenity::model::prelude::command::CommandType;
use serenity::model::prelude::interaction::Interaction;
use serenity::utils::Color;
use sqlx::{Pool, Postgres};

pub struct Handler {
    pub pool: Pool<Postgres>,
    pub application_id: ApplicationId,
    pub owner_id: UserId,
    pub updates: Arc<UpdatesModule>,
    pub permissions: Arc<PermissionsModule>,
    pub previews: Arc<PreviewsModule>,
    pub utils: Arc<UtilsModule>,
}

macro_rules! handler_log {
    ($name: expr, $thing: expr) => {
        if let Err(e) = $thing {
            error!("Error in {}: {:?}", $name, e)
        }
    };
}

macro_rules! pass_event {
    ($name: expr, $instance: expr, $func: path, $($passthrough: expr),*) => {
        async {
            handler_log!($name, $func(&$instance, $($passthrough),*).await);
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn channel_delete(&self, ctx: Context, channel: &GuildChannel) {
        let b_ctx = BotContext::build(ctx, self.pool.clone());
        tokio::join! {
            pass_event!("Previews", &self.previews, PreviewsModule::channel_delete, &b_ctx, channel),
        };
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _: bool) {
        let b_ctx = BotContext::build(ctx, self.pool.clone());
        handler_log!(
            format!("Guild Create Event ID {}", guild.id),
            sqlx::query("insert into Guilds (id) values ($1) on conflict on constraint id_unique do update set expiration = null")
                .bind(guild.id.0 as i64)
                .execute(&self.pool)
                .await
        );
        tokio::join! {
            pass_event!("Previews", &self.previews, PreviewsModule::guild_data, &b_ctx, &guild),
        };
    }

    async fn message(&self, ctx: Context, message: Message) {
        let b_ctx = BotContext::build(ctx, self.pool.clone());
        tokio::join! {
            pass_event!("Previews", &self.previews, PreviewsModule::message, &b_ctx, &message),
        };
    }
    async fn ready(&self, ctx: Context, _: Ready) {
        info!("received ready event");
        ctx.shard
            .set_activity(Some(Activity::listening("your inner thoughts")));
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let b_ctx = BotContext::build(ctx, self.pool.clone());
        match interaction {
            Interaction::ApplicationCommand(command) => {
                match match command.data.kind {
                    CommandType::ChatInput => {
                        router::chat_input_router(self, &b_ctx, &command).await
                    }
                    CommandType::User => router::user_router(self, &b_ctx, &command).await,
                    CommandType::Message => router::message_router(self, &b_ctx, &command).await,
                    _ => Ok(()),
                } {
                    Ok(_) => {}
                    Err(err) => handler_log!(
                        "Command Error Response",
                        command
                            .create_followup_message(&b_ctx, |f| f.embed(|e| e
                                .description(format!(
                                    "{}{}",
                                    if err.is::<BotError>() {
                                        ""
                                    } else {
                                        "Internal error: "
                                    },
                                    err
                                ))
                                .color(Color::RED)))
                            .await
                    ),
                }
            }
            Interaction::MessageComponent(component) => {
                if !component.data.custom_id.starts_with("MAK;") {
                    return;
                }
                match router::component_router(self, &b_ctx, &component).await {
                    Ok(_) => {}
                    Err(err) => handler_log!(
                        "Message Component Error Response",
                        component
                            .create_followup_message(&b_ctx, |r| r.embed(|e| e
                                .description(format!(
                                    "{}{}",
                                    if err.is::<BotError>() {
                                        ""
                                    } else {
                                        "Internal error: "
                                    },
                                    err
                                ))
                                .color(Color::RED)))
                            .await
                    ),
                }
            }
            _ => {}
        }
    }
}
