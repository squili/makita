// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::sync::Arc;
use serenity::builder::Timestamp;
use anyhow::Result;
use serenity::cache::Cache;
use serenity::{CacheAndHttp, Client};
use serenity::client::Context;
use serenity::http::{CacheHttp, Http};
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::model::interactions::message_component::MessageComponentInteraction;
use sqlx::PgPool;

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
            pool: pool.clone()
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

pub fn remove_indexes<T>(vector: &mut Vec<T>, indexes: &Vec<usize>) -> Vec<T> {
    let mut offset = 0;
    let mut entries = Vec::new();
    for entry in indexes {
        entries.push(vector.remove(entry - offset));
        offset += 1;
    }
    entries
}

pub struct SqlId<T>(pub T) where T: From<u64> + Into<u64>;

pub fn default_arg<T, U: Default>(_: T) -> U { U::default() }

#[derive(Default)]
pub struct FollowupBuilder {
    title: Option<String>,
    description: Option<String>,
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
        match $self.$name {
            Some(s) => { $builder.$name(s); }
            None => {}
        }
    };
}

impl FollowupBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn build_command<T: AsRef<Http>>(self, http: T, interaction: &ApplicationCommandInteraction) -> Result<()> {
        self._build_command(http.as_ref(), interaction).await
    }

    async fn _build_command(self, http: &Http, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.create_followup_message(&http, |m|
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            })
        ).await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component<T: AsRef<Http>>(self, http: T, interaction: &MessageComponentInteraction) -> Result<()> {
        self._build_component(http.as_ref(), interaction).await
    }

    #[allow(unused)]
    async fn _build_component(self, http: &Http, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction.create_followup_message(&http, |m|
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            })
        ).await?;

        Ok(())
    }

    builder_entry!(String, title);
    builder_entry!(String, description);
}

pub macro invite_url {
    ($id: expr) => {
        format!("https://discord.com/oauth2/authorize?client_id={}&permissions=8&scope=applications.commands+bot", $id)
    }
}
