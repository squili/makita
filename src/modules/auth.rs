use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use jsonwebtokens::{Algorithm, Verifier};
use serenity::model::id::{GuildId, UserId};
use tokio::sync::RwLock;
use serenity::model::guild::{Guild, GuildInfo};
use serenity::model::user::CurrentUser;
use sqlx::{PgPool, Row};
use std::mem::size_of;
use std::ops::Add;
use std::time::{SystemTime, UNIX_EPOCH};
use sqlx::postgres::PgRow;
use crate::utils::{naive_now, SqlId};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use anyhow::Result;
use std::result::Result as StdResult;
use std::str::FromStr;
use crate::api::utils::{ApiContext, api_map, ApiError};
use crate::modules::PermissionType;
use std::time::Duration as StdDuration;

pub struct BotUser {
    pub id: UserId,
    pub guilds: RwLock<HashMap<GuildId, HashMap<PermissionType, (bool, SystemTime)>>>,
    pub sessions: RwLock<Vec<Weak<Session>>>,
}

pub struct Session {
    pub id: i64,
    pub user: Arc<BotUser>,
    pub expire_at: NaiveDateTime, // TODO: automatically revoke expired sessions
}

pub struct AuthModule {
    users: RwLock<HashMap<UserId, Arc<BotUser>>>,
    sessions: RwLock<HashMap<i64, Arc<Session>>>,
    pool: PgPool,
    signer: Algorithm,
    verifier: Algorithm,
}

pub enum AuthQueryError {
    BadToken,
    InvalidSession,
    NotInGuild,
    MissingPermission,
    CacheError,
}

impl AuthQueryError {
    pub fn as_api_error(&self) -> ApiError {
        match self {
            AuthQueryError::BadToken => ApiError::BadRequest("Invalid token"),
            AuthQueryError::InvalidSession => ApiError::InvalidSession,
            AuthQueryError::NotInGuild => ApiError::MissingPermission,
            AuthQueryError::MissingPermission => ApiError::MissingPermission,
            AuthQueryError::CacheError => ApiError::Internal(anyhow::Error::msg("Cache error")),
        }
    }
}

impl AuthModule {
    pub fn new(pool: PgPool, signer: Algorithm, verifier: Algorithm) -> Self {
        Self {
            users: Default::default(),
            sessions: Default::default(),
            pool, signer, verifier,
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        let rows = sqlx::query("select id, guilds from BotUsers")
            .map(|row: PgRow| (row.get::<SqlId<UserId>, _>("id").0,
                row.get::<Vec<i64>, _>("guilds").iter().map(|s| GuildId(*s as u64)).collect::<HashSet<GuildId>>()))
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let mut guilds = HashMap::new();
            for guild in row.1 {
                guilds.insert(guild, HashMap::new());
            }
            self.users.write().await.insert(row.0, Arc::new(BotUser {
                id: row.0,
                guilds: RwLock::new(guilds),
                sessions: Default::default()
            }));
        }

        let rows = sqlx::query("select id, user_id, expire_at from Sessions")
            .map(|row: PgRow| (row.get("id"), row.get::<SqlId<UserId>, _>("user_id").0, row.get("expire_at")))
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let user = self.users.read().await.get(&row.1).unwrap().clone();
            self.sessions.write().await.insert(row.0, Arc::new(Session {
                id: row.0,
                user,
                expire_at: row.2
            }));
        }

        Ok(())
    }

    async fn new_user(&self, user: CurrentUser, guilds: Vec<GuildInfo>) -> Result<Arc<BotUser>> {
        sqlx::query("insert into BotUsers (id, guilds) values ($1, $2)")
            .bind(SqlId(user.id))
            .bind(guilds.iter().map(|s| s.id.0 as i64).collect::<Vec<i64>>())
            .execute(&self.pool)
            .await?;

        let mut guild_map = HashMap::new();
        for guild in guilds {
            guild_map.insert(guild.id, HashMap::new());
        }
        let bot_user = Arc::new(BotUser {
            id: user.id,
            guilds: RwLock::new(guild_map),
            sessions: Default::default()
        });
        self.users.write().await.insert(user.id, bot_user.clone());

        Ok(bot_user)
    }

    pub async fn new_session(&self, user: CurrentUser, guilds: Vec<GuildInfo>) -> Result<String> {
        // why do we need to do this
        let handle = self.users.read().await;
        let bot_user = match handle.get(&user.id).cloned() {
            Some(s) => {
                drop(handle);
                s
            },
            None => {
                drop(handle);
                self.new_user(user, guilds).await?
            },
        };

        let expire_at = naive_now() + Duration::days(7);
        let row = sqlx::query("insert into Sessions (user_id, expire_at) values ($1, $2) returning id")
            .bind(SqlId(bot_user.id))
            .bind(expire_at.clone())
            .fetch_one(&self.pool)
            .await?;

        let session_id = row.get("id");
        let session = Arc::new(Session {
            id: session_id,
            user: bot_user.clone(),
            expire_at
        });

        bot_user.sessions.write().await.push(Arc::downgrade(&session));
        self.sessions.write().await.insert(session.id, session);
        let token = self.sign_session_token(session_id).ok_or_else(|| anyhow::Error::msg("Error signing token"))?;

        Ok(token)
    }

    pub async fn query(&self, session_token: &str, guild: Option<&GuildId>) -> StdResult<Arc<Session>, AuthQueryError> {
        let session_id = self.verify_session_token(session_token).ok_or(AuthQueryError::BadToken)?;

        let session = self.sessions.read().await.get(&session_id).ok_or(AuthQueryError::InvalidSession)?.clone();

        if naive_now() > session.expire_at {
            // expire session. note that we don't change the database, as that will be handled by a task
            self.sessions.write().await.remove(&session_id);
            return Err(AuthQueryError::InvalidSession)
        }

        if let Some(guild) = guild {
            if !session.user.guilds.read().await.contains_key(&guild) {
                return Err(AuthQueryError::NotInGuild)
            }
        }

        Ok(session)
    }

    pub async fn query_permission(&self, ctx: &ApiContext, session_token: &str, guild: &GuildId, permission: &PermissionType) -> StdResult<Arc<Session>, AuthQueryError> {
        let session = self.query(session_token, Some(guild)).await?;

        let pair = match session.user.guilds.read().await.get(&guild) {
            None => return Err(AuthQueryError::NotInGuild),
            Some(cache) => cache.get(&permission).cloned()
        };

        if pair.is_none() || SystemTime::now() > pair.unwrap().1 {
            // refresh permissions
            let member = guild.member(&ctx, &session.user.id).await.map_err(|_| AuthQueryError::CacheError)?;
            let mut upgraded_roles = Vec::new();
            let owner = ctx.cache.guild_field(guild, |g| {
                for role in member.roles {
                    match g.roles.get(&role) {
                        Some(data) => upgraded_roles.push(data.clone()),
                        None => {}
                    }
                }
                g.owner_id
            }).ok_or(AuthQueryError::CacheError)?;
            let data = ctx.permissions.check(&permission, &guild, &owner, &session.user.id, &upgraded_roles).await;
            let pair = (match data {
                Some(_) => false,
                None => true,
            }, SystemTime::now() + StdDuration::from_secs(60 * 15));
            session.user.guilds.write().await.get_mut(&guild).ok_or(AuthQueryError::CacheError)?.insert(*permission, pair);
            if data.is_some() {
                Err(AuthQueryError::MissingPermission)
            } else {
                Ok(session)
            }
        } else {
            let (has, _) = pair.unwrap();
            if has {
                Ok(session)
            } else {
                Err(AuthQueryError::MissingPermission)
            }
        }
    }

    fn sign_session_token(&self, session: i64) -> Option<String> {
        let message = session.to_string();
        let signature = self.signer.sign(&message).ok()?;
        Some(format!("{}.{}", message, signature))
    }

    fn verify_session_token(&self, token: &str) -> Option<i64> {
        let (message, signature) = token.split_once(".")?;
        self.verifier.verify(None, &message, &signature).ok()?;
        Some(i64::from_str(message).ok()?)
    }
}
