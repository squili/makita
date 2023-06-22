// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

#[cfg(not(unix))]
compile_error!("Platform not supported");

mod cli;
mod config;
mod custom_ids;
mod decode;
mod error;
mod handler;
mod logging;
mod macros;
mod modules;
mod router;
mod sql;
mod tasks;
mod utils;
// mod api;
mod prelude;

use crate::cli::{Opts, Subcommand};
use crate::config::Config;
use crate::handler::Handler;
use crate::modules::updates;
use crate::modules::updates::RESTARTING;
use crate::tasks::{background_task, TaskContext, TaskMessage};
use crate::updates::current_version;
use anyhow::{Error, Result};
use async_ctrlc::CtrlC;
use chrono::Duration;
use clap::Parser;
use log::LevelFilter;
use prelude::*;
use serenity::http::request::RequestBuilder;
use serenity::http::routing::RouteInfo;
use serenity::model::application::command::Command as ApplicationCommand;
use serenity::model::gateway::GatewayIntents;
use serenity::model::id::{ApplicationId, UserId};
use serenity::Client;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::fs::read_to_string;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::Ordering;
use tokio::sync::{broadcast, mpsc};

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
        return Err(Error::new(
            Command::new(executable)
                .envs(env::vars())
                .args(env::args().skip(1))
                .exec(),
        ));
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
    logging::init(
        if cfg!(debug_assertions) {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        },
        LevelFilter::Warn,
    )?;

    info!("makita version {}", current_version());

    let config: Config = ron::from_str(&*read_to_string("config.ron")?)?;
    let config = Arc::new(config);

    info!("connecting to database");
    let pool = PgPoolOptions::new().connect(&config.database_url).await?;

//    sqlx::migrate!().run(&pool).await?;

    info!("initializing handler");
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

    // define modules outside of handler so we can use it in the api
    let updates_module = Arc::new(modules::UpdatesModule::new(
        shutdown_tx.clone(),
        config.client_id,
    ));
    let permissions_module = Arc::new(modules::PermissionsModule::new(pool.clone()));
    let previews_module = Arc::new(modules::PreviewsModule::new()?);
    let utils_module = Arc::new(modules::UtilsModule::new());

    let handler = Handler {
        pool: pool.clone(),
        application_id: ApplicationId(config.client_id),
        owner_id: UserId(config.owner_id),
        updates: updates_module.clone(),
        permissions: permissions_module.clone(),
        previews: previews_module.clone(),
        utils: utils_module,
    };

    info!("initializing modules");
    let (task_tx, _) = broadcast::channel(0x400);
    modules::PermissionsModule::initialize(permissions_module.clone(), task_tx.subscribe()).await?;
    modules::PreviewsModule::initialize(previews_module.clone(), task_tx.subscribe(), &pool)
        .await?;

    info!("initializing client");
    let mut client = Client::builder(
        &config.token,
        GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::MESSAGE_CONTENT,
    )
    .application_id(config.client_id)
    .event_handler(handler)
    .await?;

    if option_env!("SKIP_COMMANDS").is_some() {
        warn!("skipping initializing application commands");
    } else {
        info!("initializing application commands");
        let command_data = include_bytes!(env!("MAKITA_SLASH_LOCATION"));
        for entry in command_data.split(|c| c == &b'\n') {
            let _: ApplicationCommand = match config.commands_guild {
                Some(id) => {
                    let mut req = RequestBuilder::new(RouteInfo::CreateGuildApplicationCommand {
                        application_id: config.client_id,
                        guild_id: id.0,
                    });
                    req.body(Some(entry));
                    client.cache_and_http.http.fire(req.build()).await?
                }
                None => {
                    let mut req = RequestBuilder::new(RouteInfo::CreateGlobalApplicationCommand {
                        application_id: config.client_id,
                    });
                    req.body(Some(entry));
                    client.cache_and_http.http.fire(req.build()).await?
                }
            };
        }
    }

    info!("spawning tasks");
    let bot_ctx = BotContext::from_cache_and_http(&client.cache_and_http, &pool);
    let task_ctx = TaskContext::from_bot_context(&bot_ctx, &task_tx);
    let task_ctx_clone = task_ctx.clone();
    tokio::spawn(async move {
        background_task(
            "Guild Cleanup",
            |ctx| tasks::guild_cleanup(ctx.clone()),
            task_ctx_clone,
            Duration::days(1),
        )
        .await;
    });

    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        CtrlC::new().expect("Error registering CtrlC handler").await;
        info!("goodbye!");
        shutdown_tx_clone
            .send(())
            .await
            .expect("Error sending shutdown signal");
    });

    info!("spawning update server");
    updates::start_update_server(config.clone(), updates_module.clone()).await?;

    info!("starting client");
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
