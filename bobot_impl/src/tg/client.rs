use teloxide::{adaptors::AutoSend, prelude::Requester, types::Update, Bot};

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
        let r = while let Some(updates) = tokio::select! {
            _ = tokio::signal::ctrl_c() => None,
            result = self.client.get_updates() => Some(result)
        } {
            updates?.into_iter().for_each(|update| {
                let c = self.clone();
                tokio::spawn(async move {
                    if let Err(err) = c.single_update(update).await {
                        log::info!("failed to handle update {}", err);
                    }
                });
            })
        };

        Ok(r)
    }

    async fn single_update(self, update: Update) -> Result<()> {
        crate::modules::process_updates(self, update).await;
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
