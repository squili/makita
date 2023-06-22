// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::decode::SlashMap;
use crate::impl_cache_functions;
use crate::prelude::*;
use crate::tasks::TaskMessage;
use crate::utils::{
    default_arg, defer_command, link_guild, remove_indexes, BotContext, FollowupBuilder, Link,
    SqlId,
};
use anyhow::{Error, Result};
use regex::Regex;
use serenity::builder::CreateEmbed;
use serenity::model::channel::{
    Attachment, AttachmentType, Channel, GuildChannel, Message, MessageFlags, MessageType,
};
use serenity::model::guild::Guild;
use serenity::model::id::{ChannelId, GuildId, MessageId, UserId};
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use serenity::prelude::Mentionable;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use std::borrow::Cow;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::{broadcast, RwLock};

#[derive(Default)]
pub struct PreviewsConfig {
    pub auto_channels: Vec<ChannelId>,
    pub archive_channel: Option<ChannelId>,
}

pub struct PreviewsModule {
    link_regex: Regex,
    cache: RwLock<HashMap<GuildId, PreviewsConfig>>,
}

impl PreviewsModule {
    pub fn new() -> Result<Self> {
        Ok(Self {
            link_regex: Regex::new(
                r"https://(?:\w+\.)?discord(?:app)?.com/channels/(\d+)/(\d+)/(\d+)",
            )?,
            cache: Default::default(),
        })
    }
}

impl PreviewsModule {
    impl_cache_functions!(
        read_cache,
        write_cache,
        write_cache_async,
        GuildId,
        PreviewsConfig,
        cache,
        default_arg
    );

    pub async fn initialize(
        instance: Arc<Self>,
        mut task_rx: broadcast::Receiver<TaskMessage>,
        pool: &PgPool,
    ) -> Result<()> {
        // load auto channels from db
        let rows = sqlx::query("select guild_id, channel_id from PreviewChannels")
            .map(|row: PgRow| {
                (
                    row.get::<SqlId<GuildId>, _>("guild_id").0,
                    row.get::<SqlId<ChannelId>, _>("channel_id").0,
                )
            })
            .fetch_all(pool)
            .await?;

        for row in rows {
            instance
                .write_cache(&row.0, |data| {
                    data.auto_channels.push(row.1);
                })
                .await;
        }

        // load archive channels from db
        let rows = sqlx::query("select guild_id, channel_id from ArchiveChannel")
            .map(|row: PgRow| {
                (
                    row.get::<SqlId<GuildId>, _>("guild_id").0,
                    row.get::<SqlId<ChannelId>, _>("channel_id").0,
                )
            })
            .fetch_all(pool)
            .await?;

        for row in rows {
            instance
                .write_cache(&row.0, |data| {
                    data.archive_channel = Some(row.1);
                })
                .await;
        }

        // task event handling
        tokio::spawn(async move {
            loop {
                let msg = task_rx.recv().await;
                match msg {
                    Ok(TaskMessage::Kill) | Err(_) => break,
                    Ok(TaskMessage::DestroyGuild(g)) => {
                        instance.cache.write().await.remove(&g);
                    }
                }
            }
        });

        Ok(())
    }

    async fn derive_embed(
        ctx: &BotContext,
        message: &Message,
        foreign: Option<&Guild>,
    ) -> CreateEmbed {
        macro_rules! filter_kind {
            ($($ty: ident),*) => {
                match message.kind {
                    $(MessageType::$ty)|* => true,
                    _ => false
                }
            }
        }

        let flags = message.flags.unwrap_or_else(MessageFlags::empty);
        let maybe_link_foreign = match foreign {
            Some(guild) => format!(" {}", link_guild(guild, &message.channel_id)),
            None => "".to_string(),
        };
        let link_foreign_or_you = match maybe_link_foreign.len() {
            0 => "This server ",
            _ => maybe_link_foreign.as_str(),
        };

        let mut embed = CreateEmbed::default();
        embed.description("\u{200B}"); // make sure there's a description field

        if filter_kind!(
            GroupRecipientAddition,
            GroupRecipientRemoval,
            GroupCallCreation,
            GroupNameUpdate,
            GroupIconUpdate
        ) {
            UserId(719046554744520754)
                .create_dm_channel(&ctx)
                .await
                .unwrap()
                .say(
                    &ctx,
                    format!("type {:?} spotted: {}", message.kind, message.link()),
                )
                .await
                .unwrap();
            embed.description("This is awkward... I shouldn't be able to see this message, yet I do. How will I resolve this paradox?");
            return embed;
        }

        if filter_kind!(
            Unknown,
            GuildInviteReminder,
            GuildDiscoveryGracePeriodInitialWarning,
            GuildDiscoveryGracePeriodFinalWarning,
            ThreadStarterMessage,
            ContextMenuCommand
        ) {
            UserId(719046554744520754)
                .create_dm_channel(&ctx)
                .await
                .unwrap()
                .say(
                    &ctx,
                    format!("type {:?} spotted: {}", message.kind, message.link()),
                )
                .await
                .unwrap();
            embed.description("Unsupported message type. This has incident has been reported.");
            return embed;
        }

        // populate author
        if filter_kind!(
            Regular,
            PinsAdd,
            NitroBoost,
            NitroTier1,
            NitroTier2,
            NitroTier3,
            MemberJoin,
            ChannelFollowAdd,
            ThreadCreated,
            InlineReply,
            ChatInputCommand,
            ContextMenuCommand
        ) {
            embed.author(|author| {
                author
                    .name(match message.author.discriminator {
                        0 => message.author.name.clone(),
                        _ => format!(
                            "{}#{:04}",
                            message.author.name, message.author.discriminator
                        ),
                    })
                    .url(if flags.contains(MessageFlags::IS_CROSSPOST) {
                        message.message_reference.as_ref().unwrap().link()
                    } else {
                        message.link()
                    })
                    .icon_url(
                        message
                            .author
                            .avatar_url()
                            .unwrap_or_else(|| message.author.default_avatar_url()),
                    )
            });
        }

        // standard-type messages
        if filter_kind!(Regular, InlineReply, ChatInputCommand) {
            embed
                .description(match &message.referenced_message {
                    Some(referenced) => format!(
                        "{}\n[Reply to]({})",
                        message.content,
                        referenced.link_ensured(&ctx).await
                    ),
                    None => message.content.clone(),
                })
                .field("Channel", message.channel_id.mention(), true)
                .field("Author", message.author.mention(), true);
            if matches!(foreign, Some(_)) {
                embed.field("Guild", maybe_link_foreign.clone(), true);
            }
        }

        // timestamp
        if filter_kind!(
            Regular,
            InlineReply,
            NitroBoost,
            NitroTier1,
            NitroTier2,
            NitroTier3,
            PinsAdd,
            ChannelFollowAdd,
            ThreadCreated
        ) {
            embed.timestamp(&message.timestamp);
        }

        // nitro
        if filter_kind!(NitroBoost, NitroTier1, NitroTier2, NitroTier3) {
            embed.description(format!(
                "{} boosted the server{}!",
                message.author.mention(),
                match message.kind {
                    MessageType::NitroTier1 => ", achieving tier 1",
                    MessageType::NitroTier2 => ", achieving tier 2",
                    MessageType::NitroTier3 => ", achieving tier 3",
                    _ => "",
                }
            ));
        }

        // discovery enrollment
        if filter_kind!(GuildDiscoveryDisqualified, GuildDiscoveryRequalified) {
            embed.description(format!(
                "{}has been {}qualified {} discovery.",
                link_foreign_or_you,
                if message.kind == MessageType::GuildDiscoveryDisqualified {
                    "dis"
                } else {
                    "re"
                },
                if message.kind == MessageType::GuildDiscoveryDisqualified {
                    "from"
                } else {
                    "for"
                }
            ));
        }

        // single-match messages
        match message.kind {
            MessageType::PinsAdd => {
                let reference = message.message_reference.as_ref().unwrap();
                embed.description(format!(
                    "{} pinned [a message]({}) in {}{}",
                    message.author.mention(),
                    reference
                        .message_id
                        .unwrap()
                        .link(reference.channel_id, message.guild_id),
                    maybe_link_foreign,
                    message.channel_id.mention()
                ));
            }
            MessageType::MemberJoin => {
                embed.description(format!(
                    "{} joined {}on <t:{time}:f>, <t:{time}:R>",
                    message.author.mention(),
                    maybe_link_foreign,
                    time = message.timestamp.unix_timestamp() as u64
                ));
            }
            MessageType::ChannelFollowAdd => {
                let reference = message.message_reference.as_ref().unwrap();
                embed.description(format!(
                    "{} started following {}{} in {}{}",
                    message.author.mention(),
                    match ctx.cache.guild(reference.guild_id.unwrap()) {
                        Some(guild) => format!("[{}]{} ", guild.name, reference.link()),
                        None => "".to_string(),
                    },
                    reference.channel_id.mention(),
                    maybe_link_foreign,
                    message.channel_id.mention()
                ));
            }
            MessageType::ThreadCreated => {
                let reference = message.message_reference.as_ref().unwrap();
                embed.description(format!(
                    "{} created the thread {} in {}{}",
                    message.author.mention(),
                    match message.guild(&ctx) {
                        Some(g) => match g
                            .threads
                            .iter()
                            .find(|thread| thread.id == reference.channel_id)
                        {
                            Some(channel) => channel.mention().to_string(),
                            None => format!("#{}", message.content),
                        },
                        None => format!("#{}", message.content),
                    },
                    maybe_link_foreign,
                    message.channel_id.mention()
                ));
            }
            _ => {}
        };

        // flags
        if flags.contains(MessageFlags::CROSSPOSTED) {
            embed.field("Crossposted", "\u{200B}", true);
        }

        if flags.contains(MessageFlags::SOURCE_MESSAGE_DELETED) {
            embed.field("Crosspost", "Source deleted", true);
        } else if flags.contains(MessageFlags::IS_CROSSPOST) {
            let reference = message.message_reference.as_ref().unwrap();
            match ctx
                .cache
                .guild_field(reference.guild_id.unwrap(), |f| f.name.clone())
            {
                Some(guild) => {
                    embed.field(
                        "Crosspost",
                        format!("From [{}]({})", guild, reference.link()),
                        true,
                    );
                }
                None => {
                    embed.field(
                        "Crosspost",
                        format!("From {}", reference.channel_id.mention()),
                        true,
                    );
                }
            }
        }

        if flags.contains(MessageFlags::HAS_THREAD) && message.kind != MessageType::ThreadCreated {
            embed.field("Thread", ChannelId(message.id.0).mention(), true);
        }

        if flags.contains(MessageFlags::LOADING) {
            embed.description(format!("{} is thinking...", message.author.mention()));
        }

        embed
    }

    async fn preview(
        &self,
        ctx: &BotContext,
        from_user: &UserId,
        from_guild: &Option<GuildId>,
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<(Vec<CreateEmbed>, Vec<Attachment>)> {
        match ctx.cache.guild(&guild) {
            // get guild
            None => Err(Error::new(BotError::NotFound("Server".to_string()))),
            Some(guild) => {
                match guild.member(&ctx, *from_user).await {
                    // get member
                    Err(_) => Err(Error::new(BotError::Generic(s!(
                        "You must be in a server to preview messages from it"
                    )))),
                    Ok(member) => {
                        // get permissions
                        if !member
                            .roles(&ctx)
                            .ok_or(BotError::CacheMissing)?
                            .iter()
                            .any(|role| role.permissions.administrator())
                            && !guild
                                .user_permissions_in(
                                    match guild.channels.get(&channel) {
                                        // get channel
                                        Some(s) => match s {
                                            Channel::Guild(g) => g,
                                            _ => {
                                                return Err(Error::new(BotError::NotFound(s!(
                                                    "Channel"
                                                ))))
                                            }
                                        },
                                        None => {
                                            match guild.threads.iter().find(|c| c.id == channel) {
                                                Some(s) => s,
                                                None => {
                                                    return Err(Error::new(BotError::CacheMissing))
                                                }
                                            }
                                        }
                                    },
                                    &member,
                                )?
                                .read_message_history()
                        {
                            return Err(Error::new(BotError::Generic(s!(
                                "You do not have permission to view this message"
                            ))));
                        }
                        // get message
                        let mut message = channel
                            .message(&ctx, &message)
                            .await
                            .map_err(|_| Error::new(BotError::NotFound("Message".to_string())))?;
                        message.guild_id = Some(guild.id);

                        // inner
                        let embed = Self::derive_embed(
                            ctx,
                            &message,
                            from_guild
                                .and_then(|s| if s == guild.id { None } else { Some(&guild) }),
                        )
                        .await;

                        let mut embeds = vec![embed];

                        for embed in message.embeds {
                            // copy embed data
                            let mut builder = CreateEmbed::default();
                            if let Some(title) = &embed.title {
                                builder.title(title);
                            }
                            match embed.description {
                                Some(description) => builder.description(description),
                                None => builder.description("\u{200B}"),
                            };
                            if let Some(url) = &embed.url {
                                builder.url(url);
                            }
                            if let Some(timestamp) = &embed.timestamp {
                                builder.timestamp(timestamp.clone());
                            }
                            if let Some(image) = embed.image {
                                builder.image(image.url);
                            };
                            if let Some(thumbnail) = embed.thumbnail {
                                builder.thumbnail(thumbnail.url);
                            };
                            if let Some(color) = embed.colour {
                                builder.color(color);
                            };
                            if let Some(footer) = embed.footer {
                                builder.footer(|builder| {
                                    builder.text(footer.text);
                                    if let Some(icon_url) = &footer.icon_url {
                                        builder.icon_url(icon_url);
                                    }
                                    builder
                                });
                            };
                            if let Some(author) = embed.author {
                                builder.author(|builder| {
                                    builder.name(author.name);
                                    if let Some(url) = &author.url {
                                        builder.url(url);
                                    }
                                    if let Some(icon_url) = &author.icon_url {
                                        builder.icon_url(icon_url);
                                    }
                                    builder
                                });
                            };
                            for field in embed.fields {
                                builder.field(field.name, field.value, field.inline);
                            }
                            embeds.push(builder)
                        }

                        Ok((
                            embeds,
                            message
                                .attachments
                                .into_iter()
                                .filter(|s| s.size < 8388246)
                                .collect(),
                        ))
                    }
                }
            }
        }
    }

    pub async fn message(&self, ctx: &BotContext, message: &Message) -> Result<()> {
        // ignore dms
        if matches!(message.guild_id, None) {
            return Ok(());
        }

        // detect if we should scan
        let should_scan = self
            .read_cache(&message.guild_id.unwrap(), |cached| {
                cached.auto_channels.contains(&message.channel_id)
            })
            .await;
        if !should_scan {
            return Ok(());
        }

        for item in self.link_regex.captures_iter(&message.content) {
            match self
                .preview(
                    ctx,
                    &message.author.id,
                    &message.guild_id,
                    GuildId(
                        u64::from_str(item.get(1).ok_or(BotError::Internal(0))?.as_str())
                            .map_err(|_| BotError::Internal(1))?,
                    ),
                    ChannelId(
                        u64::from_str(item.get(2).ok_or(BotError::Internal(2))?.as_str())
                            .map_err(|_| BotError::Internal(3))?,
                    ),
                    MessageId(
                        u64::from_str(item.get(3).ok_or(BotError::Internal(4))?.as_str())
                            .map_err(|_| BotError::Internal(5))?,
                    ),
                )
                .await
            {
                Ok((embeds, attachments)) => {
                    let chunks = embeds.chunks(10);
                    let mut downloaded = Vec::with_capacity(attachments.len());
                    for attachment in attachments {
                        let bytes = attachment.download().await?;
                        downloaded.push(AttachmentType::Bytes {
                            data: Cow::from(bytes),
                            filename: attachment.filename,
                        })
                    }
                    for chunk in chunks {
                        message
                            .channel_id
                            .send_message(&ctx.http, |m| m.set_embeds(chunk.to_vec()))
                            .await?;
                    }
                    if !downloaded.is_empty() {
                        message
                            .channel_id
                            .send_message(&ctx.http, |m| m.files(downloaded))
                            .await?;
                    }
                }
                Err(err) => {
                    if !err.is::<BotError>() {
                        return Err(err);
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn guild_data(&self, ctx: &BotContext, guild: &Guild) -> Result<()> {
        // remove invalid channels
        let entries = self
            .write_cache(&guild.id, |cached| {
                let mut indexes = Vec::new();
                for (index, channel) in cached.auto_channels.iter().enumerate() {
                    if !guild.channels.contains_key(channel) {
                        indexes.push(index);
                    }
                }
                remove_indexes(&mut cached.auto_channels, &indexes)
            })
            .await;

        for entry in entries {
            sqlx::query("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
                .bind(guild.id.0 as i64)
                .bind(entry.0 as i64)
                .execute(&ctx.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn channel_delete(&self, ctx: &BotContext, channel: &GuildChannel) -> Result<()> {
        // remove invalid channel
        let remove = self
            .read_cache(&channel.guild_id, |cached| {
                cached.auto_channels.binary_search(&channel.id).ok()
            })
            .await;
        if let Some(s) = remove {
            self.write_cache(&channel.guild_id, |cached| {
                cached.auto_channels.remove(s);
            })
            .await;
            sqlx::query("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
                .bind(SqlId(channel.guild_id))
                .bind(SqlId(channel.id))
                .execute(&ctx.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn previews_add(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let target = args.get_channel("target")?;
        let guild_id = interaction.guild_id.ok_or(BotError::GuildOnly)?;

        let already_added = self
            .write_cache(&guild_id, |data| {
                if data.auto_channels.contains(&target.id) {
                    true
                } else {
                    data.auto_channels.push(target.id);
                    false
                }
            })
            .await;
        if already_added {
            return Err(Error::new(BotError::Generic(
                "Channel already added".to_string(),
            )));
        }

        sqlx::query("insert into PreviewChannels (guild_id, channel_id) values ($1, $2) on conflict do nothing")
            .bind(SqlId(guild_id))
            .bind(SqlId(target.id))
            .execute(&ctx.pool)
            .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx, interaction)
            .await
    }

    pub async fn previews_remove(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let target = args.get_channel("target")?;
        let guild_id = interaction.guild_id.ok_or(BotError::GuildOnly)?;

        let not_in_previews = self
            .write_cache(&guild_id, |data| {
                match data.auto_channels.binary_search(&target.id) {
                    Ok(s) => {
                        data.auto_channels.remove(s);
                        false
                    }
                    Err(_) => true,
                }
            })
            .await;
        if not_in_previews {
            return Err(Error::new(BotError::Generic(
                "Channel not in previews".to_string(),
            )));
        }

        sqlx::query("delete from PreviewChannels where guild_id = $1 and channel_id = $2")
            .bind(SqlId(guild_id))
            .bind(SqlId(target.id))
            .execute(&ctx.pool)
            .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx, interaction)
            .await
    }

    pub async fn previews_list(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let mut items = vec!["**Channels**".to_string()];

        self.read_cache(&interaction.guild_id.ok_or(BotError::GuildOnly)?, |data| {
            for channel in &data.auto_channels {
                items.push(channel.mention().to_string())
            }
        })
        .await;

        if items.len() == 1 {
            return FollowupBuilder::new()
                .description("No channels")
                .build_command_followup(&ctx.http, interaction)
                .await;
        }

        FollowupBuilder::new()
            .description(items.join("\n"))
            .build_command_followup(&ctx, interaction)
            .await
    }

    pub async fn previews_view(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let target = args.get_string("target")?;

        let captures = self
            .link_regex
            .captures(&target)
            .ok_or_else(|| BotError::Generic("Malformed link".to_string()))?;

        let (embeds, attachments) = self
            .preview(
                ctx,
                &interaction.user.id,
                &interaction.guild_id,
                GuildId(
                    u64::from_str(captures.get(1).ok_or(BotError::Internal(6))?.as_str())
                        .map_err(|_| BotError::Internal(7))?,
                ),
                ChannelId(
                    u64::from_str(captures.get(2).ok_or(BotError::Internal(8))?.as_str())
                        .map_err(|_| BotError::Internal(9))?,
                ),
                MessageId(
                    u64::from_str(captures.get(3).ok_or(BotError::Internal(10))?.as_str())
                        .map_err(|_| BotError::Internal(11))?,
                ),
            )
            .await?;

        let chunks = embeds.chunks(10);
        let mut downloaded = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let bytes = attachment.download().await?;
            downloaded.push(AttachmentType::Bytes {
                data: Cow::from(bytes),
                filename: attachment.filename,
            })
        }
        for chunk in chunks {
            interaction
                .create_followup_message(&ctx.http, |m| m.set_embeds(chunk.to_vec()))
                .await?;
        }
        if !downloaded.is_empty() {
            interaction
                .create_followup_message(&ctx.http, |m| m.files(downloaded))
                .await?;
        }

        Ok(())
    }

    pub async fn update_archive(
        &self,
        pool: &PgPool,
        from: Option<ChannelId>,
        guild_id: GuildId,
    ) -> Result<()> {
        match from {
            // set
            Some(target) => {
                self.write_cache(&guild_id, |data| {
                    data.archive_channel = Some(target);
                })
                .await;

                sqlx::query("insert into ArchiveChannel (guild_id, channel_id) values ($1, $2) on conflict on constraint archive_idx do update set channel_id = $2")
                    .bind(&SqlId(guild_id))
                    .bind(&SqlId(target))
                    .execute(pool)
                    .await?;
            }
            // unset
            None => {
                self.write_cache(&guild_id, |data| {
                    data.archive_channel = None;
                })
                .await;

                sqlx::query("delete from ArchiveChannel where guild_id = $1")
                    .bind(&SqlId(guild_id))
                    .execute(pool)
                    .await?;
            }
        };
        Ok(())
    }

    pub async fn previews_archive(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let guild_id = interaction.guild_id.unwrap();
        self.update_archive(
            &ctx.pool,
            args.get_channel("target").ok().map(|c| c.id),
            guild_id,
        )
        .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx, interaction)
            .await
    }

    pub async fn previews_archive_context(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        message: &Message,
    ) -> Result<()> {
        FollowupBuilder::new()
            .description("Running...")
            .ephemeral()
            .build_command_response(ctx, interaction)
            .await?;

        let guild_id = interaction.guild_id.unwrap();
        let archive_channel = self
            .read_cache(&guild_id, |data| data.archive_channel)
            .await
            .ok_or_else(|| BotError::Generic("Archive channel not set".to_string()))?;

        let (source_embeds, attachments) = self
            .preview(
                ctx,
                &interaction.user.id,
                &Some(guild_id),
                guild_id,
                interaction.channel_id,
                message.id,
            )
            .await?;

        // add footer to first embed
        let mut embeds = Vec::new();
        let mut iter = source_embeds.into_iter();
        let mut embed = iter.next().unwrap();
        embed.footer(|f| {
            f.text(format!(
                "Requested by {}#{}",
                interaction.user.name, interaction.user.discriminator
            ))
            .icon_url(match &interaction.member {
                Some(member) => member.face(),
                None => interaction.user.face(),
            })
        });
        embeds.push(embed);
        embeds.extend(iter);

        let chunks = embeds.chunks(10);
        let mut downloaded = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let bytes = attachment.download().await?;
            downloaded.push(AttachmentType::Bytes {
                data: Cow::from(bytes),
                filename: attachment.filename,
            })
        }
        for chunk in chunks {
            archive_channel
                .send_message(ctx, |m| m.set_embeds(chunk.to_vec()))
                .await?;
        }
        if !downloaded.is_empty() {
            archive_channel
                .send_message(ctx, |m| m.files(downloaded))
                .await?;
        }

        FollowupBuilder::new()
            .description("Success")
            .build_command_edit(ctx, interaction)
            .await
    }
}
