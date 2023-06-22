// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::prelude::*;
use crate::utils::{BotContext, SqlId};
use anyhow::Result;
use chrono::{Duration, Utc};
use serenity::client::Cache;
use serenity::http::{CacheHttp, Http};
use serenity::model::id::GuildId;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use std::future::Future;
use tokio::sync::broadcast;
use tokio::time::sleep;

#[derive(Clone, Debug)]
pub enum TaskMessage {
    Kill,
    DestroyGuild(GuildId),
}

#[derive(Clone)]
pub struct TaskContext {
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub pool: PgPool,
    pub task_tx: broadcast::Sender<TaskMessage>,
}

impl TaskContext {
    pub fn from_bot_context(b_ctx: &BotContext, task_tx: &broadcast::Sender<TaskMessage>) -> Self {
        Self {
            http: b_ctx.http.clone(),
            cache: b_ctx.cache.clone(),
            pool: b_ctx.pool.clone(),
            task_tx: task_tx.clone(),
        }
    }
}

impl AsRef<Http> for TaskContext {
    fn as_ref(&self) -> &Http {
        &self.http
    }
}

impl AsRef<Cache> for TaskContext {
    fn as_ref(&self) -> &Cache {
        &self.cache
    }
}

impl CacheHttp for TaskContext {
    fn http(&self) -> &Http {
        &self.http
    }

    fn cache(&self) -> Option<&Arc<Cache>> {
        Some(&self.cache)
    }
}

pub async fn background_task<C, Fut>(name: &'static str, call: C, ctx: TaskContext, wait: Duration)
where
    C: Fn(&TaskContext) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let wait = wait.to_std().unwrap();
    let mut task_rx = ctx.task_tx.subscribe();
    loop {
        if tokio::select! {
            _ = sleep(wait) => false,
            resp = task_rx.recv() => match resp {
                Ok(msg) => matches!(msg, TaskMessage::Kill),
                Err(e) => {
                    error!("Error in broadcast receive in {}: {:?}", name, e);
                    true
                },
            },
        } {
            break;
        }
        if let Err(e) = call(&ctx).await {
            error!("Error in task {}: {:?}", name, e);
        }
    }
}

pub async fn guild_cleanup(ctx: TaskContext) -> Result<()> {
    debug!("starting guild cleanup");

    let guilds = ctx.cache.guilds();

    let iter = sqlx::query("select id from Guilds where expiration is null")
        .map(|row: PgRow| row.get::<i64, &str>("id") as u64)
        .fetch_all(&ctx.pool)
        .await?;

    for item in iter {
        if !guilds.contains(&GuildId(item)) {
            debug!("marking guild {} for cleanup", item);
            sqlx::query("update Guilds set expiration = $1 where id = $2")
                .bind(Utc::now() + Duration::days(90))
                .bind(item as i64)
                .execute(&ctx.pool)
                .await?;
        }
    }

    for guild in sqlx::query("delete from Guilds where expiration < now() returning id")
        .map(|row: PgRow| row.get::<SqlId<GuildId>, &str>("id").0)
        .fetch_all(&ctx.pool)
        .await?
    {
        debug!("deleting guild {}", guild);
        ctx.task_tx.send(TaskMessage::DestroyGuild(guild))?;
    }

    Ok(())
}
