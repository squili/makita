// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::prelude::*;
use anyhow::Result;
use serde::Deserializer;
use serenity::cache::Cache;
use serenity::client::Context;
use serenity::http::{CacheHttp, Http};
use serenity::model::channel::{Channel, MessageReference};
use serenity::model::guild::{Guild, Role};
use serenity::model::id::{ChannelId, GuildId, RoleId};
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::prelude::interaction::message_component::MessageComponentInteraction;
use serenity::model::prelude::interaction::{InteractionResponseType, MessageFlags};
use serenity::CacheAndHttp;
use sqlx::PgPool;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

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
            pool: pool.clone(),
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

pub struct SqlId<T>(pub T)
where
    T: From<u64> + Into<u64>;

pub fn default_arg<T, U: Default>(_: T) -> U {
    U::default()
}

#[derive(Default)]
pub struct FollowupBuilder {
    title: Option<String>,
    description: Option<String>,
    ephemeral: bool,
}

macro_rules! builder_entry {
    ($ty: ty, $name: ident) => {
        #[allow(unused)]
        pub fn $name<T: Into<$ty>>(mut self, $name: T) -> Self {
            self.$name = Some($name.into());
            self
        }
    };
}

macro_rules! build_entry {
    ($self: expr, $builder: expr, $name: ident) => {
        match &$self.$name {
            Some(s) => {
                $builder.$name(s);
            }
            None => {}
        }
    };
}

impl FollowupBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn build_command_somehow<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &ApplicationCommandInteraction,
        followup: bool,
    ) -> Result<()> {
        if followup {
            self.build_command_followup(http, interaction).await
        } else {
            self.build_command_response(http, interaction).await
        }
    }

    pub async fn build_command_response<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &ApplicationCommandInteraction,
    ) -> Result<()> {
        interaction
            .create_interaction_response(&http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| {
                        m.embed(|e| {
                            build_entry!(self, e, title);
                            build_entry!(self, e, description);
                            e
                        });
                        if self.ephemeral {
                            m.flags(MessageFlags::EPHEMERAL);
                        }
                        m
                    })
            })
            .await?;

        Ok(())
    }

    pub async fn build_command_edit<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &ApplicationCommandInteraction,
    ) -> Result<()> {
        interaction
            .edit_original_interaction_response(&http, |m| {
                m.embed(|e| {
                    build_entry!(self, e, title);
                    build_entry!(self, e, description);
                    e
                })
            })
            .await?;

        Ok(())
    }

    pub async fn build_command_followup<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &ApplicationCommandInteraction,
    ) -> Result<()> {
        interaction
            .create_followup_message(&http, |m| {
                m.embed(|e| {
                    build_entry!(self, e, title);
                    build_entry!(self, e, description);
                    e
                });
                if self.ephemeral {
                    m.flags(MessageFlags::EPHEMERAL);
                }
                m
            })
            .await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component_response<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &MessageComponentInteraction,
    ) -> Result<()> {
        interaction
            .create_interaction_response(&http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| {
                        m.embed(|e| {
                            build_entry!(self, e, title);
                            build_entry!(self, e, description);
                            e
                        });
                        if self.ephemeral {
                            m.flags(MessageFlags::EPHEMERAL);
                        }
                        m
                    })
            })
            .await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component_edit<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &MessageComponentInteraction,
    ) -> Result<()> {
        interaction
            .edit_original_interaction_response(&http, |m| {
                m.embed(|e| {
                    build_entry!(self, e, title);
                    build_entry!(self, e, description);
                    e
                })
            })
            .await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component_followup<T: AsRef<Http>>(
        self,
        http: T,
        interaction: &MessageComponentInteraction,
    ) -> Result<()> {
        interaction
            .create_followup_message(&http, |m| {
                m.embed(|e| {
                    build_entry!(self, e, title);
                    build_entry!(self, e, description);
                    e
                });
                if self.ephemeral {
                    m.flags(MessageFlags::EPHEMERAL);
                }
                m
            })
            .await?;

        Ok(())
    }

    pub fn ephemeral(mut self) -> Self {
        self.ephemeral = true;
        self
    }

    pub fn set_ephemeral(mut self, value: bool) -> Self {
        self.ephemeral = value;
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
        self.guild_id.map(|guild| match self.message_id {
            Some(msg) => format!(
                "https://discord.com/channels/{}/{}/{}",
                guild, self.channel_id, msg
            ),
            None => format!("https://discord.com/channels/{}/{}", guild, self.channel_id),
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

pub async fn defer_command<T: AsRef<Http>>(
    http: &T,
    interaction: &ApplicationCommandInteraction,
) -> Result<()> {
    interaction
        .create_interaction_response(http.as_ref(), |r| {
            r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
        })
        .await?;
    Ok(())
}

pub async fn defer_component<T: AsRef<Http>>(
    http: &T,
    interaction: &MessageComponentInteraction,
) -> Result<()> {
    interaction
        .create_interaction_response(http.as_ref(), |r| {
            r.kind(InteractionResponseType::DeferredUpdateMessage)
        })
        .await?;
    Ok(())
}

#[derive(Debug)]
pub enum OptionalOption<T> {
    Present(Option<T>),
    Missing,
}

impl<T> Default for OptionalOption<T> {
    fn default() -> Self {
        OptionalOption::Missing
    }
}

impl<'de, T> Deserialize<'de> for OptionalOption<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer) {
            Ok(val) => OptionalOption::Present(val),
            Err(_) => OptionalOption::Missing,
        })
    }
}

pub fn highest_role(cached: &HashMap<RoleId, Role>, roles: &[RoleId]) -> i64 {
    roles
        .iter()
        .map(|r| cached.get(r).map(|r| r.position).unwrap_or(0))
        .max()
        .unwrap_or(0)
}

pub fn parse_duration(from: &str) -> Option<Duration> {
    let mut duration = Duration::from_secs(0);
    let mut num_array = String::new();
    for char in from.chars() {
        if ('0'..='9').contains(&char) {
            num_array.push(char)
        } else {
            let quantity = u64::from_str(&num_array).ok()?;
            match char {
                'd' => duration += Duration::from_secs(quantity * 60 * 60 * 24),
                'h' => duration += Duration::from_secs(quantity * 60 * 60),
                'm' => duration += Duration::from_secs(quantity * 60),
                's' => duration += Duration::from_secs(quantity),
                _ => return None,
            }
        }
    }

    Some(duration)
}

// we try our best to find a channel that everyone can see
pub fn link_guild(guild: &Guild, hint: &ChannelId) -> String {
    format!(
        "[{}]({})",
        guild.name,
        (
            guild.id,
            guild.rules_channel_id.unwrap_or_else(|| {
                guild
                    .channels
                    .values()
                    .find(|channel| match channel {
                        Channel::Guild(channel) => {
                            channel
                                .permission_overwrites
                                .iter()
                                .filter(|overwrite| overwrite.deny.view_channel())
                                .count()
                                == 0
                        }
                        _ => false,
                    })
                    .map(|c| c.id())
                    .unwrap_or_else(|| *hint)
            })
        )
            .link()
    )
}
