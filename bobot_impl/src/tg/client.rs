use std::path::PathBuf;

use grammers_client::{Client, Config, InitParams, Update};
use grammers_session::Session;
use log::debug;

use crate::util::error::BotError;

use super::Result;

pub struct TgClient {
    pub client: Client,
}

impl TgClient {
    pub async fn connect<T>(token: T, api_id: i32, api_hash: T, session: PathBuf) -> Result<Self>
    where
        T: Into<String>,
    {
        let token: String = token.into();
        let api_hash: String = api_hash.into();
        let session: String = session
            .to_str()
            .ok_or_else(|| BotError::new("invalid session file"))?
            .to_owned();
        let mut res = Self {
            client: Client::connect(Config {
                api_id,
                api_hash: api_hash.clone().into(),
                session: Session::load_file_or_create(session.clone())?,
                params: InitParams {
                    catch_up: true,
                    ..Default::default()
                },
            })
            .await?,
        };

        if !res.client.is_authorized().await? {
            res.client
                .bot_sign_in(token.as_ref(), api_id, api_hash.as_ref())
                .await?;
            res.client.session().save_to_file(session)?;
        }

        Ok(res)
    }

    pub async fn run(self) -> Result<()> {
        let r = while let Some(update) = tokio::select! {
            _ = tokio::signal::ctrl_c() => Ok(None),
            result = self.client.next_update() => result,
        }? {
            let c = self.clone();
            tokio::spawn(async move {
                if let Err(err) = c.single_update(update).await {
                    debug!("failed to handle update {}", err);
                }
            });
        };

        Ok(r)
    }

    async fn single_update(self, update: Update) -> Result<()> {
        crate::modules::process_updates(self, update).await;
        Ok(())
    }
}

impl Clone for TgClient {
    fn clone(&self) -> Self {
        TgClient {
            client: self.client.clone(),
        }
    }
}
