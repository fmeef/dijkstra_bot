//! # Dijkstra: A modular telegram bot framework.
//!
//! Dijkstra is a high level framework for creating telegram bots
//! featuring tools for group admistration, text formatting, rich media, peristance, caching,
//! and telemetry/metrics.
//!
//! Dijkstra is under heavy development and the API is not considered stable yet. Check back later for a future
//! stable release.

use confy::load_path;
use metadata::Metadata;
use prometheus::default_registry;
use prometheus_hyper::Server;
use sea_orm::ConnectionTrait;
use statics::{Config, CONFIG, EXEC};
use tg::client::{TgClient, UpdateHandler};
use tokio::sync::Notify;
use util::error::Result;

/// Utilities for keeping track of the module list and generating the help menu.
pub mod metadata;

/// Built in modules compiled into this bot. Accessible via the src/modules directory.
pub mod modules;

/// Database, caching, and serialization tools.
pub mod persist;

/// Helper utilities for interacting with telegram api and group administration tools.
pub mod tg;

/// Misc utilities.
pub mod util;

/// Internal logger framework, external code should just use log crate
mod logger;

/// Static values for bot api, database, redis, and config
pub mod statics;

use macros::get_langs;

use crate::statics::{ARGS, CLIENT_BACKEND, CONFIG_BACKEND};
get_langs!();

fn init_db() {
    println!(
        "db initialized, mock: {}",
        &statics::DB.is_mock_connection()
    );
}
fn run_bot() {
    EXEC.block_on(async move {
        let handle = prometheus_serve();
        let me = statics::TG.client.get_me().await.unwrap();
        statics::ME.set(me).unwrap();
        statics::TG.run().await.unwrap();
        handle.await.unwrap().unwrap();
    });
}

fn prometheus_serve() -> tokio::task::JoinHandle<Result<()>> {
    tokio::spawn(async move {
        Server::run(
            default_registry(),
            CONFIG.logging.prometheus_hook.clone(),
            Notify::new().notified(),
        )
        .await?;
        Ok(())
    })
}

/// Configuration options for starting a bot instance.
pub struct DijkstraOpts {
    config: Option<Config>,
    modules: Option<Vec<Metadata>>,
    handler: UpdateHandler,
}

impl DijkstraOpts {
    /// Constructs a new empty config
    pub fn new() -> Self {
        Self {
            config: None,
            modules: None,
            handler: UpdateHandler::new(),
        }
    }

    /// Adds an external module list to this bot. This overrides any built-in modules in the help menu.
    /// to disable any built-in commands also use the Module section of the Config type
    pub fn modules(mut self, modules: Vec<Metadata>) -> Self {
        self.modules = Some(modules);
        self
    }

    /// Add a custom configration to this bot, overriding the config parsed from config.toml via
    /// the --config argument.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Adds a custom update handler to run in addition to the builtin modules's update handlers.
    pub fn update_handler(mut self, update_handler: UpdateHandler) -> Self {
        self.handler = update_handler;
        self
    }

    /// Initialize and run the bot
    pub fn run(self) {
        let config = if let Some(config) = self.config {
            config
        } else {
            load_path(&ARGS.config).expect("failed to load config")
        };
        CONFIG_BACKEND.set(config).unwrap();

        let mut handle = logger::setup_log();

        let client = if let Some(metadata) = self.modules {
            TgClient::connect_mod(&CONFIG.bot_token, metadata, self.handler)
        } else {
            TgClient::connect(&CONFIG.bot_token)
        };
        CLIENT_BACKEND.set(client).unwrap();
        init_db();
        run_bot();
        println!("complete");
        handle.join();
    }
}
