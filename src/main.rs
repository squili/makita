// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

#![feature(decl_macro)]
#![feature(try_trait_v2)]
#![feature(once_cell)]

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
mod api;
mod prelude;

use prelude::*;
use std::{env, fs};
use crate::config::Config;
use crate::handler::Handler;
use anyhow::{Error, Result};
use log::LevelFilter;
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::Client;
use sqlx::postgres::PgPoolOptions;
use std::fs::read_to_string;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::Ordering;
use async_ctrlc::CtrlC;
use crate::tasks::{background_task, TaskContext, TaskMessage};
use chrono::Duration;
use serenity::model::interactions::application_command::ApplicationCommand;
use serenity::http::request::RequestBuilder;
use serenity::http::routing::RouteInfo;
use crate::cli::{Opts, Subcommand};
use clap::Clap;
use jsonwebtokens::{Algorithm, AlgorithmID};
use serenity::model::id::{ApplicationId, UserId};
use tokio::sync::{broadcast, mpsc};
use crate::modules::updates;
use crate::modules::updates::RESTARTING;

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

    // define modules outside of handler so we can use it in the api
    let updates_module = Arc::new(modules::UpdatesModule::new(shutdown_tx.clone()));
    let permissions_module = Arc::new(modules::PermissionsModule::new(UserId(config.owner_id), pool.clone()));
    let previews_module = Arc::new(modules::PreviewsModule::new()?);
    let auth_module = Arc::new(modules::AuthModule::new(pool.clone(),
        Algorithm::new_ecdsa_pem_signer(AlgorithmID::ES256, &fs::read("public.pem")?)?,
        Algorithm::new_ecdsa_pem_verifier(AlgorithmID::ES256, &fs::read("private.pem")?)?,
    ));
    let utils_module = Arc::new(modules::UtilsModule::new());

    let handler = Handler {
        pool: pool.clone(),
        application_id: ApplicationId(config.client_id),
        owner_id: UserId(config.owner_id),
        updates: updates_module.clone(),
        permissions: permissions_module.clone(),
        previews: previews_module.clone(),
        auth: auth_module.clone(),
        utils: utils_module,
    };

    info!("initializing modules");
    let (task_tx, _) = broadcast::channel(0x400);
    modules::PermissionsModule::initialize(permissions_module.clone(), task_tx.subscribe()).await?;
    modules::PreviewsModule::initialize(previews_module.clone(), task_tx.subscribe(), &pool).await?;
    auth_module.initialize().await?;

    info!("initializing client");
    let mut client = Client::builder(&config.token)
        .application_id(config.client_id)
        .event_handler(handler)
        .intents(
            GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_MEMBERS,
        )
        .await?;

    if option_env!("SKIP_COMMANDS").is_some() {
        warn!("skipping initializing application commands");
    } else {
        info!("initializing application commands");
        let command_data = include_bytes!(env!("MAKITA_SLASH_LOCATION"));
        let mut iter = command_data.split(|c| c == &b'\n');

        let mut req = RequestBuilder::new(RouteInfo::CreateGuildApplicationCommand {
            application_id: client.cache_and_http.http.application_id,
            guild_id: config.manager_guild,
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
    }

    info!("spawning tasks");
    let bot_ctx = BotContext::from_cache_and_http(&client.cache_and_http, &pool);
    let task_ctx = TaskContext::from_bot_context(&bot_ctx, &task_tx);
    let task_ctx_clone = task_ctx.clone();
    tokio::spawn(async move {
        background_task("Guild Cleanup", |ctx| tasks::guild_cleanup(ctx.clone()), task_ctx_clone, Duration::days(1)).await;
    });

    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        CtrlC::new().expect("Error registering CtrlC handler").await;
        info!("goodbye!");
        shutdown_tx_clone.send(()).await.expect("Error sending shutdown signal");
    });

    info!("spawning api server");
    api::start(Arc::new(ApiContext {
        http: client.cache_and_http.http.clone(),
        cache: client.cache_and_http.cache.clone(),
        pool,
        config,
        updates: updates_module,
        permissions: permissions_module,
        previews: previews_module,
        auth: auth_module,
    })).await?;

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
