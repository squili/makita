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
use sqlx::{Pool, Postgres, Row};
use std::future::Future;
use std::ops::Add;
use std::sync::Arc;
use tokio::time::sleep;

pub async fn background_task<C, Fut>(name: &'static str, call: C, wait: Duration)
where
    C: Fn() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let wait = wait.to_std().unwrap();
    loop {
        sleep(wait).await;
        if let Err(e) = call().await {
            error!("Error in task {}: {:?}", name, e);
        }
    }
}

pub async fn guild_cleanup(cache: Arc<Cache>, pool: Pool<Postgres>) -> Result<()> {
    info!("starting guild cleanup");

    let guilds = cache.guilds().await;

    let iter = sqlx::query("select id from Guilds where expiration is null")
        .map(|row: PgRow| row.get::<i64, &str>("id") as u64)
        .fetch_all(&pool)
        .await?;

    for item in iter {
        if !guilds.contains(&GuildId(item)) {
            sqlx::query("update Guilds set expiration = $1 where id = $2")
                .bind(Utc::now().add(Duration::days(90)))
                .bind(item as i64)
                .execute(&pool)
                .await?;
        }
    }

    sqlx::query("delete from Guilds where expiration < now()")
        .execute(&pool)
        .await?;

    Ok(())
}
