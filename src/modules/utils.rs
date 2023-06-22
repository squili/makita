// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::decode::SlashMap;
use crate::prelude::*;
use crate::utils::{highest_role, link_guild, parse_duration, FollowupBuilder};
use anyhow::Result;
use serenity::builder::CreateEmbed;
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::Timestamp;
use serenity::prelude::Mentionable;

pub struct UtilsModule {}

impl UtilsModule {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn timeout_command(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        let reason = args.get_string("reason")?;
        let shame = args.get_boolean("shame").unwrap_or(true);
        let dm = args.get_boolean("dm").unwrap_or(true);
        let anon = args.get_boolean("anon").unwrap_or(false);
        let duration = match parse_duration(&args.get_string("duration")?) {
            Some(s) => s,
            None => {
                return FollowupBuilder::new()
                    .description("Duration is malformed")
                    .set_ephemeral(anon)
                    .build_command_response(&ctx, interaction)
                    .await;
            }
        };
        let until = Timestamp::from_unix_timestamp(
            (SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + duration.as_secs()) as i64,
        )?;

        let target = interaction
            .guild_id
            .unwrap()
            .member(&ctx, args.get_user("target")?.id())
            .await?;

        if shame && !anon {
            interaction.defer(ctx).await?;
        }

        // TODO: break out rest of this functionality into a generic function that can be used by other modules

        // check if we can actually do what they are asking from us
        if duration > Duration::from_secs(28 * 60 * 60 * 24) {
            return FollowupBuilder::new()
                .description("Duration is too long")
                .set_ephemeral(anon)
                .build_command_somehow(&ctx, interaction, shame && !anon)
                .await;
        }

        let guild_owner = ctx
            .cache
            .guild_field(&interaction.guild_id.unwrap(), |guild| guild.owner_id)
            .ok_or(BotError::CacheMissing)?;
        if target.user.id == guild_owner {
            return FollowupBuilder::new()
                .description("Can't time out owner")
                .set_ephemeral(anon)
                .build_command_somehow(&ctx, interaction, shame && !anon)
                .await;
        }

        let roles = ctx
            .cache
            .guild_roles(interaction.guild_id.unwrap())
            .ok_or(BotError::CacheMissing)?;
        let our_position = ctx
            .cache
            .member_field(
                &interaction.guild_id.unwrap(),
                &ctx.cache.current_user_id(),
                |member| highest_role(&roles, &member.roles),
            )
            .ok_or(BotError::CacheMissing)?;
        let target_position = highest_role(&roles, &target.roles);

        if our_position <= target_position {
            return FollowupBuilder::new()
                .description(format!("{} has a role above me", target.mention()))
                .set_ephemeral(anon)
                .build_command_somehow(&ctx, interaction, shame && !anon)
                .await;
        }

        // it's actually kinda nice that, with discord's timeout feature, this is all i need to write out to do the actual changes
        target
            .edit(&ctx, |e| e.disable_communication_until_datetime(until))
            .await?;

        let until = format!(
            "<t:{0}:{1}> <t:{0}:R>",
            until.unix_timestamp(),
            if duration <= Duration::from_secs(60 * 60) {
                "t"
            } else {
                "f"
            }
        );

        if shame {
            let mut create_embed = CreateEmbed::default();
            create_embed
                .description(format!("{} was muted", target.mention()))
                .field("Reason", &reason, false)
                .field("Until", &until, false);

            if anon {
                interaction
                    .channel_id
                    .send_message(&ctx, |m| m.set_embed(create_embed))
                    .await?;
            } else {
                create_embed.field("By", interaction.user.mention(), false);
                interaction
                    .create_followup_message(&ctx, |m| m.add_embed(create_embed))
                    .await?;
            }
        }

        if dm && !target.user.bot {
            let guild = ctx
                .cache
                .guild(&interaction.guild_id.unwrap())
                .ok_or(BotError::CacheMissing)?;
            // TODO: properly handle reporting errors, marking if the user has blocked the bot (make sure to handle anon)
            target
                .user
                .dm(&ctx, |m| {
                    m.embed(|e| {
                        e.description(format!(
                            "You were muted in {} until {} for {}",
                            link_guild(&guild, &interaction.channel_id),
                            until,
                            reason
                        ))
                    })
                })
                .await
                .ok();
        }

        if !shame {
            FollowupBuilder::new()
                .description("Success")
                .set_ephemeral(anon)
                .build_command_response(&ctx, interaction)
                .await?;
        }

        Ok(())
    }

    pub async fn untimeout_command(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        interaction.defer(&ctx).await?;

        let target = interaction
            .guild_id
            .unwrap()
            .member(&ctx, args.get_user("target")?.id())
            .await?;

        target.edit(&ctx, |e| e.enable_communication()).await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx, interaction)
            .await
    }
}
