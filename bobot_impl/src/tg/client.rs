use std::collections::{HashMap, VecDeque};

use botapi::{
    bot::Bot,
    ext::LongPoller,
    gen_types::{Message, UpdateExt},
};

use super::user::GetUser;
use crate::{
    metadata::Metadata,
    modules,
    tg::command::{parse_cmd, Arg},
};
use futures::StreamExt;
use std::sync::Arc;

pub struct MetadataCollection {
    pub helps: HashMap<String, String>,
    pub modules: HashMap<String, Metadata>,
}

impl MetadataCollection {
    pub fn get_all_help(&self) -> String {
        self.modules
            .iter()
            .map(|(m, _)| format!("{}: send /help {}", m, m))
            .collect::<Vec<String>>()
            .join("\n")
    }
}

pub struct TgClient {
    pub client: Bot,
    pub modules: Arc<MetadataCollection>,
}
use crate::statics::TG;
use anyhow::Result;

async fn show_help(
    args: VecDeque<Arg>,
    message: &Message,
    helps: Arc<MetadataCollection>,
) -> Result<bool> {
    let cnf = "Command not found";
    if let Some(Arg::Arg(ref cmd)) = args.front() {
        let cmd = helps.helps.get(cmd).map(|v| v.as_str()).unwrap_or(cnf);
        TG.client()
            .build_send_message(message.get_chat().get_id(), cmd)
            .reply_to_message_id(message.get_message_id())
            .build()
            .await?;
    } else {
        TG.client()
            .build_send_message(message.get_chat().get_id(), &helps.get_all_help())
            .reply_to_message_id(message.get_message_id())
            .build()
            .await?;
    }

    Ok(true)
}

async fn handle_help(update: &UpdateExt, helps: Arc<MetadataCollection>) -> Result<bool> {
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
        let metadata = MetadataCollection {
            helps: metadata
                .iter()
                .flat_map(|v| v.commands.iter())
                .map(|(c, h)| (c.to_owned(), h.to_owned()))
                .collect(),
            modules: metadata.into_iter().map(|v| (v.name.clone(), v)).collect(),
        };
        Self {
            client: Bot::new(token).unwrap(),
            modules: Arc::new(metadata),
        }
    }
    pub async fn run(&self) -> Result<()> {
        log::debug!("run");
        let poll = LongPoller::new(&self.client);
        poll.get_updates()
            .await
            .for_each_concurrent(None, |update| async move {
                let modules = Arc::clone(&self.modules);
                tokio::spawn(async move {
                    match update {
                        Ok(update) => {
                            if let Err(err) = update.record_user().await {
                                log::error!("failed to record_user: {}", err);
                            }

                            match handle_help(&update, modules).await {
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
            modules: Arc::clone(&self.modules),
        }
    }
}
