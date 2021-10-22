// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use chrono::NaiveDateTime;
use serenity::cache::Cache;
use serenity::CacheAndHttp;
use serenity::client::Context;
use serenity::http::{CacheHttp, Http};
use serenity::model::channel::MessageReference;
use serenity::model::id::{ChannelId, GuildId};
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::model::interactions::{InteractionApplicationCommandCallbackDataFlags, InteractionResponseType};
use serenity::model::interactions::message_component::{ButtonStyle, MessageComponentInteraction};
use sqlx::PgPool;
use crate::custom_ids::{build_custom_id, CustomIdType};

#[derive(Clone)]
pub struct BotContext {
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub pool: PgPool,
}

impl BotContext {
    pub fn build(ctx: Context, pool: PgPool) -> Self {
        Self {
            http: ctx.http,
            cache: ctx.cache,
            pool,
        }
    }

    pub fn from_cache_and_http(cah: &Arc<CacheAndHttp>, pool: &PgPool) -> Self {
        Self {
            http: cah.http.clone(),
            cache: cah.cache.clone(),
            pool: pool.clone()
        }
    }
}

impl AsRef<Http> for BotContext {
    fn as_ref(&self) -> &Http {
        &self.http
    }
}

impl AsRef<Cache> for BotContext {
    fn as_ref(&self) -> &Cache {
        &self.cache
    }
}

impl CacheHttp for BotContext {
    fn http(&self) -> &Http {
        &self.http
    }

    fn cache(&self) -> Option<&Arc<Cache>> {
        Some(&self.cache)
    }
}

pub fn remove_indexes<T>(vector: &mut Vec<T>, indexes: &[usize]) -> Vec<T> {
    let mut entries = Vec::new();
    for (offset, entry) in indexes.iter().enumerate() {
        entries.push(vector.remove(entry - offset));
    }
    entries
}

pub struct SqlId<T>(pub T) where T: From<u64> + Into<u64>;

pub fn default_arg<T, U: Default>(_: T) -> U { U::default() }

#[derive(Default)]
pub struct FollowupBuilder {
    title: Option<String>,
    description: Option<String>,
    ephemeral: bool,
}

macro builder_entry {
    ($ty: ty, $name: ident) => {
        #[allow(unused)]
        pub fn $name<T: Into<$ty>>(mut self, $name: T) -> Self {
            self.$name = Some($name.into());
            self
        }
    }
}

macro build_entry {
    ($self: expr, $builder: expr, $name: ident) => {
        match &$self.$name {
            Some(s) => { $builder.$name(s); }
            None => {}
        }
    }
}

impl FollowupBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn build_command_response<T: AsRef<Http>>(self, http: T, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.create_interaction_response(&http, |r| {
            r
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|m| {
                    m.create_embed(|e| {
                        build_entry!(self, e, title);
                        build_entry!(self, e, description);
                        e
                    });
                    if self.ephemeral {
                        m.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL);
                    }
                    m
                })
        }).await?;

        Ok(())
    }

    pub async fn build_command_edit<T: AsRef<Http>>(self, http: T, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.edit_original_interaction_response(&http, |m|
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            })
        ).await?;

        Ok(())
    }

    pub async fn build_command_followup<T: AsRef<Http>>(self, http: T, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.create_followup_message(&http, |m| {
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            });
            if self.ephemeral {
                m.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL);
            }
            m
        }).await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component_response<T: AsRef<Http>>(self, http: T, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction.create_interaction_response(&http, |r| {
            r
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|m| {
                    m.create_embed(|e| {
                        build_entry!(self, e, title);
                        build_entry!(self, e, description);
                        e
                    });
                    if self.ephemeral {
                        m.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL);
                    }
                    m
                })
        }).await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component_edit<T: AsRef<Http>>(self, http: T, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction.edit_original_interaction_response(&http, |m|
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            })
        ).await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component_followup<T: AsRef<Http>>(self, http: T, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction.create_followup_message(&http, |m| {
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            });
            if self.ephemeral {
                m.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL);
            }
            m
        }).await?;

        Ok(())
    }

    pub fn ephemeral(mut self) -> Self {
        self.ephemeral = true;
        self
    }

    builder_entry!(String, title);
    builder_entry!(String, description);
}

pub trait TryLink {
    fn try_link(&self) -> Option<String>;
}

impl TryLink for MessageReference {
    fn try_link(&self) -> Option<String> {
        self.guild_id.map(|guild| {
            match self.message_id {
                Some(msg) => format!("https://discord.com/channels/{}/{}/{}", guild, self.channel_id, msg),
                None => format!("https://discord.com/channels/{}/{}", guild, self.channel_id),
            }
        })
    }
}

pub trait Link {
    fn link(&self) -> String;
}

impl<T: TryLink> Link for T {
    fn link(&self) -> String {
        self.try_link().unwrap()
    }
}

impl Link for (GuildId, ChannelId) {
    fn link(&self) -> String {
        format!("https://discord.com/channels/{}/{}", self.0, self.1)
    }
}

// some debug command
pub async fn debug_command(ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
    interaction.create_followup_message(&ctx, |m| {
        m.content(".").components(|c| c.create_action_row(|a|
            a.create_button(|b| b.style(ButtonStyle::Primary)
                .custom_id(build_custom_id(&CustomIdType::Debug, &None)).label("abc"))))
    }).await?;

    Ok(())
}

// some debug component
pub async fn debug_component(ctx: &BotContext, interaction: &MessageComponentInteraction) -> Result<()> {
    interaction.create_followup_message(&ctx, |m| m.content("1")).await?;
    interaction.create_followup_message(&ctx, |m| m.content("2")).await?;
    interaction.create_followup_message(&ctx, |m| m.content("3")).await?;

    Ok(())
}

pub fn naive_now() -> NaiveDateTime {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32)
}

pub async fn defer_command<T: AsRef<Http>>(http: &T, interaction: &ApplicationCommandInteraction) -> Result<()> {
    interaction.create_interaction_response(http.as_ref(), |r| r.kind(InteractionResponseType::DeferredChannelMessageWithSource)).await?;
    Ok(())
}

pub async fn defer_component<T: AsRef<Http>>(http: &T, interaction: &MessageComponentInteraction) -> Result<()> {
    interaction.create_interaction_response(http.as_ref(), |r| r.kind(InteractionResponseType::DeferredUpdateMessage)).await?;
    Ok(())
}