// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use anyhow::Result;
use chrono::{Duration, Utc};
use log::{error, info};
use serenity::client::Cache;
use serenity::model::id::GuildId;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Pool, Postgres, Row};
use std::future::Future;
use std::sync::Arc;
use serenity::http::{CacheHttp, Http};
use tokio::sync::{mpsc, oneshot, broadcast};
use tokio::time::sleep;
use crate::utils::BotContext;

#[derive(Clone)]
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

pub async fn background_task<C, Fut>(name: &'static str, call: C, mut ctx: TaskContext, wait: Duration)
where
    C: Fn(&TaskContext) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let wait = wait.to_std().unwrap();
    let mut task_rx = ctx.task_tx.subscribe();
    loop {
        if tokio::select! {
            _ = sleep(wait) => false,
            msg = task_rx.recv() => match msg {
                Kill => true,
                _ => false,
            },
        } {
            break
        }
        if let Err(e) = call(&ctx).await {
            error!("Error in task {}: {:?}", name, e);
        }
    }
}

pub async fn guild_cleanup(ctx: TaskContext) -> Result<()> {
    info!("starting guild cleanup");

    let guilds = ctx.cache.guilds().await;

    let iter = sqlx::query("select id from Guilds where expiration is null")
        .map(|row: PgRow| row.get::<i64, &str>("id") as u64)
        .fetch_all(&ctx.pool)
        .await?;

    for item in iter {
        if !guilds.contains(&GuildId(item)) {
            sqlx::query("update Guilds set expiration = $1 where id = $2")
                .bind(Utc::now() + Duration::days(90))
                .bind(item as i64)
                .execute(&ctx.pool)
                .await?;
        }
    }

    sqlx::query("delete from Guilds where expiration < now()")
        .execute(&ctx.pool)
        .await?;

    Ok(())
}
