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

use std::env;
use crate::config::Config;
use crate::handler::Handler;
use anyhow::{Error, Result};
use log::info;
use log::LevelFilter;
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::http::Http;
use serenity::Client;
use sqlx::postgres::PgPoolOptions;
use std::fs::read_to_string;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use async_ctrlc::CtrlC;
use crate::tasks::{background_task, TaskContext, TaskMessage};
use chrono::Duration;
use serenity::model::interactions::application_command::ApplicationCommand;
use serenity::http::request::RequestBuilder;
use serenity::http::routing::RouteInfo;
use crate::cli::{Opts, Subcommand};
use clap::Clap;
use serenity::model::id::ApplicationId;
use tokio::sync::{broadcast, mpsc};
use crate::modules::updates;
use crate::modules::updates::RESTARTING;
use crate::utils::BotContext;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let executable = env::current_exe()?.to_str().unwrap().to_string();

    // bootstrap
    runtime.block_on(bootstrap())?;
    runtime.shutdown_timeout(Duration::minutes(1).to_std()?);

    // restart if required
    if RESTARTING.load(Ordering::SeqCst) {
        info!("restarting");
        return Err(Error::new(Command::new(executable).envs(env::vars()).args(env::args().skip(1)).exec()))
    }

    Ok(())
}

async fn bootstrap() -> Result<()> {
    let opts: Opts = Opts::parse();

    match opts.subcommand {
        Subcommand::Run => start().await,
        Subcommand::Init => cli::init(),
        Subcommand::Invite(opts) => cli::invite(opts).await,
    }
}

async fn start() -> Result<()> {
    logging::init(if cfg!(debug_assertions) { LevelFilter::Debug } else { LevelFilter::Info }, LevelFilter::Warn)?;

    info!("hello world");

    if updates::GIT_META.is_some() {
        info!("checking for updates");
        if let Some(update) = updates::check_update().await? {
            info!("new update available: {} -> {}", update.old_version, update.new_version)
        }
    }

    let config: Config = ron::from_str(&*read_to_string("config.ron")?)?;
    let config = Arc::new(config);

    info!("connecting to database");
    let pool = PgPoolOptions::new().connect(&config.database_url).await?;

    sqlx::migrate!().run(&pool).await?;

    info!("initializing handler");
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    let application_id = Http::new_with_token(&config.token).get_current_application_info().await?.id.0;
    let handler = Handler {
        pool: pool.clone(),
        application_id: ApplicationId(application_id),
        owner_id: config.owner_id,
        updates: Arc::new(modules::UpdatesModule::new(shutdown_tx.clone())),
        permissions: Arc::new(modules::PermissionsModule::new(config.owner_id, pool.clone())),
        previews: Arc::new(modules::PreviewsModule::new()?),
    };

    info!("initializing modules");
    let (task_tx, _) = broadcast::channel(0x400);
    modules::PermissionsModule::initialize(handler.permissions.clone(), task_tx.subscribe()).await?;
    modules::PreviewsModule::initialize(handler.previews.clone(), task_tx.subscribe(), &pool).await?;

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
    let mut iter = command_data.split(|c| c == &b'\n');

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
    let bot_ctx = BotContext::from_cache_and_http(&client.cache_and_http, &pool);
    let task_ctx = TaskContext::from_bot_context(&bot_ctx, &task_tx);
    let task_ctx_clone = task_ctx.clone();
    tokio::spawn(async move {
        background_task("Guild Cleanup", |ctx| tasks::guild_cleanup(ctx.clone()), task_ctx_clone, Duration::days(1)).await;
    });

    info!("starting client");
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        CtrlC::new().expect("Error registering CtrlC handler").await;
        info!("goodbye!");
        shutdown_tx_clone.send(()).await.expect("Error sending shutdown signal");
    });

    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        if let Err(err) = client.start().await {
            log::error!("client error: {}", err)
        }
    });

    shutdown_rx.recv().await; // wait for shutdown
    shard_manager.lock().await.shutdown_all().await; // shutdown gateway connection
    task_tx.send(TaskMessage::Kill)?; // shutdown tasks

    Ok(())
}
