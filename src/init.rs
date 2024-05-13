use crate::persist::redis::RedisPoolBuilder;
use crate::statics;
use crate::statics::{
    Args, ARGS, CLIENT_BACKEND, CONFIG, CONFIG_BACKEND, DB_BACKEND, EXEC, REDIS_BACKEND,
};
use crate::tg::client::TgClient;
use crate::util::error::{BotError, Result};
use crate::{logger, DijkstraOpts};
use clap::Parser;
use confy::load_path;
use nonblock_logger::JoinHandle;
use prometheus::default_registry;
use prometheus_hyper::Server;
use sea_orm::{ConnectOptions, Database};
use tokio::sync::Notify;

fn prometheus_serve() -> tokio::task::JoinHandle<Result<()>> {
    tokio::spawn(async move {
        Server::run(
            default_registry(),
            CONFIG.logging.prometheus_hook,
            Notify::new().notified(),
        )
        .await?;
        Ok(())
    })
}

impl DijkstraOpts {
    async fn init_real(self) -> Result<JoinHandle> {
        ARGS.set(Args::parse()).unwrap();
        let config = if let Some(config) = self.config {
            config
        } else {
            load_path(&ARGS.get().unwrap().config).expect("failed to load config")
        };
        CONFIG_BACKEND.set(config).unwrap();

        let db = Database::connect(ConnectOptions::new(
            CONFIG.persistence.database_connection.to_owned(),
        ))
        .await?;
        DB_BACKEND.set(db).unwrap();

        let log_handle = logger::setup_log();

        let client = if let Some(metadata) = self.modules {
            TgClient::connect_mod(&CONFIG.bot_token, metadata, self.handler)
        } else {
            TgClient::connect(&CONFIG.bot_token)
        };
        CLIENT_BACKEND.set(client).unwrap();

        REDIS_BACKEND
            .set(
                RedisPoolBuilder::new(&CONFIG.persistence.redis_connection)
                    .build()
                    .await?,
            )
            .map_err(|_| BotError::generic("Failed to set RedisBackend"))?;
        Ok(log_handle)
    }

    /// Initialize and run the bot
    pub fn run(self) {
        EXEC.block_on(async move {
            let mut log_handle = self.init_real().await.expect("failed to init state");

            let handle = prometheus_serve();
            let me = statics::TG.client.get_me().await.unwrap();
            statics::ME.set(me).unwrap();
            statics::TG.run().await.unwrap();
            handle.await.unwrap().unwrap();
            log_handle.join();
        });
    }
}
