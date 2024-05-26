//! Telegram client wrapper with webhook support. Handles incomming updates from telegram
//! and forwards them to modules. This type indexes module metadata and autogenerates a help
//! command handler as well. Due to rust async limitations with the borrow checker this type
//! is most useful from a static context only

use std::collections::HashMap;

use super::{
    admin_helpers::is_dm,
    button::InlineKeyboardBuilder,
    command::{Context, TextArgs},
    dialog::{dialog_from_update, Conversation, ConversationState},
    permissions::*,
    user::RecordUser,
};
use crate::{
    metadata::{markdownify, Metadata},
    modules,
    tg::{
        admin_helpers::IntoChatUser,
        command::{post_deep_link, PopSlice},
        markdown::MarkupBuilder,
    },
    util::{
        callback::{MultiCallback, MultiCb, SingleCallback, SingleCb},
        error::Fail,
        string::{should_ignore_chat, Speak},
    },
};
use crate::{
    statics::{CONFIG, ME, TG},
    util::error::Result,
    util::string::get_chat_lang,
};
use botapi::{
    bot::{ApiError, Bot, BotBuilder},
    ext::{BotUrl, LongPoller, Webhook},
    gen_types::{
        CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder,
        LinkPreviewOptionsBuilder, Message, ReplyParametersBuilder, UpdateExt,
    },
};
use convert_case::Case;
use convert_case::Casing;
use dashmap::DashMap;
use futures::{future::BoxFuture, Future, StreamExt};
use macros::{lang_fmt, message_fmt};
use std::sync::Arc;

static INVALID: &str = "invalid";

/// List of module info for populating bot help
#[derive(Debug)]
pub struct MetadataCollection(HashMap<String, Arc<Metadata>>);

impl MetadataCollection {
    fn get_module_text(&self, module: &str) -> String {
        self.0
            .get(module)
            .map(|v| {
                let helps = v
                    .commands
                    .iter()
                    .map(|(c, h)| format!("/{}: {}", c, markdownify(h)))
                    .collect::<Vec<String>>()
                    .join("\n");

                if !v.commands.is_empty() {
                    format!("[*{}]:\n{}\n\nCommands:\n{}", v.name, v.description, helps)
                } else {
                    format!("[*{}]\n{}", v.name, v.description)
                }
            })
            .unwrap_or_else(|| INVALID.to_owned())
    }

    async fn get_conversation(
        &self,
        message: &Message,
        current: Option<String>,
    ) -> Result<Conversation> {
        let me = ME.get().unwrap();

        let lang = get_chat_lang(message.get_chat().get_id()).await?;
        let mut state = ConversationState::new_prefix(
            "help".to_owned(),
            lang_fmt!(lang, "welcome", me.get_first_name()),
            message.get_chat().get_id(),
            message
                .get_from()
                .map(|u| u.get_id())
                .ok_or_else(|| message.fail_err("User does not exist"))?,
            "button",
        )?;

        let start = state.get_start()?.state_id;
        self.0.iter().for_each(|(_, n)| {
            let s = state.add_state(self.get_module_text(&n.name));
            state.add_transition(start, s, n.name.to_lowercase(), n.name.to_case(Case::Title));
            state.add_transition(s, start, "back", "Back");
            n.sections.iter().for_each(|(sub, content)| {
                let sb = state.add_state(content);
                state.add_transition(s, sb, sub.to_lowercase(), sub.to_case(Case::Title));
                state.add_transition(sb, s, "back", "Back");
            });
        });

        let conversation = state.build();
        conversation.write_self().await?;
        if let Some(mut current) = current {
            for (module, v) in self.0.iter() {
                // log::info!("checking {:?}", v.commands);
                if v.commands.contains_key(&current) {
                    current = module.to_lowercase();
                    break;
                }
            }
            conversation.transition(current).await?;
        }
        Ok(conversation)
    }
}

pub type UpdateCallback =
    Arc<dyn for<'b> Fn(&'b Context) -> BoxFuture<'b, Result<()>> + Send + Sync>;

/// wrapper around a function that is called once for every update received by the bot
pub struct UpdateHandler(Option<UpdateCallback>);

impl UpdateHandler {
    pub(crate) async fn handle_update(&self, ctx: &Context) {
        if let Some(ref custom) = self.0 {
            if let Err(err) = custom(ctx).await {
                log::warn!("failed to process update from custom handler {:?}", err);
                err.record_stats();
                match err.get_message().await {
                    Err(err) => {
                        log::warn!("failed to send error message: {}, what the FLOOP", err);
                        err.record_stats();
                    }
                    Ok(v) => {
                        if !v {
                            if let Some(chat) = ctx.chat() {
                                if let Err(err) = chat.reply(err.to_string()).await {
                                    log::warn!("triple fault! {}", err);
                                }
                            }

                            log::warn!("handle_update custom error: {}", err);
                        }
                    }
                }
            }
        }
    }

    /// Construct a new update handler without a contained function. This handler does nothing.
    pub fn new() -> Self {
        Self(None)
    }

    /// Set the update handler function
    pub fn handler<F>(mut self, func: F) -> Self
    where
        F: for<'b> Fn(&'b Context) -> BoxFuture<'b, Result<()>> + Send + Sync + 'static,
    {
        self.0 = Some(Arc::new(func));
        self
    }

    /// returns true if the UpdateHandler contains a function
    pub fn has_handler(&self) -> bool {
        self.0.is_some()
    }
}

impl Default for UpdateHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for UpdateHandler {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for UpdateHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("its a function uwu")?;
        Ok(())
    }
}

/// Modular telegram client with long polling and webhook support
#[derive(Debug)]
pub struct TgClient {
    pub client: Bot,
    pub modules: Arc<MetadataCollection>,
    pub token: String,
    pub button_events: Arc<DashMap<String, SingleCb<CallbackQuery, Result<()>>>>,
    pub button_repeat: Arc<DashMap<String, MultiCb<CallbackQuery, Result<bool>>>>,
    handler: UpdateHandler,
}

/// Helper function to show the interactive help menu.
pub(crate) async fn show_help<'a>(
    ctx: &Context,
    message: &Message,
    helps: Arc<MetadataCollection>,
    args_raw: &'a TextArgs<'a>,
) -> Result<bool> {
    if !should_ignore_chat(message.get_chat().get_id()).await? {
        let lang = get_chat_lang(message.get_chat().get_id()).await?;

        let param = args_raw
            .args
            .first()
            .map(|v| v.get_text())
            .map(|v| v.to_lowercase());

        let args = args_raw.as_slice();
        let args = args.pop_slice().map(|(_, v)| v);
        log::info!("custom help {:?}", param);
        if is_dm(message.get_chat()) {
            let me = ME.get().unwrap();

            let conv = match helps.get_conversation(message, param.clone()).await {
                Ok(v) => v,
                Err(_) => {
                    message
                        .reply(lang_fmt!(
                            lang,
                            "invalid_help",
                            param.as_deref().unwrap_or("default")
                        ))
                        .await?;
                    return Ok(false);
                }
            };
            if let Some(mut args) = args {
                while let Some((arg, a)) = args.pop_slice() {
                    args = a;
                    let arg = arg.get_text().to_lowercase();
                    match conv.transition(&arg).await {
                        Ok(_) => (),
                        Err(_) => {
                            message.reply(lang_fmt!(lang, "invalid_help", arg)).await?;
                            return Ok(false);
                        }
                    }
                }
            }
            let current = conv.get_current().await?;

            let m = if current.state_id == conv.get_start()?.state_id {
                lang_fmt!(lang, "welcome", me.get_first_name())
            } else {
                current.content.clone()
            };

            let (text, entities, _) = MarkupBuilder::new(None)
                .set_text(m)
                .filling(false)
                .header(false)
                .chatuser(message.get_chatuser().as_ref())
                .build_murkdown_nofail()
                .await;

            TG.client()
                .build_send_message(message.get_chat().get_id(), &text)
                .entities(&entities)
                .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                    conv.get_current_markup(3).await?,
                ))
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .reply_parameters(&ReplyParametersBuilder::new(message.get_message_id()).build())
                .build()
                .await?;
        } else {
            let url = post_deep_link(args_raw, help_key).await?;
            let mut button = InlineKeyboardBuilder::default();

            button.button(
                InlineKeyboardButtonBuilder::new(lang_fmt!(ctx, "helpbutton"))
                    .set_url(url)
                    .build(),
            );
            message_fmt!(ctx, "dmhelp")
                .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                    button.build(),
                ))
                .reply_parameters(&ReplyParametersBuilder::new(message.get_message_id()).build())
                .build()
                .await?;
        }
    }

    Ok(true)
}

#[inline(always)]
pub fn help_key(key: &str) -> String {
    format!("gethelp:{}", key)
}

impl TgClient {
    /// Register a button callback to be called when the corresponding callback button sends an update
    /// This callback will only fire once and be removed afterwards
    pub(crate) fn register_button<F, Fut>(&self, button: &InlineKeyboardButton, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        if let Some(data) = button.get_callback_data() {
            self.button_events
                .insert(data.to_owned(), SingleCb::new(func));
        }
    }

    /// Register a button callback to be called when the corresponding callback button sends an update
    /// This callback will be called any number of times until the callback returns false
    pub(crate) fn register_button_multi<F, Fut>(&self, button: &InlineKeyboardButton, func: F)
    where
        F: Fn(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        if let Some(data) = button.get_callback_data() {
            self.button_repeat
                .insert(data.to_owned(), MultiCb::new(func));
        }
    }

    /// Creates a new client from a bot api token
    pub fn connect<T>(token: T) -> Self
    where
        T: Into<String>,
    {
        let metadata = modules::get_metadata();
        let metadata = MetadataCollection(
            metadata
                .into_iter()
                .map(|v| (v.name.clone(), Arc::new(v)))
                .collect(),
        );
        let token = token.into();
        Self {
            client: BotBuilder::new(token.clone())
                .unwrap()
                .auto_wait(true)
                .build(),
            token,
            modules: Arc::new(metadata),
            button_events: Arc::new(DashMap::new()),
            button_repeat: Arc::new(DashMap::new()),
            handler: UpdateHandler(None),
        }
    }

    /// Creates a new client from a bot api token
    pub fn connect_mod<T>(token: T, metadata: Vec<Metadata>, handler: UpdateHandler) -> Self
    where
        T: Into<String>,
    {
        let metadata = MetadataCollection(
            metadata
                .into_iter()
                .map(|v| (v.name.clone(), Arc::new(v)))
                .collect(),
        );
        let token = token.into();
        Self {
            client: BotBuilder::new(token.clone())
                .unwrap()
                .auto_wait(true)
                .build(),
            token,
            modules: Arc::new(metadata),
            button_events: Arc::new(DashMap::new()),
            button_repeat: Arc::new(DashMap::new()),
            handler,
        }
    }

    /// Processes a single update from telegram
    async fn handle_update(&self, update: std::result::Result<UpdateExt, ApiError>) {
        let modules = Arc::clone(&self.modules);
        let callbacks = Arc::clone(&self.button_events);
        let repeats = Arc::clone(&self.button_repeat);
        let custom_handler = self.handler.clone();
        tokio::spawn(async move {
            match update {
                Ok(UpdateExt::CallbackQuery(callbackquery)) => {
                    if let Some(data) = callbackquery.get_data() {
                        let data: String = data.to_owned();
                        if let Some(cb) = callbacks.remove(&data) {
                            if let Err(err) = cb.1.cb(callbackquery.clone()).await {
                                log::warn!("button handler err {}", err);
                                err.record_stats();
                            }
                        }

                        let remove = if let Some(cb) = repeats.get(&data) {
                            match cb.cb(callbackquery).await {
                                Err(err) => {
                                    log::warn!("failed multi handler {}", err);
                                    err.record_stats();
                                    true
                                }
                                Ok(v) => {
                                    if v {
                                        log::info!("removing multi callback");
                                    }
                                    v
                                }
                            }
                        } else {
                            false
                        };

                        if remove {
                            repeats.remove(&data);
                        }
                    }
                }
                Ok(update) => {
                    if let Err(err) = update_self_admin(&update).await {
                        log::warn!("failed to update admin change: {}", err);
                        err.record_stats();
                    }

                    if let Err(err) = update.record_user().await {
                        log::warn!("failed to record_user: {}", err);
                        err.record_stats();
                    }

                    if let Err(err) = dialog_from_update(&update).await {
                        log::warn!("failed to update dialog from update");
                        err.record_stats();
                    }

                    if let Err(err) =
                        crate::modules::process_updates(update, modules, custom_handler).await
                    {
                        log::warn!("process updates error: {}", err);
                        err.record_stats()
                    }
                }
                Err(err) => {
                    log::warn!("failed to process update: {}", err);
                }
            }
        });
    }

    /// Handles updates from telegram forever either using webhooks or long polling
    /// depending on toml config
    pub async fn run(&self) -> Result<()> {
        log::info!("run");
        let updates = Some(
            vec![
                "update_id",
                "message",
                "edited_message",
                "channel_post",
                "edited_channel_post",
                "inline_query",
                "chosen_inline_result",
                "callback_query",
                "shipping_query",
                "pre_checkout_query",
                "poll",
                "poll_answer",
                "my_chat_member",
                "chat_member",
                "chat_join_request",
            ]
            .into_iter()
            .map(|v| v.to_owned())
            .collect(),
        );
        match CONFIG.webhook.enable_webhook {
            false => {
                self.client
                    .build_delete_webhook()
                    .drop_pending_updates(true) // TODO: change this
                    .build()
                    .await?;
                LongPoller::new(&self.client, updates)
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
                    updates,
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

    pub fn client(&self) -> &'_ Bot {
        &self.client
    }
}

impl Clone for TgClient {
    fn clone(&self) -> Self {
        TgClient {
            token: self.token.clone(),
            client: self.client.clone(),
            modules: Arc::clone(&self.modules),
            button_events: Arc::clone(&self.button_events),
            button_repeat: Arc::clone(&self.button_repeat),
            handler: UpdateHandler(self.handler.0.clone()),
        }
    }
}
