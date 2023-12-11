use dijkstra::{statics::CONFIG, tg::client::TgClient};

pub fn main() {
    dijkstra::run(TgClient::connect(CONFIG.bot_token.to_owned()));
}
