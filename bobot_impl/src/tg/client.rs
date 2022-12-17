use botapi::{bot::Bot, ext::LongPoller};

use futures::StreamExt;

use super::{user::GetUser, Result};

pub struct TgClient {
    pub client: Bot,
}

impl TgClient {
    pub fn connect<T>(token: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            client: Bot::new(token).unwrap(),
        }
    }

    pub async fn run(&self) -> Result<()> {
        log::debug!("run");
        let poll = LongPoller::new(&self.client);
        poll.get_updates()
            .await
            .for_each_concurrent(None, |update| async move {
                tokio::spawn(async move {
                    match update {
                        Ok(update) => {
                            if let Err(err) = update.record_user().await {
                                log::error!("failed to record_user: {}", err);
                            }
                            crate::modules::process_updates(update).await;
                        }
                        Err(err) => {
                            log::error!("failed to process update: {}", err);
                        }
                    }
                });
            })
            .await;
        Ok(())
    }

    pub fn client<'a>(&'a self) -> &'a Bot {
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
