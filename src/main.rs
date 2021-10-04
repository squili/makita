// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

#![feature(decl_macro)]

mod config;
mod handler;
mod logging;
mod tasks;
mod utils;
mod decode;
mod router;
mod sql;
mod macros;
mod error;
mod custom_ids;
mod cli;
mod modules;

use crate::config::Config;
use crate::handler::Handler;
use anyhow::Result;
use log::info;
use log::LevelFilter;
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::http::Http;
use serenity::Client;
use sqlx::postgres::PgPoolOptions;
use std::fs::read_to_string;
use std::sync::Arc;
use crate::tasks::background_task;
use chrono::Duration;
use serenity::model::interactions::application_command::ApplicationCommand;
use serenity::http::request::RequestBuilder;
use serenity::http::routing::RouteInfo;
use crate::cli::{Opts, Subcommand};
use clap::Clap;
use serenity::model::id::ApplicationId;

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcommand {
        Subcommand::Run => start().await,
        Subcommand::Init => cli::init(),
    }
}

async fn start() -> Result<()> {
    logging::init(LevelFilter::Info, LevelFilter::Warn)?;

    info!("hello world");

    let config: Config = ron::from_str(&*read_to_string("config.ron")?)?;
    let config = Arc::new(config);

    info!("connecting to database");
    let pool = PgPoolOptions::new().connect(&config.database_url).await?;

    sqlx::migrate!().run(&pool).await?;

    info!("initializing handler");
    let application_id = Http::new_with_token(&config.token).get_current_application_info().await?.id.0;
    let handler = Handler {
        pool: pool.clone(),
        application_id: ApplicationId(application_id),
        permissions: modules::PermissionsModule::new(config.owner_id.clone(), pool.clone()),
        previews_module: modules::PreviewsModule::new(pool.clone())?,
    };

    info!("initializing modules");
    handler.permissions.initialize().await?;
    handler.previews_module.initialize().await?;

    info!("initializing client");
    let mut client = Client::builder(&config.token)
        .application_id(application_id)
        .event_handler(handler)
        .intents(
            GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_MEMBERS,
        )
        .await?;

    info!("initializing application commands");
    let command_data = include_bytes!(env!("MAKITA_SLASH_LOCATION"));
    let mut iter = command_data.split(|c| c == &('\n' as u8));

    let mut req = RequestBuilder::new(RouteInfo::CreateGuildApplicationCommand {
        application_id: client.cache_and_http.http.application_id,
        guild_id: config.manager_guild.0,
    });
    req.body(iter.next());
    let _: ApplicationCommand = client.cache_and_http.http.fire(req.build()).await?;

    for entry in iter {
        let _: ApplicationCommand = match config.commands_guild {
            Some(id) => {
                let mut req = RequestBuilder::new(RouteInfo::CreateGuildApplicationCommand {
                    application_id: client.cache_and_http.http.application_id,
                    guild_id: id.0,
                });
                req.body(Some(entry));
                client.cache_and_http.http.fire(req.build()).await?
            }
            None => {
                let mut req = RequestBuilder::new(RouteInfo::CreateGlobalApplicationCommand {
                    application_id: client.cache_and_http.http.application_id,
                });
                req.body(Some(entry));
                client.cache_and_http.http.fire(req.build()).await?
            }
        };
    }

    info!("spawning tasks");
    let cache_clone = client.cache_and_http.cache.clone();
    let pool_clone = pool.clone();
    tokio::spawn(async {
        background_task("Guild Cleanup", move || {
            tasks::guild_cleanup(cache_clone.clone(), pool_clone.clone())
        }, Duration::days(1)).await;
    });

    info!("starting client");
    client.start().await?;

    Ok(())
}
