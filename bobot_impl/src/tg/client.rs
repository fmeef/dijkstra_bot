use std::collections::{HashMap, VecDeque};

use botapi::{
    bot::Bot,
    ext::LongPoller,
    gen_types::{
        CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
        Message, UpdateExt,
    },
};
use dashmap::DashMap;

use super::{
    button::{InlineKeyboardBuilder, OnPush},
    user::GetUser,
};
use crate::statics::TG;
use crate::{
    metadata::Metadata,
    modules,
    tg::command::{parse_cmd, Arg},
    util::callback::{SingleCallback, SingleCb},
};
use anyhow::Result;
use futures::{Future, StreamExt};
use std::sync::Arc;

pub(crate) struct MetadataCollection {
    pub(crate) helps: HashMap<String, String>,
    pub(crate) modules: HashMap<String, Metadata>,
}

impl MetadataCollection {
    pub(crate) fn get_all_help(&self) -> String {
        self.modules
            .iter()
            .map(|(m, _)| format!("{}: send /help {}", m, m))
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub(crate) fn get_markup(&self) -> InlineKeyboardMarkup {
        self.modules
            .iter()
            .fold(InlineKeyboardBuilder::default(), |builder, (m, _)| {
                let button = InlineKeyboardButtonBuilder::new(m.clone())
                    .set_callback_data(Some("fmef".to_owned()))
                    .build();
                button.on_push(|q| async move {
                    log::info!("callback query from {}", q.get_id());
                    TG.client()
                        .build_answer_callback_query(q.get_id())
                        .build()
                        .await
                        .unwrap();
                });
                builder.button(button)
            })
            .build()
    }
}

pub(crate) struct TgClient {
    pub(crate) client: Bot,
    pub(crate) modules: Arc<MetadataCollection>,
    pub(crate) button_events: Arc<DashMap<String, SingleCb<CallbackQuery, ()>>>,
}

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
            .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                helps.get_markup(),
            ))
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
    pub(crate) fn register_button<F, Fut>(&self, button: &InlineKeyboardButton, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if let Some(data) = button.get_callback_data() {
            log::info!("registering button callback with data {}", data);
            self.button_events
                .insert(data.to_owned(), SingleCb::new(func));
        }
    }
    pub(crate) fn connect<T>(token: T) -> Self
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
            button_events: Arc::new(DashMap::new()),
        }
    }
    pub(crate) async fn run(&self) -> Result<()> {
        log::debug!("run");
        let poll = LongPoller::new(&self.client);
        poll.get_updates()
            .await
            .for_each_concurrent(None, |update| async move {
                let modules = Arc::clone(&self.modules);
                let callbacks = Arc::clone(&self.button_events);
                tokio::spawn(async move {
                    match update {
                        Ok(UpdateExt::CallbackQuery(callbackquery)) => {
                            if let Some(data) = callbackquery.get_data() {
                                if let Some(cb) = callbacks.remove(data) {
                                    cb.1.cb(callbackquery).await;
                                }
                            }
                        }
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

    pub(crate) fn client<'a>(&'a self) -> &'a Bot {
        &self.client
    }
}

impl Clone for TgClient {
    fn clone(&self) -> Self {
        TgClient {
            client: self.client.clone(),
            modules: Arc::clone(&self.modules),
            button_events: Arc::clone(&self.button_events),
        }
    }
}
