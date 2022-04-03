use teloxide::{adaptors::AutoSend, types::Update, Bot};

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
        Ok(())
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
