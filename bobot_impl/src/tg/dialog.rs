use ::redis::AsyncCommands;
use anyhow::anyhow;
use botapi::gen_types::{
    CallbackQuery, Chat, InlineKeyboardButtonBuilder, InlineKeyboardMarkup, Message,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::persist::redis::RedisStr;
use crate::statics::{REDIS, TG};
use crate::tg::button::OnPush;
use crate::util::error::BotError;
use log::info;

use crate::persist::Result;
use std::sync::Arc;

use super::button::InlineKeyboardBuilder;
pub const TYPE_DIALOG: &str = "DialogDb";

#[inline(always)]
fn get_conversation_key_prefix(chat: i64, user: i64, prefix: &str) -> String {
    format!("{}:{}:{}", prefix, chat, user)
}

#[inline(always)]
pub fn get_conversation_key(chat: i64, user: i64) -> String {
    format!("conv:{}:{}", chat, user)
}

#[inline(always)]
pub fn get_state_key(chat: i64, user: i64) -> String {
    get_conversation_key_prefix(chat, user, "convstate")
}

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
        Err(anyhow!(BotError::new("message does not have sender")))
    }
}

#[inline(always)]
pub fn get_conversation_key_message(message: &Message) -> Result<String> {
    get_conversation_key_message_prefix(message, "conv")
}

#[inline(always)]
pub fn get_state_key_message(message: &Message) -> Result<String> {
    get_conversation_key_message_prefix(message, "convstate")
}

#[derive(Serialize, Deserialize)]
pub struct ConversationState {
    pub conversation_id: Uuid,
    pub triggerphrase: String,
    pub chat: i64,
    pub user: i64,
    pub states: HashMap<Uuid, FSMState>,
    start: Uuid,
    pub transitions: HashMap<(Uuid, String), FSMTransition>,
    rediskey: String,
    #[serde(default, skip)]
    state_callback: Option<Box<dyn Fn(Uuid, Conversation) -> () + Send + Sync>>,
}

#[derive(Serialize, Deserialize)]
pub struct Conversation(Arc<ConversationState>);

#[derive(Serialize, Deserialize)]
pub struct FSMState {
    pub state_id: Uuid,
    pub parent: Uuid,
    pub start_for: Option<Uuid>,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct FSMTransition {
    pub transition_id: Uuid,
    pub start_state: Uuid,
    pub end_state: Uuid,
}

#[derive(Serialize, Deserialize)]
pub struct Dialog {
    pub chat_id: i64,
    pub last_activity: DateTime<chrono::Utc>,
}

impl FSMState {
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
    fn new(start_state: Uuid, end_state: Uuid) -> Self {
        let id = Uuid::new_v4();

        FSMTransition {
            transition_id: id,
            start_state,
            end_state,
        }
    }
}

impl ConversationState {
    pub fn add_transition<S: Into<String>>(
        &mut self,
        start: Uuid,
        end: Uuid,
        triggerphrase: S,
    ) -> Uuid {
        let transition = FSMTransition::new(start, end);
        let uuid = transition.transition_id;
        self.transitions
            .insert((start, triggerphrase.into()), transition);
        uuid
    }

    pub fn add_state<S: Into<String>>(&mut self, reply: S) -> Uuid {
        let state = FSMState::new(self.conversation_id, false, reply.into());
        let uuid = state.state_id;
        self.states.insert(state.state_id, state);
        uuid
    }

    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
        if let Some(start) = self.states.get(&self.start) {
            Ok(start)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub fn new(triggerphrase: String, reply: String, chat: i64, user: i64) -> Result<Self> {
        Self::new_prefix(triggerphrase, reply, chat, user, "convstate")
    }

    pub fn state_callback<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(Uuid, Conversation) -> () + Send + Sync + 'static,
    {
        self.state_callback = Some(Box::new(callback));
        self
    }

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
            transitions: HashMap::new(),
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
    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
        if let Some(start) = self.0.states.get(&self.0.start) {
            Ok(start)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub fn get_state<'a>(&'a self, uuid: &Uuid) -> Option<&'a FSMState> {
        self.0.states.get(uuid)
    }

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
                Err(BotError::new("invalid choice"))
            }
        } else {
            Err(BotError::new("invalid choice current"))
        }?;
        self.write_key(current.state_id).await?;
        if let Some(cb) = self.0.state_callback.as_ref() {
            cb(current.state_id, self.clone());
        }
        Ok(&current.content)
    }

    pub async fn write_key(&self, new: Uuid) -> Result<()> {
        REDIS
            .pipe(|p| p.set(&self.0.rediskey.to_string(), new.to_string()))
            .await?;
        Ok(())
    }

    pub async fn write_self(&self) -> Result<()> {
        REDIS
            .pipe(|p| p.set(&self.0.rediskey.to_string(), self.0.start.to_string()))
            .await
    }

    pub async fn get_current<'a>(&'a self) -> Result<&'a FSMState> {
        let current: String = REDIS.sq(|p| p.get(&self.0.rediskey)).await?;
        let current = Uuid::from_str(&current)?;
        if let Some(current) = self.0.states.get(&current) {
            Ok(current)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    async fn edit_button_transition(
        &self,
        trans: Uuid,
        content: String,
        callback: &CallbackQuery,
    ) -> Result<()> {
        if let Some(message) = callback.get_message() {
            self.write_key(trans).await?;

            let n = self.get_current_markup().await?;
            TG.client()
                .build_edit_message_text(&content)
                .message_id(message.get_message_id())
                .reply_markup(&n)
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

    pub fn get_current_markup(&self) -> BoxFuture<'static, Result<InlineKeyboardMarkup>> {
        let me = self.clone();
        async move {
            let state = me.get_current().await?;
            let markup =
                me.0.transitions
                    .iter()
                    .filter(|(_, t)| t.start_state == state.state_id)
                    .map(|((_, n), t)| {
                        let b = InlineKeyboardButtonBuilder::new(n.to_owned())
                            .set_callback_data(Uuid::new_v4().to_string())
                            .build();
                        let trans = t.end_state.to_owned();
                        if let Some(newstate) = me.0.states.get(&t.end_state) {
                            let content = newstate.content.to_owned();
                            let me = me.clone();
                            b.on_push(move |callback| async move {
                                if let Err(err) =
                                    me.edit_button_transition(trans, content, &callback).await
                                {
                                    log::error!("failed to transition: {}", err);
                                }
                            });
                        }
                        b
                    })
                    .fold(InlineKeyboardBuilder::default(), |builder, st| {
                        builder.button(st)
                    })
                    .build();
            Ok(markup)
        }
        .boxed()
    }

    pub async fn get_current_text(&self) -> Result<String> {
        let c = self.get_current().await?.content.to_string();
        Ok(c)
    }

    pub async fn reset(self) -> Result<()> {
        self.write_key(self.0.start).await
    }
}

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

pub async fn drop_converstaion(message: &Message) -> Result<()> {
    let key = get_conversation_key_message(message)?;
    REDIS.sq(|p| p.del(&key)).await?;
    Ok(())
}

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

pub async fn get_or_create_conversation<F>(
    message: &Message,
    create: F,
) -> Result<Conversation>
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
