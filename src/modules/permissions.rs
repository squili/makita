// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::custom_ids::{build_custom_id, CustomIdType};
use crate::decode::SlashMap;
use crate::impl_cache_functions;
use crate::prelude::*;
use crate::tasks::TaskMessage;
use crate::utils::{defer_command, defer_component, BotContext, FollowupBuilder, SqlId};
use anyhow::{Error, Result};
use futures::FutureExt;
use serenity::builder::CreateSelectMenuOptions;
use serenity::model::guild::Role;
use serenity::model::id::{GuildId, RoleId, UserId};
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::prelude::interaction::message_component::MessageComponentInteraction;
use serenity::model::Permissions as DiscordPermissions;
use serenity::prelude::Mentionable;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use tokio::sync::{broadcast, RwLock};

macro_rules! impl_permission_type {
    ($($enum: ident, $value: expr, $display: expr, $desc: expr),+) => {
        #[allow(dead_code)]
        #[derive(Hash, Eq, PartialEq, Copy, Clone, Debug, sqlx::Type)]
        #[sqlx(type_name = "PermissionType")]
        pub enum PermissionType {
            $($enum),+
        }

        #[allow(dead_code)]
        impl PermissionType {
            pub fn as_value(&self) -> &'static str {
                match self {
                    $(PermissionType::$enum => $value),+
                }
            }

            pub fn as_display(&self) -> &'static str {
                match self {
                    $(PermissionType::$enum => $display),+
                }
            }

            pub fn from_string(from: &str) -> Result<Self> {
                match from {
                    $($value => Ok(PermissionType::$enum)),+,
                    _ => Err(Error::new(BotError::InvalidRequest(format!("Invalid permission name {}", from))))
                }
            }

            pub fn build_select_menu_options(builder: &mut CreateSelectMenuOptions) -> &mut CreateSelectMenuOptions {
                $(builder.create_option(|o| o.label($display).value($value).description($desc)));+;
                builder
            }
        }
    }
}

impl_permission_type!(
    Administrator,
    "Administrator",
    "Administrator",
    "Access to all permissions",
    ManagePermissions,
    "ManagePermissions",
    "Manage Permissions",
    "Manage bot permissions",
    ManagePreviews,
    "ManagePreviews",
    "Manage Previews",
    "Manage preview configuration",
    CreateArchive,
    "CreateArchive",
    "Create Archive",
    "Create entries in the archive channel",
    Timeout,
    "Timeout",
    "Timeout",
    "Timeout users"
);

pub struct GuildPermissionData {
    discord: DiscordPermissions,
    roles: Vec<RoleId>,
    users: Vec<UserId>,
}

impl GuildPermissionData {
    fn new(permissions: &DiscordPermissions) -> Self {
        Self {
            discord: *permissions,
            roles: Vec::new(),
            users: Vec::new(),
        }
    }

    pub fn default(ty: &PermissionType) -> Self {
        match ty {
            PermissionType::Administrator => Self::new(&DiscordPermissions::ADMINISTRATOR),
            PermissionType::ManagePermissions => Self::new(&DiscordPermissions::ADMINISTRATOR),
            PermissionType::ManagePreviews => Self::new(&DiscordPermissions::MANAGE_GUILD),
            PermissionType::CreateArchive => Self::new(&DiscordPermissions::MANAGE_MESSAGES),
            PermissionType::Timeout => Self::new(&DiscordPermissions::MODERATE_MEMBERS),
        }
    }
}

#[derive(Default)]
pub struct GuildPermissionEntry {
    data: HashMap<PermissionType, GuildPermissionData>,
    guild_id: GuildId,
}

impl GuildPermissionEntry {
    fn new(guild_id: &GuildId) -> Self {
        let mut entry = Self {
            data: Default::default(),
            guild_id: *guild_id,
        };
        entry.data.insert(
            PermissionType::Administrator,
            GuildPermissionData::default(&PermissionType::Administrator),
        );
        entry.data.insert(
            PermissionType::ManagePermissions,
            GuildPermissionData::default(&PermissionType::ManagePermissions),
        );
        entry.data.insert(
            PermissionType::ManagePreviews,
            GuildPermissionData::default(&PermissionType::ManagePreviews),
        );
        entry.data.insert(
            PermissionType::CreateArchive,
            GuildPermissionData::default(&PermissionType::CreateArchive),
        );
        entry.data.insert(
            PermissionType::Timeout,
            GuildPermissionData::default(&PermissionType::Timeout),
        );
        entry
    }

    pub fn get(&self, ty: &PermissionType) -> &GuildPermissionData {
        self.data.get(ty).unwrap()
    }

    // Safety: Must update database yourself
    unsafe fn get_mut(&mut self, ty: &PermissionType) -> &mut GuildPermissionData {
        self.data.get_mut(ty).unwrap()
    }

    pub async fn set<F>(&mut self, ty: &PermissionType, pool: &PgPool, mut func: F) -> Result<()>
    where
        F: FnMut(&mut GuildPermissionData),
    {
        let data = self.data.get_mut(ty).unwrap();
        func(data);
        sqlx::query("insert into Permissions (type, guild_id, overwrites, roles, users) values ($1, $2, $3, $4, $5)\
                         on conflict on constraint permissions_idx do update set overwrites = $3, roles = $4, users = $5")
            .bind(&ty)
            .bind(&SqlId(self.guild_id))
            .bind(&SqlId(data.discord.bits()))
            .bind(data.roles.iter().map(|s| s.0 as i64).collect::<Vec<i64>>())
            .bind(data.users.iter().map(|s| s.0 as i64).collect::<Vec<i64>>())
            .execute(pool)
            .await?;
        Ok(())
    }
}

pub struct PermissionsModule {
    guild_cache: RwLock<HashMap<GuildId, GuildPermissionEntry>>,
    pub sudo_users: RwLock<HashSet<UserId>>,
    pool: PgPool,
}

impl PermissionsModule {
    pub fn new(pool: PgPool) -> Self {
        Self {
            guild_cache: Default::default(),
            sudo_users: Default::default(),
            pool,
        }
    }

    impl_cache_functions!(
        guild_read,
        guild_write,
        write_guild_async,
        GuildId,
        GuildPermissionEntry,
        guild_cache,
        GuildPermissionEntry::new
    );

    pub async fn initialize(
        self: Arc<Self>,
        mut task_rx: broadcast::Receiver<TaskMessage>,
    ) -> Result<()> {
        // guild cache data
        let rows = sqlx::query("select guild_id, type, overwrites, roles, users from Permissions")
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            self.guild_write(&row.get::<SqlId<GuildId>, _>("guild_id").0, |entry| {
                let data = unsafe { entry.get_mut(&row.get::<PermissionType, _>("type")) };
                data.discord =
                    DiscordPermissions::from_bits(row.get::<SqlId<u64>, _>("overwrites").0)
                        .unwrap();
                data.roles = row
                    .get::<Vec<i64>, _>("roles")
                    .iter()
                    .map(|s| RoleId(*s as u64))
                    .collect::<Vec<RoleId>>();
                data.users = row
                    .get::<Vec<i64>, _>("users")
                    .iter()
                    .map(|s| UserId(*s as u64))
                    .collect::<Vec<UserId>>();
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
                        self.guild_cache.write().await.remove(&g);
                    }
                }
            }
        });

        Ok(())
    }

    fn entry_perms_check(
        ty: &PermissionType,
        entry: &GuildPermissionEntry,
        highest: &DiscordPermissions,
        user: &UserId,
        roles: &[Role],
    ) -> bool {
        let data = entry.get(ty);
        (data.discord.bits() > 0 && highest.contains(data.discord))
            || data.users.contains(user)
            || roles.iter().any(|item| data.roles.contains(&item.id))
    }

    #[allow(clippy::needless_lifetimes)] // lifetimes not actually needless
    pub async fn check<'a>(
        &self,
        ty: &'a PermissionType,
        guild: &GuildId,
        owner: &UserId,
        user: &UserId,
        roles: &[Role],
    ) -> Option<&'a PermissionType> {
        if self.sudo_users.read().await.contains(owner) {
            return None;
        }

        if owner == user {
            return None;
        }
        let mut highest_permissions = DiscordPermissions::empty();
        for role in roles {
            highest_permissions.insert(role.permissions);
        }
        self.guild_read(guild, |entry| {
            if Self::entry_perms_check(
                &PermissionType::Administrator,
                entry,
                &highest_permissions,
                user,
                roles,
            ) || Self::entry_perms_check(ty, entry, &highest_permissions, user, roles)
            {
                None
            } else {
                Some(ty)
            }
        })
        .await
    }

    pub async fn permissions_list(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        interaction
            .create_followup_message(&ctx.http, |a| {
                a.components(|b| {
                    b.create_action_row(|c| {
                        c.create_select_menu(|d| {
                            d.custom_id(build_custom_id(&CustomIdType::ListPermissions, &None))
                                .options(|e| {
                                    PermissionType::build_select_menu_options(e);
                                    e
                                })
                        })
                    })
                })
                .embed(|b| b.description("Select a permission"))
            })
            .await?;

        Ok(())
    }

    pub async fn permissions_list_component(
        &self,
        ctx: &BotContext,
        interaction: &MessageComponentInteraction,
    ) -> Result<()> {
        defer_component(&ctx, interaction).await?;
        let ty =
            PermissionType::from_string(interaction.data.values.get(0).ok_or_else(|| {
                BotError::InvalidRequest("Missing component values".to_string())
            })?)?;

        let (permissions, roles, users) = self
            .guild_read(&interaction.guild_id.ok_or(BotError::GuildOnly)?, |entry| {
                let data = entry.get(&ty);
                (
                    data.discord.to_string(),
                    data.roles
                        .iter()
                        .map(|x| x.mention().to_string())
                        .collect::<Vec<String>>()
                        .join("\n"),
                    data.users
                        .iter()
                        .map(|x| x.mention().to_string())
                        .collect::<Vec<String>>()
                        .join("\n"),
                )
            })
            .await;

        interaction
            .edit_original_interaction_response(&ctx.http, |a| {
                a.embed(|b| {
                    if !roles.is_empty() {
                        b.field("Roles", roles, false);
                    }
                    if !users.is_empty() {
                        b.field("Users", users, false);
                    }
                    b.field("Default", permissions, false)
                        .title(ty.as_display())
                })
                .components(|b| {
                    b.create_action_row(|c| {
                        c.create_select_menu(|d| {
                            d.custom_id(build_custom_id(&CustomIdType::ListPermissions, &None))
                                .options(|e| {
                                    PermissionType::build_select_menu_options(e);
                                    e
                                })
                                .placeholder(ty.as_display())
                        })
                    })
                })
            })
            .await?;

        Ok(())
    }

    pub async fn permissions_set(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let ty = PermissionType::from_string(&args.get_string("permission")?)?;
        let permissions = DiscordPermissions::from_bits(args.get_integer("bits")? as u64)
            .ok_or_else(|| BotError::InvalidRequest("Invalid permissions bits".to_string()))?;

        self.write_guild_async(
            &interaction.guild_id.ok_or(BotError::GuildOnly)?,
            |entry: &mut GuildPermissionEntry, _| {
                async move {
                    entry
                        .set(&ty, &self.pool, |data| {
                            data.discord = permissions;
                        })
                        .await
                }
                .boxed()
            },
        )
        .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx.http, interaction)
            .await
    }

    pub async fn permissions_add(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let ty = PermissionType::from_string(&args.get_string("permission")?)?;
        let user = args.get_user("user").map(|s| s.get_user().id).ok();
        let role_object = args.get_role("role").ok();
        let role_id = role_object.as_ref().map(|s| s.id);
        let guild_id = interaction.guild_id.ok_or(BotError::GuildOnly)?;

        if let Some(role) = role_object {
            if role.guild_id != guild_id {
                return Err(Error::new(BotError::WrongGuild));
            }
        }

        if let (None, None) = (user, role_id) {
            return Err(Error::new(BotError::Generic(
                "Must specify either `user` or `role`".into(),
            )));
        }

        self.write_guild_async(&guild_id, |entry: &mut GuildPermissionEntry, _| {
            async move {
                entry
                    .set(&ty, &self.pool, |data| {
                        if let Some(s) = user {
                            data.users.push(s);
                        }
                        if let Some(s) = role_id {
                            data.roles.push(s);
                        }
                    })
                    .await
            }
            .boxed()
        })
        .await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx.http, interaction)
            .await
    }

    pub async fn permissions_remove(
        &self,
        ctx: &BotContext,
        interaction: &ApplicationCommandInteraction,
        args: SlashMap,
    ) -> Result<()> {
        defer_command(&ctx, interaction).await?;
        let ty = PermissionType::from_string(&args.get_string("permission")?)?;
        let user = args.get_user("user").map(|s| s.get_user().id).ok();
        let role = args.get_role("role").map(|s| s.id).ok();

        if let (None, None) = (user, role) {
            return Err(Error::new(BotError::Generic(
                "Must specify either `user` or `role`".into(),
            )));
        }

        let mut user_found = true;
        let mut role_found = true;
        self.write_guild_async(
            &interaction.guild_id.ok_or(BotError::GuildOnly)?,
            |entry: &mut GuildPermissionEntry, _| {
                async move {
                    entry
                        .set(&ty, &self.pool, |data| {
                            if let Some(s) = user {
                                match data.users.binary_search(&s) {
                                    Ok(s) => {
                                        data.users.remove(s);
                                    }
                                    Err(_) => {
                                        user_found = false;
                                    }
                                }
                            }
                            if let Some(s) = role {
                                match data.roles.binary_search(&s) {
                                    Ok(s) => {
                                        data.roles.remove(s);
                                    }
                                    Err(_) => {
                                        role_found = false;
                                    }
                                }
                            }
                        })
                        .await
                }
                .boxed()
            },
        )
        .await?;

        if !user_found {
            return Err(Error::new(BotError::Generic(format!(
                "User {} not added",
                user.unwrap().mention()
            ))));
        }

        if !role_found {
            return Err(Error::new(BotError::Generic(format!(
                "Role {} not added",
                role.unwrap().mention()
            ))));
        }

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx.http, interaction)
            .await
    }
}
