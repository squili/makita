// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::borrow::Cow;
use anyhow::{Error, Result};
use serenity::client::Context;
use serenity::model::channel::{Message, GuildChannel, Attachment};
use serenity::model::id::{GuildId, ChannelId, MessageId, UserId};
use sqlx::{PgPool, Row};
use regex::Regex;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::str::FromStr;
use serenity::builder::CreateEmbed;
use serenity::http::AttachmentType;
use serenity::model::guild::Guild;
use crate::utils::{remove_indexes, SqlId, default_arg, FollowupBuilder};
use sqlx::postgres::PgRow;
use crate::decode::SlashMap;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::model::misc::Mentionable;
use crate::error::BotError;
use crate::macros::impl_cache_functions;

pub struct PreviewsModule {
    pool: PgPool,
    link_regex: Regex,
    cache: RwLock<HashMap<GuildId, Vec<ChannelId>>>,
}

impl PreviewsModule {
    pub fn new(pool: PgPool) -> Result<Self> {
        Ok(Self {
            pool,
            link_regex: Regex::new(r"https://(?:\w+\.)?discord(?:app)?.com/channels/(\d+)/(\d+)/(\d+)")?,
            cache: Default::default()
        })
    }
}

impl PreviewsModule {
    impl_cache_functions!(read_cache, write_cache, write_cache_async, GuildId, Vec<ChannelId>, cache, default_arg);

    pub async fn initialize(&self) -> Result<()> {
        // load data from db
        let mut handle = self.cache.write().await;
        let rows = sqlx::query("select guild_id, channel_id from PreviewChannels")
            .map(|row: PgRow| {
                (row.get::<SqlId<GuildId>, &str>("guild_id").0, row.get::<SqlId<ChannelId>, &str>("channel_id").0)
            })
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            match handle.get_mut(&row.0) {
                Some(s) => {
                    s.push(row.1);
                }
                None => {
                    handle.insert(row.0, vec![row.1]);
                }
            }
        }

        Ok(())
    }

    async fn preview(&self, ctx: &Context, original: &str, from_user: &UserId, from_guild: &Option<GuildId>,
                     guild: GuildId, channel: ChannelId, message: MessageId) -> Result<(Vec<CreateEmbed>, Vec<Attachment>)> {
        match ctx.cache.guild(&guild).await {
            None => Err(Error::new(BotError::NotFound("Server".to_string()))),
            Some(guild) => {
                match guild.member(&ctx.http, *from_user).await {
                    Err(_) => Err(Error::new(BotError::Generic("You must be in a server to preview messages from it".to_string()))),
                    Ok(member) => {
                        if !member.roles(&ctx.cache).await.ok_or(BotError::CacheMissing)?
                            .iter().any(|role| role.permissions.administrator()) {
                            if !guild.user_permissions_in(guild.channels.get(&channel).ok_or(BotError::CacheMissing)?, &member)?.read_messages() {
                                return Err(Error::new(BotError::Generic("You do not have permission to view this message".to_string())));
                            }
                        }
                        let message = channel.message(&ctx.http, &message).await
                            .map_err(|_| Error::new(BotError::NotFound("Message".to_string())))?;

                        let mut embed = CreateEmbed::default();
                        embed
                            .description(&message.content)
                            .author(|author|
                                author
                                    .name(format!("{}#{}", message.author.name, message.author.discriminator))
                                    .icon_url(message.author.avatar_url().unwrap_or_else(|| message.author.default_avatar_url()))
                                    .url(original)
                            ).timestamp(&message.timestamp)
                            .field("Channel", channel.mention(), true)
                            .field("Author", message.author.mention(), true);

                        if match from_guild {
                            Some(s) => *s != guild.id,
                            None => true
                        } {
                            embed.field("Guild", guild.name, true);
                        }

                        let mut embeds = vec![embed];

                        for embed in message.embeds {
                            // copy embed data
                            let mut builder = CreateEmbed::default();
                            builder.color(embed.colour);
                            if let Some(title) = &embed.title { builder.title(title); }
                            if let Some(description) = &embed.description { builder.description(description); }
                            if let Some(url) = &embed.url { builder.url(url); }
                            if let Some(timestamp) = &embed.timestamp { builder.timestamp(timestamp.clone()); }
                            if let Some(image) = embed.image { builder.image(image.url); };
                            if let Some(thumbnail) = embed.thumbnail { builder.thumbnail(thumbnail.url); };
                            if let Some(footer) = embed.footer {
                                builder.footer(|builder| {
                                    builder.text(footer.text);
                                    if let Some(icon_url) = &footer.icon_url { builder.icon_url(icon_url); }
                                    builder
                                });
                            };
                            if let Some(author) = embed.author {
                                builder.author(|builder| {
                                    builder.name(author.name);
                                    if let Some(url) = &author.url { builder.url(url); }
                                    if let Some(icon_url) = &author.icon_url { builder.icon_url(icon_url); }
                                    builder
                                });
                            };
                            for field in embed.fields {
                                builder.field(field.name, field.value, field.inline);
                            }
                            embeds.push(builder)
                        }

                        Ok((embeds, message.attachments.into_iter().filter(|s| s.size < 8388246).collect()))
                    }
                }
            }
        }
    }

    pub async fn message(&self, ctx: &Context, message: &Message) -> Result<()> {
        // ignore dms
        if matches!(message.guild_id, None) {
            return Ok(());
        }

        // detect if we should scan
        if !self.read_cache(&message.guild_id.unwrap(), |cached: &Vec<ChannelId>| {
            cached.contains(&message.channel_id)
        }).await {
            return Ok(());
        }

        for item in self.link_regex.captures_iter(&message.content) {
            match self.preview(&ctx,
                item.get(0).unwrap().as_str(),
                &message.author.id,
                &message.guild_id,
                GuildId(u64::from_str(item.get(1).ok_or(BotError::Internal(0))?.as_str()).map_err(|_| BotError::Internal(1))?),
                ChannelId(u64::from_str(item.get(2).ok_or(BotError::Internal(2))?.as_str()).map_err(|_| BotError::Internal(3))?),
                MessageId(u64::from_str(item.get(3).ok_or(BotError::Internal(4))?.as_str()).map_err(|_| BotError::Internal(5))?)
            ).await {
                Ok((embeds, attachments)) => {
                    let chunks = embeds.chunks(10);
                    let mut downloaded = Vec::with_capacity(attachments.len());
                    for attachment in attachments {
                        let bytes = attachment.download().await?;
                        downloaded.push(AttachmentType::Bytes { data: Cow::from(bytes), filename: attachment.filename })
                    }
                    for chunk in chunks {
                        message.channel_id.send_message(&ctx.http, |m| m.set_embeds(chunk.to_vec())).await?;
                    }
                    if downloaded.len() > 0 {
                        message.channel_id.send_message(&ctx.http, |m| m.files(downloaded)).await?;
                    }
                }
                Err(err) => if !err.is::<BotError>() {
                    return Err(err);
                }
            }
        }

        Ok(())
    }

    pub async fn guild_data(&self, guild: &Guild) -> Result<()> {
        // remove invalid channels
        let entries = self.write_cache(&guild.id, |cached: &mut Vec<ChannelId>| {
            let mut indexes = Vec::new();
            for (index, channel) in cached.iter().enumerate() {
                if !guild.channels.contains_key(&channel) {
                    indexes.push(index);
                }
            }
            remove_indexes(cached, &indexes)
        }).await;

        for entry in entries {
            sqlx::query("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
                .bind(guild.id.0 as i64)
                .bind(entry.0 as i64)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn channel_delete(&self, channel: &GuildChannel) -> Result<()> {
        // remove invalid channel
        let remove = self.read_cache(&channel.guild_id, |cached: &Vec<ChannelId>| {
            cached.binary_search(&channel.id).ok()
        }).await;
        match remove {
            Some(s) => {
                self.write_cache(&channel.guild_id, |cached: &mut Vec<ChannelId>| {
                    cached.remove(s);
                }).await;
                sqlx::query("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
                    .bind(SqlId(channel.guild_id))
                    .bind(SqlId(channel.id))
                    .execute(&self.pool)
                    .await?;
            }
            None => {}
        }
        Ok(())
    }

    pub async fn previews_add(&self, ctx: &Context, interaction: &ApplicationCommandInteraction, args: SlashMap) -> Result<()> {
        let target = args.get_channel("target")?;
        let guild_id = interaction.guild_id.ok_or(BotError::GuildOnly)?;

        if self.write_cache(&guild_id, |data| {
            if data.contains(&target.id) {
                true
            } else {
                data.push(target.id);
                false
            }
        }).await {
            return Err(Error::new(BotError::Generic("Channel already added".to_string())))
        }

        sqlx::query("insert into PreviewChannels (guild_id, channel_id) values ($1, $2) on conflict do nothing")
            .bind(SqlId(guild_id))
            .bind(SqlId(target.id))
            .execute(&self.pool)
            .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command(&ctx.http, &interaction)
            .await
    }

    pub async fn previews_remove(&self, ctx: &Context, interaction: &ApplicationCommandInteraction, args: SlashMap) -> Result<()> {
        let target = args.get_channel("target")?;
        let guild_id = interaction.guild_id.ok_or(BotError::GuildOnly)?;

        if self.write_cache(&guild_id, |data| {
            match data.binary_search(&target.id) {
                Ok(s) => {
                    data.remove(s);
                    false
                },
                Err(_) => true,
            }
        }).await {
            return Err(Error::new(BotError::Generic("Channel not in previews".to_string())))
        }

        sqlx::query("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
            .bind(SqlId(guild_id))
            .bind(SqlId(target.id))
            .execute(&self.pool)
            .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command(&ctx.http, &interaction)
            .await
    }

    pub async fn previews_list(&self, ctx: &Context, interaction: &ApplicationCommandInteraction) -> Result<()> {
        let mut items = vec!["**Channels**".to_string()];

        self.read_cache(&interaction.guild_id.ok_or(BotError::GuildOnly)?, |data| {
            for channel in data {
                items.push(channel.mention().to_string())
            }
        }).await;

        if items.len() == 1 {
            return FollowupBuilder::new()
                .description("No channels")
                .build_command(&ctx.http, interaction)
                .await;
        }

        FollowupBuilder::new()
            .description(items.join("\n"))
            .build_command(&ctx.http, interaction)
            .await
    }

    pub async fn previews_view(&self, ctx: &Context, interaction: &ApplicationCommandInteraction, args: SlashMap) -> Result<()> {
        let target = args.get_string("target")?;

        let captures = self.link_regex.captures(&target).ok_or(BotError::Generic("Malformed link".to_string()))?;

        let (embeds, attachments) = self.preview(&ctx,
                     captures.get(0).unwrap().as_str(),
                     &interaction.user.id,
                     &interaction.guild_id,
                     GuildId(u64::from_str(captures.get(1).ok_or(BotError::Internal(6))?.as_str()).map_err(|_| BotError::Internal(7))?),
                     ChannelId(u64::from_str(captures.get(2).ok_or(BotError::Internal(8))?.as_str()).map_err(|_| BotError::Internal(9))?),
                     MessageId(u64::from_str(captures.get(3).ok_or(BotError::Internal(10))?.as_str()).map_err(|_| BotError::Internal(11))?)
        ).await?;

        let chunks = embeds.chunks(10);
        let mut downloaded = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let bytes = attachment.download().await?;
            downloaded.push(AttachmentType::Bytes { data: Cow::from(bytes), filename: attachment.filename })
        }
        for chunk in chunks {
            interaction.create_followup_message(&ctx.http, |m| m.embeds(chunk.to_vec())).await?;
        }
        if downloaded.len() > 0 {
            interaction.create_followup_message(&ctx.http, |m| m.files(downloaded)).await?;
        }

        Ok(())
    }
}
