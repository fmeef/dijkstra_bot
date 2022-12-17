use anyhow::anyhow;
use botapi::gen_types::{Chat, Message};
use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::persist::redis::RedisStr;
use crate::statics::REDIS;
use crate::util::error::BotError;
use log::info;

use crate::persist::Result;
#[cfg(test)]
mod tests {

    use super::Conversation;
}

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
pub struct Conversation {
    pub conversation_id: Uuid,
    pub triggerphrase: String,
    pub chat: i64,
    pub user: i64,
    pub states: HashMap<Uuid, FSMState>,
    start: Uuid,
    pub transitions: HashMap<String, FSMTransition>,
    rediskey: String,
}

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

impl Conversation {
    pub fn add_state<S: Into<String>>(&mut self, reply: S) -> Uuid {
        let state = FSMState::new(self.conversation_id, false, reply.into());
        let uuid = state.state_id;
        self.states.insert(state.state_id, state);
        uuid
    }

    pub fn add_transition<S: Into<String>>(
        &mut self,
        start: Uuid,
        end: Uuid,
        triggerphrase: S,
    ) -> Uuid {
        let transition = FSMTransition::new(start, end);
        let uuid = transition.transition_id;
        self.transitions.insert(triggerphrase.into(), transition);
        uuid
    }

    pub fn new(triggerphrase: String, reply: String, chat: i64, user: i64) -> Result<Self> {
        let conversation_id = Uuid::new_v4();
        let startstate = FSMState::new(conversation_id, true, reply);
        let mut states = HashMap::<Uuid, FSMState>::new();
        let start = startstate.state_id;
        states.insert(startstate.state_id, startstate);
        let conversation = Conversation {
            conversation_id,
            triggerphrase,
            chat,
            states,
            start,
            user,
            transitions: HashMap::<String, FSMTransition>::new(),
            rediskey: get_state_key(chat, user),
        };

        Ok(conversation)
    }

    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
        if let Some(start) = self.states.get(&self.start) {
            Ok(start)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub async fn transition<'a, S>(&'a self, next: S) -> Result<&'a str>
    where
        S: Into<String>,
    {
        let current = if let Some(next) = self.transitions.get(&next.into()) {
            if let Some(next) = self.states.get(&next.end_state) {
                Ok(next)
            } else {
                Err(BotError::new("invalid choice"))
            }
        } else {
            Err(BotError::new("invalid choice"))
        }?;
        self.write_key(current.state_id).await?;
        Ok(&current.content)
    }

    pub async fn write_key(&self, new: Uuid) -> Result<()> {
        REDIS
            .pipe(|p| p.set(&self.rediskey.to_string(), new.to_string()))
            .await?;
        Ok(())
    }

    pub async fn write_self(&self) -> Result<()> {
        REDIS
            .pipe(|p| p.set(&self.rediskey.to_string(), self.start.to_string()))
            .await
    }

    pub async fn get_current<'a>(&'a self) -> Result<&'a FSMState> {
        let current: String = REDIS.sq(|p| p.get(&self.rediskey)).await?;
        let current = Uuid::from_str(&current)?;
        if let Some(current) = self.states.get(&current) {
            Ok(current)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub async fn get_current_text(&self) -> Result<String> {
        let c = self.get_current().await?.content.to_string();
        Ok(c)
    }

    pub async fn reset(self) -> Result<()> {
        self.write_key(self.start).await
    }
}

pub(crate) async fn get_conversation(message: &Message) -> Result<Option<Conversation>> {
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

pub(crate) async fn drop_converstaion(message: &Message) -> Result<()> {
    let key = get_conversation_key_message(message)?;
    REDIS.sq(|p| p.del(&key)).await?;
    Ok(())
}

pub(crate) async fn replace_conversation<F>(message: &Message, create: F) -> Result<Conversation>
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
            p.set(&conversation.rediskey, conversation.start.to_string())
        })
        .await?;
    Ok(conversation)
}

pub(crate) async fn get_or_create_conversation<F>(
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
                p.set(&res.rediskey, res.start.to_string())
            })
            .await?;
        Ok(res)
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
