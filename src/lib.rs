use prometheus::default_registry;
use prometheus_hyper::Server;
use sea_orm::ConnectionTrait;
use statics::{CONFIG, EXEC};
use tg::client::TgClient;
use tokio::sync::Notify;
use util::error::Result;
pub mod metadata;
pub mod modules;
pub mod persist;
pub mod tg;
pub mod util;

mod logger;
pub mod statics;

use macros::get_langs;

use crate::statics::CLIENT_BACKEND;
get_langs!();

fn init_db() {
    println!(
        "db initialized, mock: {}",
        &statics::DB.is_mock_connection()
    );
}
pub fn run_bot() {
    EXEC.block_on(async move {
        let handle = prometheus_serve();
        drop(handle);
        let me = statics::TG.client.get_me().await.unwrap();
        statics::ME.set(me).unwrap();
        statics::TG.run().await.unwrap();
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

pub fn run(client: TgClient) {
    let mut handle = logger::setup_log();
    CLIENT_BACKEND.set(client).unwrap();
    init_db();
    run_bot();
    println!("complete");
    handle.join();
}
