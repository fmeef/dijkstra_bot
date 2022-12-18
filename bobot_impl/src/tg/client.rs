use std::collections::{HashMap, VecDeque};

use botapi::{
    bot::Bot,
    ext::LongPoller,
    gen_types::{Message, UpdateExt},
};

use super::user::GetUser;
use crate::{
    modules,
    tg::command::{parse_cmd, Arg},
};
use futures::StreamExt;
use std::sync::Arc;
pub struct TgClient {
    pub client: Bot,
    pub helps: Arc<HashMap<String, String>>,
}
use crate::statics::TG;
use anyhow::Result;

async fn show_help(
    args: VecDeque<Arg>,
    message: &Message,
    helps: Arc<HashMap<String, String>>,
) -> Result<bool> {
    let cnf = &"Command not found".to_owned();
    if let Some(Arg::Arg(ref cmd)) = args.front() {
        let cmd = helps.get(cmd).unwrap_or(cnf);
        TG.client()
            .build_send_message(message.get_chat().get_id(), cmd)
            .reply_to_message_id(message.get_message_id())
            .build()
            .await?;
    } else {
        TG.client()
            .build_send_message(
                message.get_chat().get_id(),
                "Stop being the deflate brain menhera and fucking google it",
            )
            .reply_to_message_id(message.get_message_id())
            .build()
            .await?;
    }

    Ok(true)
}

async fn handle_help(update: &UpdateExt, helps: Arc<HashMap<String, String>>) -> Result<bool> {
    if let UpdateExt::Message(ref message) = update {
        if let Ok((Arg::Arg(cmd), args)) = parse_cmd(message.get_text().unwrap_or("")) {
            return match cmd.as_str() {
                "/help" => show_help(args, message, helps).await,
                _ => Ok(false),
            };
        }
    }
    return Ok(false);
}

impl TgClient {
    pub fn connect<T>(token: T) -> Self
    where
        T: Into<String>,
    {
        let metadata = modules::get_metadata();
        Self {
            client: Bot::new(token).unwrap(),
            helps: Arc::new(
                metadata
                    .iter()
                    .flat_map(|v| v.commands.iter())
                    .map(|(c, h)| (c.to_owned(), h.to_owned()))
                    .collect(),
            ),
        }
    }
    pub async fn run(&self) -> Result<()> {
        log::debug!("run");
        let poll = LongPoller::new(&self.client);
        poll.get_updates()
            .await
            .for_each_concurrent(None, |update| async move {
                let helps = Arc::clone(&self.helps);
                tokio::spawn(async move {
                    match update {
                        Ok(update) => {
                            if let Err(err) = update.record_user().await {
                                log::error!("failed to record_user: {}", err);
                            }

                            match handle_help(&update, helps).await {
                                Ok(false) => crate::modules::process_updates(update).await,
                                Err(err) => log::error!("failed to show help: {}", err),
                                _ => (),
                            }
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
            helps: self.helps.clone(),
        }
    }
}
