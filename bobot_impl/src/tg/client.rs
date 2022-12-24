use std::collections::{HashMap, VecDeque};

use botapi::{
    bot::Bot,
    ext::{BotUrl, LongPoller, Webhook},
    gen_types::{
        CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
        Message, UpdateExt,
    },
};
use dashmap::DashMap;

use super::{
    button::{InlineKeyboardBuilder, OnPush},
    dialog::{Conversation, ConversationState},
    user::GetUser,
};
use crate::statics::{CONFIG, TG};
use crate::{
    metadata::Metadata,
    modules,
    tg::command::{parse_cmd, Arg},
    util::callback::{SingleCallback, SingleCb},
};
use anyhow::{anyhow, Result};
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
                builder.command_button(m.to_owned(), format!("/help {}", m))
            })
            .build()
    }

    pub(crate) async fn get_conversation(&self, message: &Message) -> Result<Conversation> {
        let mut state = ConversationState::new_prefix(
            "/help".to_owned(),
            "Welcome to Default Bot, a modular group management bot written in pyton and asynctio"
                .to_owned(),
            message.get_chat().get_id(),
            message
                .get_from()
                .map(|u| u.get_id())
                .ok_or_else(|| anyhow!("not user"))?,
            "button",
        )?;

        let start = state.get_start()?.state_id;
        self.modules.iter().for_each(|(_, n)| {
            let s = state.add_state(&n.name);
            state.add_transition(start, s, n.name.clone());
            state.add_transition(s, start, "Back");
        });

        let conversation = state.build();
        conversation.write_self().await?;
        Ok(conversation)
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
            .build_send_message(
                message.get_chat().get_id(),
                 "Welcome to Default Bot, a modular group management bot written in pyton and asynctio"
            )
            .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                helps.get_conversation(&message).await?.get_current_markup().await?
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

    async fn handle_update(&self, update: Result<UpdateExt>) {
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
    }

    pub(crate) async fn run(&self) -> Result<()> {
        log::debug!("run");
        match CONFIG.webhook.enable_webhook {
            false => {
                LongPoller::new(&self.client)
                    .get_updates()
                    .await
                    .for_each_concurrent(
                        None,
                        |update| async move { self.handle_update(update).await },
                    )
                    .await
            }
            true => {
                Webhook::new(
                    &self.client,
                    BotUrl::Host(CONFIG.webhook.webhook_url.to_owned()),
                    false,
                    CONFIG.webhook.listen.to_owned(),
                )
                .get_updates()
                .await?
                .for_each_concurrent(
                    None,
                    |update| async move { self.handle_update(update).await },
                )
                .await
            }
        }
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
