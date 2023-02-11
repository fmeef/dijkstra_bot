use std::collections::HashMap;

use botapi::{
    bot::{ApiError, Bot},
    ext::{BotUrl, LongPoller, Webhook},
    gen_types::{
        CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder, Message, UpdateExt,
    },
};
use dashmap::DashMap;
use macros::{rlformat, rmformat};

use super::{
    admin_helpers::{handle_pending_action, is_dm},
    button::{get_url, InlineKeyboardBuilder},
    dialog::{Conversation, ConversationState},
    user::get_me,
    user::RecordUser,
};
use crate::{
    metadata::Metadata,
    modules,
    tg::command::parse_cmd,
    util::{
        callback::{SingleCallback, SingleCb},
        error::BotError,
        string::{should_ignore_chat, Speak},
    },
};
use crate::{
    statics::{CONFIG, TG},
    util::error::Result,
    util::string::get_chat_lang,
};
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
            message.get_from().map(|u| u.get_id()).ok_or_else(|| {
                BotError::speak("User does not exist", message.get_chat().get_id())
            })?,
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

async fn show_help<'a>(message: &Message, helps: Arc<MetadataCollection>) -> Result<bool> {
    if !should_ignore_chat(message.get_chat().get_id()).await? {
        let lang = get_chat_lang(message.get_chat().get_id()).await?;
        if is_dm(message.get_chat_ref()) {
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
                        .get_current_markup(3)
                        .await?,
                ))
                .reply_to_message_id(message.get_message_id())
                .build()
                .await?;
        } else {
            let url = get_url("help").await?;
            rmformat!(lang, message.get_chat().get_id(), "dmhelp")
                .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                    InlineKeyboardBuilder::default()
                        .button(
                            InlineKeyboardButtonBuilder::new("Inbix!".to_owned())
                                .set_url(url)
                                .build(),
                        )
                        .build(),
                ))
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
                "help" => show_help(message, helps).await,
                "start" => {
                    if let Some("help") = args.args.first().map(|a| a.get_text()) {
                        show_help(message, helps).await?;
                    } else {
                        message.reply("Hi there start weeenie").await?;
                    }
                    Ok(true)
                }
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

    async fn handle_update(&self, update: std::result::Result<UpdateExt, ApiError>) {
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
                    if let Err(err) = handle_pending_action(&update).await {
                        log::error!("failed to handle pending action: {}", err);
                        err.record_stats();
                    }
                    if let Err(err) = update.record_user().await {
                        log::error!("failed to record_user: {}", err);
                        err.record_stats();
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
