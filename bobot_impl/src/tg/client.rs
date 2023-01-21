use std::collections::{HashMap, VecDeque};

use botapi::{
    bot::Bot,
    ext::{BotUrl, LongPoller, Webhook},
    gen_types::{CallbackQuery, InlineKeyboardButton, Message, UpdateExt},
};
use dashmap::DashMap;
use macros::rlformat;

use super::{
    dialog::{Conversation, ConversationState},
    markdown::MarkupBuilder,
    user::get_me,
    user::RecordUser,
};
use crate::{
    metadata::Metadata,
    modules,
    tg::command::{parse_cmd, TextArg},
    util::{
        callback::{SingleCallback, SingleCb},
        string::should_ignore_chat,
    },
};
use crate::{
    statics::{CONFIG, TG},
    util::string::get_chat_lang,
};
use anyhow::{anyhow, Result};
use futures::{Future, StreamExt};
use std::sync::Arc;

static INVALID: &str = "invalid";

pub struct MetadataCollection {
    pub helps: HashMap<String, String>,
    pub modules: HashMap<String, Metadata>,
}

impl MetadataCollection {
    fn get_module_text(&self, module: &str) -> String {
        self.modules
            .get(module)
            .map(|v| {
                let helps = v
                    .commands
                    .iter()
                    .map(|(c, h)| format!("/{}: {}", c, h))
                    .collect::<Vec<String>>()
                    .join("\n");
                format!("{}:\n{}", v.name, helps)
            })
            .unwrap_or_else(|| INVALID.to_owned())
    }

    pub async fn get_conversation(&self, message: &Message) -> Result<Conversation> {
        let me = get_me().await?;

        let lang = get_chat_lang(message.get_chat().get_id()).await?;
        let mut state = ConversationState::new_prefix(
            "help".to_owned(),
            rlformat!(lang, "welcome", me.get_first_name()),
            message.get_chat().get_id(),
            message
                .get_from()
                .map(|u| u.get_id())
                .ok_or_else(|| anyhow!("not user"))?,
            "button",
        )?;

        let start = state.get_start()?.state_id;
        self.modules.iter().for_each(|(_, n)| {
            let s = state.add_state(self.get_module_text(&n.name));
            state.add_transition(start, s, n.name.clone());
            state.add_transition(s, start, "Back");
        });

        let conversation = state.build();
        conversation.write_self().await?;
        Ok(conversation)
    }
}

pub struct TgClient {
    pub client: Bot,
    pub modules: Arc<MetadataCollection>,
    pub button_events: Arc<DashMap<String, SingleCb<CallbackQuery, ()>>>,
}

async fn show_help<'a>(
    args: VecDeque<TextArg<'a>>,
    message: &Message,
    helps: Arc<MetadataCollection>,
) -> Result<bool> {
    if !should_ignore_chat(message.get_chat().get_id()).await? {
        let lang = get_chat_lang(message.get_chat().get_id()).await?;
        let cnf = rlformat!(lang, "commandnotfound");
        if let Some(TextArg::Arg(cmd)) = args.front() {
            let cmd = helps.helps.get(*cmd).map(|v| v.as_str()).unwrap_or(&cnf);
            let mut builder = MarkupBuilder::new();
            let (cmd, entities) = builder
                .strikethrough("@everyone")
                .text(format!(" {}", cmd))
                .build();
            TG.client()
                .build_send_message(message.get_chat().get_id(), &cmd)
                .entities(&entities)
                .reply_to_message_id(message.get_message_id())
                .build()
                .await?;
        } else {
            let me = get_me().await?;
            TG.client()
                .build_send_message(
                    message.get_chat().get_id(),
                    &rlformat!(lang, "welcome", me.get_first_name()),
                )
                .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                    helps
                        .get_conversation(&message)
                        .await?
                        .get_current_markup()
                        .await?,
                ))
                .reply_to_message_id(message.get_message_id())
                .build()
                .await?;
        }
    }

    Ok(true)
}

async fn handle_help(update: &UpdateExt, helps: Arc<MetadataCollection>) -> Result<bool> {
    if let UpdateExt::Message(ref message) = update {
        if let Some((cmd, args, _)) = parse_cmd(message) {
            return match cmd {
                "help" => show_help(args.args, message, helps).await,
                _ => Ok(false),
            };
        }
    }
    return Ok(false);
}

impl TgClient {
    pub fn register_button<F, Fut>(&self, button: &InlineKeyboardButton, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if let Some(data) = button.get_callback_data() {
            log::info!("registering button callback with data {}", data);
            self.button_events
                .insert(data.into_owned(), SingleCb::new(func));
        }
    }
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
                        if let Some(cb) = callbacks.remove(data.as_ref()) {
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

    pub async fn run(&self) -> Result<()> {
        log::info!("run");
        match CONFIG.webhook.enable_webhook {
            false => {
                self.client
                    .build_delete_webhook()
                    .drop_pending_updates(true)
                    .build()
                    .await?;
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

    pub fn client<'a>(&'a self) -> &'a Bot {
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
