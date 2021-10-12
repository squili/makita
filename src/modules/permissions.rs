// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use anyhow::{Result, Error};
use serenity::model::Permissions as DiscordPermissions;
use serenity::model::id::{RoleId, UserId, GuildId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, RwLock};
use sqlx::{PgPool, Row};
use crate::utils::{SqlId, FollowupBuilder, BotContext};
use crate::macros::impl_cache_functions;
use crate::error::BotError;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::builder::CreateSelectMenuOptions;
use crate::custom_ids::{build_custom_id, CustomIdType};
use serenity::model::interactions::message_component::MessageComponentInteraction;
use serenity::model::misc::Mentionable;
use crate::decode::SlashMap;
use futures::FutureExt;
use crate::tasks::TaskMessage;

macro_rules! impl_permission_type {
    ($($enum: ident, $value: expr, $display: expr, $desc: expr),+) => {
        #[derive(Hash, Eq, PartialEq, Copy, Clone, Debug, sqlx::Type)]
        #[sqlx(type_name = "PermissionType")]
        pub enum PermissionType {
            $($enum),+
        }

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
    Administrator, "Administrator", "Administrator", "Access to all permissions",
    ManagePermissions, "ManagePermissions", "Manage Permissions", "Manage bot permissions",
    ManagePreviews, "ManagePreviews", "Manage Previews", "Manage preview configuration",
    CreateArchive, "CreateArchive", "Create Archive", "Create entries in the archive channel"
);

pub struct PermissionData {
    discord: DiscordPermissions,
    roles: Vec<RoleId>,
    users: Vec<UserId>,
}

impl PermissionData {
    fn new(permissions: &DiscordPermissions) -> Self {
        Self { discord: *permissions, roles: Vec::new(), users: Vec::new() }
    }

    pub fn default(ty: &PermissionType) -> Self {
        match ty {
            PermissionType::Administrator => Self::new(&DiscordPermissions::ADMINISTRATOR),
            PermissionType::ManagePermissions => Self::new(&DiscordPermissions::ADMINISTRATOR),
            PermissionType::ManagePreviews => Self::new(&DiscordPermissions::MANAGE_GUILD),
            PermissionType::CreateArchive => Self::new(&DiscordPermissions::MANAGE_MESSAGES),
        }
    }
}

#[derive(Default)]
pub struct PermissionEntry {
    data: HashMap<PermissionType, PermissionData>,
    guild_id: GuildId,
}

impl PermissionEntry {
    fn new(guild_id: &GuildId) -> Self {
        let mut entry = Self { data: Default::default(), guild_id: *guild_id };
        entry.data.insert(PermissionType::Administrator, PermissionData::default(&PermissionType::Administrator));
        entry.data.insert(PermissionType::ManagePermissions, PermissionData::default(&PermissionType::ManagePermissions));
        entry.data.insert(PermissionType::ManagePreviews, PermissionData::default(&PermissionType::ManagePreviews));
        entry
    }

    pub fn get(&self, ty: &PermissionType) -> &PermissionData {
        self.data.get(ty).unwrap()
    }

    // Safety: Must update database yourself
    unsafe fn get_mut(&mut self, ty: &PermissionType) -> &mut PermissionData {
        self.data.get_mut(ty).unwrap()
    }

    pub async fn set<F>(&mut self, ty: &PermissionType, pool: &PgPool, mut func: F) -> Result<()>
    where F: FnMut(&mut PermissionData) {
        let data = self.data.get_mut(ty).unwrap();
        func(data);
        sqlx::query("insert into Permissions (type, guild_id, overwrites, roles, users) values ($1, $2, $3, $4, $5)\
                         on conflict on constraint permissions_idx do update set overwrites = $3, roles = $4, users = $5")
            .bind(&ty)
            .bind(&SqlId(self.guild_id))
            .bind(&SqlId(data.discord.bits))
            .bind(data.roles.iter().map(|s| s.0 as i64).collect::<Vec<i64>>())
            .bind(data.users.iter().map(|s| s.0 as i64).collect::<Vec<i64>>())
            .execute(pool)
            .await?;
        Ok(())
    }
}

pub struct PermissionsModule {
    cache: RwLock<HashMap<GuildId, PermissionEntry>>,
    pool: PgPool,
    sudo_enabled: AtomicBool,
    owner_id: UserId,
}

impl PermissionsModule {
    pub fn new(owner_id: UserId, pool: PgPool) -> Self {
        Self {
            cache: Default::default(),
            sudo_enabled: AtomicBool::new(false),
            pool,
            owner_id,
        }
    }

    impl_cache_functions!(guild_read, guild_write, write_guild_async, GuildId, PermissionEntry, cache, PermissionEntry::new);

    pub async fn initialize(instance: Arc<Self>, mut task_rx: broadcast::Receiver<TaskMessage>) -> Result<()> {
        let rows = sqlx::query("select guild_id, type, overwrites, roles, users from Permissions")
            .fetch_all(&instance.pool)
            .await?;

        for row in rows {
            instance.guild_write(&row.get::<SqlId<GuildId>, &str>("guild_id").0, |entry| {
                let data = unsafe { entry.get_mut(&row.get::<PermissionType, &str>("type")) };
                data.discord = DiscordPermissions { bits: row.get::<SqlId<u64>, &str>("overwrites").0 };
                data.roles = row.get::<Vec<i64>, &str>("roles").iter().map(|s| RoleId(*s as u64)).collect::<Vec<RoleId>>();
                data.users = row.get::<Vec<i64>, &str>("users").iter().map(|s| UserId(*s as u64)).collect::<Vec<UserId>>();
            }).await;
        }

        // task event handling
        tokio::spawn(async move {
            loop {
                let msg = task_rx.recv().await.unwrap();
                match msg {
                    TaskMessage::Kill => break,
                    TaskMessage::DestroyGuild(g) => {
                        instance.cache.write().await.remove(&g);
                    }
                }
            }
        });

        Ok(())
    }

    fn entry_perms_check(ty: &PermissionType, entry: &PermissionEntry, highest: &DiscordPermissions, user: &UserId, roles: &[RoleId]) -> bool {
        let data = entry.get(ty);
        (data.discord.bits > 0 && highest.contains(data.discord)) || data.users.contains(user)
            || roles.iter().any(|item| data.roles.contains(item))
    }

    #[allow(clippy::needless_lifetimes)] // lifetimes not actually needless
    pub async fn check<'a>(&self, ctx: &BotContext, ty: &'a PermissionType, guild: &GuildId, user: &UserId, roles: &[RoleId]) -> Result<Option<&'a PermissionType>> {
        if self.sudo_enabled.load(Ordering::Relaxed) && user == &self.owner_id {
              return Ok(None)
        }

        let guild_data = ctx.cache.guild(*guild).await.ok_or(BotError::CacheMissing)?;
        if &guild_data.owner_id == user {
            return Ok(None)
        }
        let mut highest_permissions = DiscordPermissions::empty();
        for role in roles {
            let role = guild_data.roles.get(role).ok_or(BotError::CacheMissing)?;
            highest_permissions.insert(role.permissions);
        }
        if highest_permissions.administrator() {
            return Ok(None);
        }
        self.guild_read(guild, |entry| {
            if Self::entry_perms_check(&PermissionType::Administrator, entry, &highest_permissions, user, roles) ||
                Self::entry_perms_check(ty, entry, &highest_permissions, user, roles) {
                Ok(None)
            } else {
                Ok(Some(ty))
            }
        }).await
    }

    pub async fn makita_sudo(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.defer(&ctx).await?;
        let sudo_enabled = self.sudo_enabled.load(Ordering::Relaxed);
        self.sudo_enabled.store(!sudo_enabled, Ordering::Relaxed);

        FollowupBuilder::new()
            .description(format!("Sudo mode set to {}", if !sudo_enabled { "enabled" } else { "disabled" }))
            .build_command_followup(&ctx.http, interaction)
            .await
    }

    pub async fn permissions_list(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.defer(&ctx).await?;
        interaction.create_followup_message(&ctx.http, |a|
            a.components(|b|
                b.create_action_row(|c|
                    c.create_select_menu(|d|
                        d
                            .custom_id(build_custom_id(&CustomIdType::ListPermissions, &None))
                            .options(|e| {
                                PermissionType::build_select_menu_options(e);
                                e
                            })
                    )
                )
            ).create_embed(|b| b.description("Select a permission"))
        ).await?;

        Ok(())
    }

    pub async fn permissions_list_component(&self, ctx: &BotContext, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction.defer(&ctx).await?;
        let ty = PermissionType::from_string(
            interaction.data.values.get(0).ok_or_else(|| BotError::InvalidRequest("Missing component values".to_string()))?)?;

        let (permissions, roles, users) = self.guild_read(&interaction.guild_id.ok_or(BotError::GuildOnly)?, |entry| {
            let data = entry.get(&ty);
            (data.discord.to_string(),
             data.roles.iter().map(|x| x.mention().to_string()).collect::<Vec<String>>().join("\n"),
             data.users.iter().map(|x| x.mention().to_string()).collect::<Vec<String>>().join("\n"))
        }).await;

        interaction.create_followup_message(&ctx.http, |a|
            a.create_embed(|b| {
                if !roles.is_empty() {
                    b.field("Roles", roles, false);
                }
                if !users.is_empty() {
                    b.field("Users", users, false);
                }
                b.field("Default", permissions, false).title(ty.as_display())
            }).components(|b|
                b.create_action_row(|c|
                    c.create_select_menu(|d|
                        d
                            .custom_id(build_custom_id(&CustomIdType::ListPermissions, &None))
                            .options(|e| {
                                PermissionType::build_select_menu_options(e);
                                e
                            })
                            .placeholder(ty.as_display())
                    )
                )
            )
        ).await?;

        Ok(())
    }

    pub async fn permissions_set(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction, args: SlashMap) -> Result<()> {
        interaction.defer(&ctx).await?;
        let ty = PermissionType::from_string(&args.get_string("permission")?)?;
        let permissions = DiscordPermissions::from_bits(args.get_integer("bits")? as u64)
            .ok_or_else(|| BotError::InvalidRequest("Invalid permissions bits".to_string()))?;

        self.write_guild_async(&interaction.guild_id.ok_or(BotError::GuildOnly)?, |entry: &mut PermissionEntry, _| async move {
            entry.set(&ty, &self.pool, |data| {
                data.discord = permissions;
            }).await
        }.boxed()).await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx.http, interaction)
            .await
    }

    pub async fn permissions_add(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction, args: SlashMap) -> Result<()> {
        interaction.defer(&ctx).await?;
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
            return Err(Error::new(BotError::Generic("Must specify either `user` or `role`".into())))
        }

        self.write_guild_async(&guild_id, |entry: &mut PermissionEntry, _| async move {
            entry.set(&ty, &self.pool, |data| {
                if let Some(s) = user {
                    data.users.push(s);
                }
                if let Some(s) = role_id {
                    data.roles.push(s);
                }
            }).await
        }.boxed()).await?;

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx.http, interaction)
            .await
    }

    pub async fn permissions_remove(&self, ctx: &BotContext, interaction: &ApplicationCommandInteraction, args: SlashMap) -> Result<()> {
        interaction.defer(&ctx).await?;
        let ty = PermissionType::from_string(&args.get_string("permission")?)?;
        let user = args.get_user("user").map(|s| s.get_user().id).ok();
        let role = args.get_role("role").map(|s| s.id).ok();

        if let (None, None) = (user, role) {
            return Err(Error::new(BotError::Generic("Must specify either `user` or `role`".into())))
        }

        let mut user_found = true;
        let mut role_found = true;
        self.write_guild_async(&interaction.guild_id.ok_or(BotError::GuildOnly)?, |entry: &mut PermissionEntry, _| async move {
            entry.set(&ty, &self.pool, |data| {
                if let Some(s) = user {
                    match data.users.binary_search(&s) {
                        Ok(s) => { data.users.remove(s); }
                        Err(_) => { user_found = false; }
                    }
                }
                if let Some(s) = role {
                    match data.roles.binary_search(&s) {
                        Ok(s) => { data.roles.remove(s); }
                        Err(_) => { role_found = false; }
                    }
                }
            }).await
        }.boxed()).await?;

        if !user_found {
            return Err(Error::new(BotError::Generic(format!("User {} not added", user.unwrap().mention()))));
        }

        if !role_found {
            return Err(Error::new(BotError::Generic(format!("Role {} not added", role.unwrap().mention()))));
        }

        FollowupBuilder::new()
            .description("Success")
            .build_command_followup(&ctx.http, interaction)
            .await
    }
}
