//! Code for managing conversation state machine.
//! Each user/chat pair can have a "dialog", essentially a state machine of queries and responses
//! that can be used as a menu, conversational interface, or other dialog based UI.
//!
//! Dialogs have a starting state, one or more ending states, and a set of transitions and
//! intermediate states. Each state has a message associated with it, and each transition
//! has a prompt that triggers it.
//!
//! Dialogs are often used to make button menus, but can also be used for other freeform
//! text interfaces. Dialogs are serializable to allow them to be saved, shared, and edited
//! by users in json format

use crate::util::error::Result;
use ::redis::AsyncCommands;

use botapi::gen_types::{
    CallbackQuery, Chat, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
    LinkPreviewOptionsBuilder, MaybeInaccessibleMessage, Message, UpdateExt,
};
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;

use futures::FutureExt;

use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use uuid::Uuid;

use crate::persist::core::{chat_members, dialogs};
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisStr, ToRedisStr};
use crate::statics::{CONFIG, DB, REDIS, TG};
use crate::tg::button::OnPush;
use crate::util::error::BotError;
use log::info;

use std::sync::Arc;

use super::admin_helpers::IntoChatUser;
use super::button::InlineKeyboardBuilder;
use super::command::Context;
use super::markdown::MarkupBuilder;
pub const TYPE_DIALOG: &str = "DialogDb";

#[inline(always)]
fn get_conversation_key_prefix(chat: i64, user: i64, prefix: &str) -> String {
    format!("{}:{}:{}", prefix, chat, user)
}

/// get the key for storing chat settings
#[inline(always)]
pub fn get_dialog_key(chat: i64) -> String {
    format!("dia:{}", chat)
}

/// Attempt to record the current dialog from a message.
/// TODO: Remove this later once all existing chats are updated?
pub async fn dialog_from_update(update: &UpdateExt) -> Result<()> {
    if let UpdateExt::Message(message) = update {
        let chat = message.get_chat();
        dialog_or_default(chat).await?;
    }
    Ok(())
}

/// Get chat settings for a specific chat
pub async fn get_dialog(chat: &Chat) -> Result<Option<dialogs::Model>> {
    let chat_id = chat.get_id();
    let key = get_dialog_key(chat.get_id());
    let res = default_cache_query(
        |_, _| async move {
            let res = dialogs::Entity::find_by_id(chat_id).one(*DB).await?;
            Ok(res)
        },
        Duration::try_seconds(CONFIG.timing.cache_timeout).unwrap(),
    )
    .query(&key, &())
    .await?;
    Ok(res)
}

/// Update or insert a chat settings value
pub async fn upsert_dialog<T>(db: &T, model: dialogs::ActiveModel) -> Result<()>
where
    T: ConnectionTrait,
{
    if let Set(key) = model.chat_id {
        let key = get_dialog_key(key);
        let _: () = REDIS.sq(|q| q.del(&key)).await?;
    }
    dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_columns([
                    dialogs::Column::WarnLimit,
                    dialogs::Column::Federation,
                    dialogs::Column::Language,
                    dialogs::Column::ChatType,
                    dialogs::Column::CanSendMessages,
                    dialogs::Column::CanSendAudio,
                    dialogs::Column::CanSendVideo,
                    dialogs::Column::CanSendPhoto,
                    dialogs::Column::CanSendDocument,
                    dialogs::Column::CanSendVoiceNote,
                    dialogs::Column::CanSendVideoNote,
                    dialogs::Column::CanSendPoll,
                    dialogs::Column::CanSendOther,
                    dialogs::Column::WarnTime,
                    dialogs::Column::ActionType,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}

/// Get chat settings for a chat or initialize it with the default values
pub async fn dialog_or_default(chat: &Chat) -> Result<dialogs::Model> {
    let key = get_dialog_key(chat.get_id());
    let model = if let Some(model) = get_dialog(chat).await? {
        model
    } else {
        let d = dialogs::Entity::insert(dialogs::Model::from_chat(chat).await?)
            .on_conflict(
                OnConflict::column(dialogs::Column::ChatId)
                    .update_columns([
                        dialogs::Column::WarnLimit,
                        dialogs::Column::Federation,
                        dialogs::Column::Language,
                        dialogs::Column::ChatType,
                        dialogs::Column::CanSendMessages,
                        dialogs::Column::CanSendAudio,
                        dialogs::Column::CanSendVideo,
                        dialogs::Column::CanSendPhoto,
                        dialogs::Column::CanSendDocument,
                        dialogs::Column::CanSendVoiceNote,
                        dialogs::Column::CanSendVideoNote,
                        dialogs::Column::CanSendPoll,
                        dialogs::Column::CanSendOther,
                        dialogs::Column::WarnTime,
                        dialogs::Column::ActionType,
                    ])
                    .to_owned(),
            )
            .exec_with_returning(*DB)
            .await?;
        let _: () = REDIS
            .try_pipe(|q| {
                Ok(q.set(&key, d.to_redis()?)
                    .expire(&key, CONFIG.timing.cache_timeout))
            })
            .await?;
        d
    };
    Ok(model)
}

/// Get the redis key for a conversation state from user and chat (from message)
#[inline(always)]
fn get_conversation_key_message_prefix(message: &Message, prefix: &str) -> Result<String> {
    if let Some(user) = message.get_from() {
        let res = format!(
            "{}:{}:{}",
            prefix,
            message.get_chat().get_id(),
            user.get_id()
        );
        info!("conversation key: {}", res);
        Ok(res)
    } else {
        Err(BotError::conversation_err("message does not have sender"))
    }
}

#[inline(always)]
fn get_conversation_key_message(message: &Message) -> Result<String> {
    get_conversation_key_message_prefix(message, "conv")
}

#[inline(always)]
fn get_member_key(user: i64) -> String {
    format!("mbr:{}", user)
}

pub async fn update_chat(
    user: i64,
) -> Result<Box<dyn Iterator<Item = chat_members::ActiveModel> + Send>> {
    let key = get_member_key(user);
    if !REDIS.sq(|q| q.exists(&key)).await? {
        let members = chat_members::Entity::find()
            .filter(chat_members::Column::UserId.eq(user))
            .all(*DB)
            .await?;

        if !members.is_empty() {
            let _: () = REDIS
                .pipe(|p| {
                    for v in members.iter() {
                        p.sadd(&key, v.chat_id);
                    }
                    p
                })
                .await?;
        }

        Ok(Box::new(members.into_iter().map(|v| v.into_active_model())))
    } else {
        let (o, _): (Vec<i64>, bool) = REDIS
            .pipe(|p| p.smembers(&key).expire(&key, CONFIG.timing.cache_timeout))
            .await?;
        Ok(Box::new(o.into_iter().map(move |v| {
            chat_members::ActiveModel {
                user_id: Set(user),
                chat_id: Set(v),
                banned_by_me: NotSet,
            }
        })))
    }
}

/// Returns true if the provided user is a member of the provided chat.
/// This relies on an internal cache, so it may not reflect the state of telegram as a whole
pub async fn is_chat_member(user: i64, chat: i64) -> Result<bool> {
    let key = get_member_key(user);
    let v = match REDIS.pipe(|p| p.exists(&key).sismember(&key, chat)).await {
        Ok((true, v)) => Ok::<bool, BotError>(v),
        Err(err) => Err(err),
        Ok((false, _)) => {
            let mut v = update_chat(user).await?;
            let model = chat_members::ActiveModel {
                chat_id: Set(chat),
                user_id: Set(user),
                banned_by_me: NotSet,
            };
            Ok(v.any(|p| p.eq(&model)))
        }
    }?;

    Ok(v)
}

/// Returns an iterator over all the chats a user has been seen in
pub async fn get_user_chats(user: i64) -> Result<impl Iterator<Item = i64> + Send> {
    let v = update_chat(user).await?.filter_map(|v| {
        if let Set(id) = v.chat_id {
            Some(id)
        } else {
            None
        }
    });
    Ok(v)
}

pub async fn get_user_banned_chats(user: i64) -> Result<impl Iterator<Item = i64> + Send> {
    let v = update_chat(user).await?.filter_map(|v| {
        if let Set(false) = v.banned_by_me {
            None
        } else if let Set(id) = v.chat_id {
            Some(id)
        } else {
            None
        }
    });
    Ok(v)
}

impl Context {
    pub async fn record_chat_member(&self) -> Result<()> {
        match self.update() {
            UpdateExt::ChatMember(member) => {
                record_chat_member(member.get_from().get_id(), member.get_chat().get_id()).await
            }
            UpdateExt::Message(message) => {
                if let Some(user) = message.get_from() {
                    record_chat_member(user.get_id(), message.get_chat().get_id()).await?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Updates the chat member cache with new chat membership data
pub async fn record_chat_member(user: i64, chat: i64) -> Result<()> {
    let key = get_member_key(user);
    let (updated, _): (i64, bool) = REDIS
        .pipe(|q| q.sadd(&key, chat).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    log::info!("record_chat_member {}", updated);
    if updated > 0 {
        chat_members::Entity::insert(chat_members::ActiveModel {
            chat_id: Set(chat),
            user_id: Set(user),
            banned_by_me: NotSet,
        })
        .on_conflict(
            OnConflict::columns([chat_members::Column::ChatId, chat_members::Column::UserId])
                .update_columns([chat_members::Column::ChatId, chat_members::Column::UserId])
                .to_owned(),
        )
        .exec(*DB)
        .await?;
    }
    Ok(())
}

/// Updates the list of known chat members with a banned status.
pub async fn record_chat_member_banned(user: i64, chat: i64, banned: bool) -> Result<()> {
    let key = get_member_key(user);
    let (updated, _): (i64, bool) = REDIS
        .pipe(|q| q.sadd(&key, chat).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    log::info!("record_chat_member {}", updated);
    if updated > 0 {
        chat_members::Entity::insert(chat_members::ActiveModel {
            chat_id: Set(chat),
            user_id: Set(user),
            banned_by_me: Set(banned),
        })
        .on_conflict(
            OnConflict::columns([chat_members::Column::ChatId, chat_members::Column::UserId])
                .update_columns([
                    chat_members::Column::ChatId,
                    chat_members::Column::UserId,
                    chat_members::Column::BannedByMe,
                ])
                .to_owned(),
        )
        .exec(*DB)
        .await?;
    }
    Ok(())
}

pub async fn reset_banned_chats(user: i64) -> Result<()> {
    let key = get_member_key(user);
    let _: () = REDIS.sq(|q| q.del(&key)).await?;
    chat_members::Entity::update_many()
        .filter(chat_members::Column::UserId.eq(user))
        .set(chat_members::ActiveModel {
            user_id: NotSet,
            chat_id: NotSet,
            banned_by_me: Set(false),
        })
        .exec(*DB)
        .await?;
    Ok(())
}

/// Internal readonly state for a converstation. Contains metadata for transitions and
/// states
#[derive(Serialize, Deserialize)]
pub struct ConversationState {
    pub conversation_id: Uuid,
    pub triggerphrase: String,
    pub chat: i64,
    pub user: i64,
    pub states: HashMap<Uuid, FSMState>,
    start: Uuid,
    pub transitions: BTreeMap<(Uuid, String), FSMTransition>,
    rediskey: String,
    #[serde(default, skip)]
    state_callback: Option<Box<dyn Fn(Uuid, Conversation) + Send + Sync>>,
}

/// Converstation state machine with internal state backed by redis.
#[derive(Serialize, Deserialize)]
pub struct Conversation(Arc<ConversationState>);

/// State machine state
#[derive(Serialize, Deserialize)]
pub struct FSMState {
    pub state_id: Uuid,
    pub parent: Uuid,
    pub start_for: Option<Uuid>,
    pub content: String,
}

/// State machine transition
#[derive(Serialize, Deserialize)]
pub struct FSMTransition {
    pub transition_id: Uuid,
    pub start_state: Uuid,
    pub end_state: Uuid,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct Dialog {
    pub chat_id: i64,
    pub last_activity: DateTime<chrono::Utc>,
}

impl FSMState {
    /// Creates a new state associated with a conversation id
    fn new(conversation_id: Uuid, is_start: bool, reply: String) -> Self {
        let id = Uuid::new_v4();
        FSMState {
            state_id: id,
            parent: conversation_id,
            start_for: if is_start {
                Some(conversation_id)
            } else {
                None
            },
            content: reply,
        }
    }
}

impl FSMTransition {
    /// Create a new transition associated with a state id
    fn new(start_state: Uuid, end_state: Uuid, name: String) -> Self {
        let id = Uuid::new_v4();

        FSMTransition {
            transition_id: id,
            start_state,
            end_state,
            name,
        }
    }
}

impl ConversationState {
    /// Add a new transition from state id start to state id end triggered by triggerphrase
    /// autogenerating and returning the conversation id
    pub fn add_transition<S: Into<String>>(
        &mut self,
        start: Uuid,
        end: Uuid,
        triggerphrase: S,
        name: S,
    ) -> Uuid {
        let transition = FSMTransition::new(start, end, name.into());
        let uuid = transition.transition_id;
        self.transitions
            .insert((start, triggerphrase.into()), transition);
        uuid
    }

    /// Add a new state with given reply text, autogenerating and returning the state id
    pub fn add_state<S: Into<String>>(&mut self, reply: S) -> Uuid {
        let state = FSMState::new(self.conversation_id, false, reply.into());
        let uuid = state.state_id;
        self.states.insert(state.state_id, state);
        uuid
    }

    /// get a reference to the starting state
    pub fn get_start(&self) -> Result<&'_ FSMState> {
        if let Some(start) = self.states.get(&self.start) {
            Ok(start)
        } else {
            Err(BotError::conversation_err("corrupt graph"))
        }
    }

    /// Create a new ConversationState
    pub fn new(triggerphrase: String, reply: String, chat: i64, user: i64) -> Result<Self> {
        Self::new_prefix(triggerphrase, reply, chat, user, "convstate")
    }

    /// register a callback to be called whenever the state changes
    pub fn state_callback<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(Uuid, Conversation) + Send + Sync + 'static,
    {
        self.state_callback = Some(Box::new(callback));
        self
    }

    /// creates a new Conversation with a redis prefix. This helps if you want
    /// multiple copies of the same conversation each with a different state
    pub fn new_prefix(
        triggerphrase: String,
        reply: String,
        chat: i64,
        user: i64,
        prefix: &str,
    ) -> Result<Self> {
        let conversation_id = Uuid::new_v4();
        let startstate = FSMState::new(conversation_id, true, reply);
        let mut states = HashMap::<Uuid, FSMState>::new();
        let start = startstate.state_id;
        states.insert(startstate.state_id, startstate);
        let state = ConversationState {
            conversation_id,
            triggerphrase,
            chat,
            states,
            start,
            user,
            transitions: BTreeMap::new(),
            rediskey: get_conversation_key_prefix(chat, user, prefix),
            state_callback: None,
        };

        Ok(state)
    }

    pub fn build(self) -> Conversation {
        Conversation(Arc::new(self))
    }
}

impl Conversation {
    /// Get a reference to the starting state of the conversation
    pub fn get_start(&self) -> Result<&'_ FSMState> {
        if let Some(start) = self.0.states.get(&self.0.start) {
            Ok(start)
        } else {
            Err(BotError::conversation_err("corrupt graph"))
        }
    }

    /// get a reference to a stored state by id
    pub fn get_state<'a>(&'a self, uuid: &Uuid) -> Option<&'a FSMState> {
        self.0.states.get(uuid)
    }

    /// Transition this conversation to a new state by transition keyword
    pub async fn transition<S>(&self, next: S) -> Result<&'_ str>
    where
        S: Into<String>,
    {
        let current = self.get_current().await?.state_id;
        let current = if let Some(next) = {
            let n = (current, next.into());
            self.0.transitions.get(&n)
        } {
            if let Some(next) = self.0.states.get(&next.end_state) {
                Ok(next)
            } else {
                Err(BotError::conversation_err("invalid choice"))
            }
        } else {
            Err(BotError::conversation_err("invalid choice current"))
        }?;
        self.write_key(current.state_id).await?;
        log::info!("transition {}", current.state_id);
        if let Some(cb) = self.0.state_callback.as_ref() {
            cb(current.state_id, self.clone());
        }
        Ok(&current.content)
    }

    /// Manually update the redis key for the current state wtih a new uuid.
    pub async fn write_key(&self, new: Uuid) -> Result<()> {
        let _: () = REDIS
            .pipe(|p| p.set(&self.0.rediskey.to_string(), new.to_string()))
            .await?;
        Ok(())
    }

    // Updates the redis key with the initial start state
    pub async fn write_self(&self) -> Result<()> {
        REDIS
            .pipe(|p| p.set(&self.0.rediskey.to_string(), self.0.start.to_string()))
            .await
    }

    /// Gets a reference to the current state using redis
    pub async fn get_current(&self) -> Result<&'_ FSMState> {
        let current: String = REDIS.sq(|p| p.get(&self.0.rediskey)).await?;
        let current = Uuid::from_str(&current)?;
        if let Some(current) = self.0.states.get(&current) {
            Ok(current)
        } else {
            Err(BotError::conversation_err("corrupt graph"))
        }
    }

    async fn edit_button_transition(
        &self,
        trans: Uuid,
        content: String,
        callback: &CallbackQuery,
        row_limit: usize,
    ) -> Result<()> {
        if let Some(MaybeInaccessibleMessage::Message(message)) = callback.get_message() {
            self.write_key(trans).await?;

            if let Some(cb) = self.0.state_callback.as_ref() {
                cb(trans, self.clone());
            }
            let n = self.get_current_markup(row_limit).await?;

            let (text, entities, _) = MarkupBuilder::new(None)
                .set_text(content)
                .filling(false)
                .header(false)
                .chatuser(message.get_chatuser().as_ref())
                .build_murkdown_nofail()
                .await;

            TG.client()
                .build_edit_message_text(&text)
                .message_id(message.get_message_id())
                .reply_markup(&n)
                .entities(&entities)
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .chat_id(message.get_chat().get_id())
                .build()
                .await?;

            TG.client()
                .build_answer_callback_query(callback.get_id())
                .build()
                .await?;
        }
        Ok(())
    }

    /// convert this conversation into a button menu automatically
    /// Returns markup to add to a message
    pub fn get_current_markup(
        &self,
        row_limit: usize,
    ) -> BoxFuture<'static, Result<InlineKeyboardMarkup>> {
        let me = self.clone();
        async move {
            let state = me.get_current().await?;
            let markup =
                me.0.transitions
                    .iter()
                    .filter(|(_, t)| t.start_state == state.state_id)
                    .map(|((_, _), t)| {
                        let b = InlineKeyboardButtonBuilder::new(t.name.clone())
                            .set_callback_data(Uuid::new_v4().to_string())
                            .build();
                        let trans = t.end_state.to_owned();
                        if let Some(newstate) = me.0.states.get(&t.end_state) {
                            let content = newstate.content.to_owned();
                            let me = me.clone();
                            b.on_push(move |callback| async move {
                                if let Err(err) = me
                                    .edit_button_transition(trans, content, &callback, row_limit)
                                    .await
                                {
                                    log::warn!("failed to transition: {}", err);
                                }
                                Ok(())
                            });
                        }
                        b
                    })
                    .fold(&mut InlineKeyboardBuilder::default(), |builder, st| {
                        if builder.row_len() < row_limit {
                            builder.button(st)
                        } else {
                            builder.newline().button(st)
                        }
                    })
                    .to_owned()
                    .build();
            Ok(markup)
        }
        .boxed()
    }

    /// Shortcut to get the current state's text
    pub async fn get_current_text(&self) -> Result<String> {
        let c = self.get_current().await?.content.to_string();
        Ok(c)
    }

    pub async fn reset(self) -> Result<()> {
        self.write_key(self.0.start).await
    }
}

/// gets the current conversation for the chat-user pair (from a message's sender)
pub async fn get_conversation(message: &Message) -> Result<Option<Conversation>> {
    let key = get_conversation_key_message(message)?;
    let rstr = REDIS
        .query(|mut c| async move {
            if c.exists(&key).await? {
                let conv: RedisStr = c.get(&key).await?;
                Ok(Some(conv))
            } else {
                Ok(None)
            }
        })
        .await?;

    let res = if let Some(rstr) = rstr {
        Some(rstr.get::<Conversation>()?)
    } else {
        None
    };

    Ok(res)
}

/// delete the redis key for a conversation
pub async fn drop_converstaion(message: &Message) -> Result<()> {
    let key = get_conversation_key_message(message)?;
    let _: () = REDIS.sq(|p| p.del(&key)).await?;
    Ok(())
}

/// Replace a conversation key with a conversation returned by the function
pub async fn replace_conversation<F>(message: &Message, create: F) -> Result<Conversation>
where
    F: FnOnce(&Message) -> Result<Conversation>,
{
    let key = get_conversation_key_message(message)?;
    let conversation = create(message)?;
    let conversationstr = RedisStr::new(&conversation)?;
    let _: () = REDIS
        .pipe(|p| {
            p.atomic();
            p.set(&key, conversationstr);
            p.set(&conversation.0.rediskey, conversation.0.start.to_string())
        })
        .await?;
    Ok(conversation)
}

/// Get a converstation or initialize it via a default value
pub async fn get_or_create_conversation<F>(message: &Message, create: F) -> Result<Conversation>
where
    F: FnOnce(&Message) -> Result<Conversation>,
{
    if let Some(conversation) = get_conversation(message).await? {
        Ok(conversation)
    } else {
        let res = create(message)?;
        let s = RedisStr::new(&res)?;
        let key = get_conversation_key_message(message)?;
        let _: () = REDIS
            .pipe(|p| {
                p.atomic();
                p.set(&key, s);
                p.set(&res.0.rediskey, res.0.start.to_string())
            })
            .await?;
        Ok(res)
    }
}

impl Clone for Conversation {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Dialog {
    pub fn new(chat: &Chat) -> Self {
        Dialog {
            chat_id: chat.get_id(),
            last_activity: Utc::now(),
        }
    }
}
