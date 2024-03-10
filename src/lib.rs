//! # Dijkstra: A modular telegram bot framework.
//!
//! Dijkstra is a high level framework for creating telegram bots
//! featuring tools for group admistration, text formatting, rich media, peristance, caching,
//! and telemetry/metrics.
//!
//! Dijkstra is under heavy development and the API is not considered stable yet. Check back later for a future
//! stable release.
use metadata::Metadata;

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
pub(crate) mod logger;

/// Static values for bot api, database, redis, and config
pub mod statics;

use macros::get_langs;

pub use botapi;
pub use lazy_static;
pub use macros;
pub use once_cell;
pub use redis;
pub use sea_orm;
pub use sea_orm_migration;
pub use sea_query;
pub use serde;
pub use serde_json;
use statics::Config;
use tg::client::UpdateHandler;
pub use uuid;
#[cfg(not(test))]
pub mod init;

get_langs!();

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
}
