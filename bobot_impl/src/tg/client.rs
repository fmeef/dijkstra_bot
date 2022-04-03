use teloxide::{
    adaptors::AutoSend,
    dispatching::update_listeners::{polling_default, AsUpdateStream},
    Bot,
};

use futures::StreamExt;

use super::Result;

pub struct TgClient {
    pub client: AutoSend<Bot>,
}

impl TgClient {
    pub fn connect<T>(token: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            client: AutoSend::new(Bot::new(token)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        polling_default(self.client.clone())
            .await
            .as_stream()
            .for_each_concurrent(None, |update| async move {
                tokio::spawn(async move {
                    if let Ok(update) = update {
                        crate::modules::process_updates(update).await;
                    } else {
                        log::debug!("failed to process update");
                    }
                });
            })
            .await;
        Ok(())
    }

    pub fn client<'a>(&'a self) -> &'a AutoSend<Bot> {
        &self.client
    }
}

impl Clone for TgClient {
    fn clone(&self) -> Self {
        TgClient {
            client: self.client.clone(),
        }
    }
}
