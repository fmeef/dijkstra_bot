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
    CallbackQuery, Chat, InlineKeyboardButtonBuilder, InlineKeyboardMarkup, Message,
};
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use futures::FutureExt;
use lazy_static::__Deref;
use sea_orm::sea_query::OnConflict;
use sea_orm::EntityTrait;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use uuid::Uuid;

use crate::persist::core::dialogs;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache, RedisStr};
use crate::statics::{CONFIG, DB, REDIS, TG};
use crate::tg::button::OnPush;
use crate::util::error::BotError;
use log::info;

use std::sync::Arc;

use super::admin_helpers::IntoChatUser;
use super::button::InlineKeyboardBuilder;
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

/// Get chat settings for a specific chat
pub async fn get_dialog(chat: &Chat) -> Result<Option<dialogs::Model>> {
    let chat_id = chat.get_id();
    let key = get_dialog_key(chat.get_id());
    let res = default_cache_query(
        |_, _| async move {
            let res = dialogs::Entity::find_by_id(chat_id)
                .one(DB.deref().deref())
                .await?;
            Ok(res)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await?;
    Ok(res)
}

/// Update or insert a chat settings value
pub async fn upsert_dialog(model: dialogs::Model) -> Result<()> {
    let key = get_dialog_key(model.chat_id);
    dialogs::Entity::insert(model.cache(key).await?)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::WarnLimit)
                .to_owned(),
        )
        .exec(DB.deref().deref())
        .await?;
    Ok(())
}

/// Get chat settings for a chat or initialize it with the default values
pub async fn dialog_or_default(chat: &Chat) -> Result<dialogs::Model> {
    let key = get_dialog_key(chat.get_id());
    let model = if let Some(model) = get_dialog(chat).await? {
        model
    } else {
        dialogs::Entity::insert(dialogs::Model::from_chat(chat).await?.cache(key).await?)
            .on_conflict(
                OnConflict::column(dialogs::Column::ChatId)
                    .update_column(dialogs::Column::WarnLimit)
                    .to_owned(),
            )
            .exec_with_returning(DB.deref().deref())
            .await?
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
    state_callback: Option<Box<dyn Fn(Uuid, Conversation) -> () + Send + Sync>>,
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
    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
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
        F: Fn(Uuid, Conversation) -> () + Send + Sync + 'static,
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
    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
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
    pub async fn transition<'a, S>(&'a self, next: S) -> Result<&'a str>
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
        if let Some(cb) = self.0.state_callback.as_ref() {
            cb(current.state_id, self.clone());
        }
        Ok(&current.content)
    }

    /// Manually update the redis key for the current state wtih a new uuid.
    pub async fn write_key(&self, new: Uuid) -> Result<()> {
        REDIS
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
    pub async fn get_current<'a>(&'a self) -> Result<&'a FSMState> {
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
        if let Some(message) = callback.get_message() {
            self.write_key(trans).await?;

            let n = self.get_current_markup(row_limit).await?;
            if let Ok(builder) =
                MarkupBuilder::from_murkdown_chatuser(&content, message.get_chatuser().as_ref())
                    .await
            {
                let (content, entities) = builder.build();
                TG.client()
                    .build_edit_message_text(&content)
                    .message_id(message.get_message_id())
                    .reply_markup(&n)
                    .entities(entities)
                    .chat_id(message.get_chat().get_id())
                    .build()
                    .await?;
            } else {
                TG.client()
                    .build_edit_message_text(&content)
                    .message_id(message.get_message_id())
                    .reply_markup(&n)
                    .chat_id(message.get_chat().get_id())
                    .build()
                    .await?;
            }

            TG.client()
                .build_answer_callback_query(&callback.get_id())
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
                                    log::error!("failed to transition: {}", err);
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
    let key = get_conversation_key_message(&message)?;
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
    REDIS.sq(|p| p.del(&key)).await?;
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
    REDIS
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
        let key = get_conversation_key_message(&message)?;
        REDIS
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
