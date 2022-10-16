use botapi::{bot::Bot, ext::LongPoller};

use futures::StreamExt;

use super::Result;

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
        let poll = LongPoller::new(&self.client);
        poll.get_updates()
            .await
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
