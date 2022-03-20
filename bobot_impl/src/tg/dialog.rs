use anyhow::anyhow;
use chrono::{DateTime, Utc};
use grammers_client::types::{Chat, Message};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::persist::redis::RedisStr;
use crate::statics::REDIS;
use crate::util::error::BotError;

use crate::persist::Result;
#[cfg(test)]
mod tests {

    use super::Conversation;
}

pub const TYPE_DIALOG: &str = "DialogDb";

#[inline(always)]
pub fn get_conversation_key(chat: i64, user: i64) -> String {
    format!("conv:{}:{}", chat, user)
}

#[inline(always)]
pub fn get_conversation_key_message(message: &Message) -> Result<String> {
    if let Some(user) = message.sender() {
        let res = format!("conv:{}:{}", message.chat().id(), user.id());
        Ok(res)
    } else {
        Err(anyhow!(BotError::new("message does not have sender")))
    }
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
            rediskey: get_conversation_key(chat, user),
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

    pub async fn transition<S>(&self, next: S) -> Result<()>
    where
        S: Into<String>,
    {
        let current = if let Some(next) = self.transitions.get(&next.into()) {
            if let Some(next) = self.states.get(&next.end_state) {
                Ok(next.state_id)
            } else {
                Err(BotError::new("invalid choice"))
            }
        } else {
            Err(BotError::new("invalid choice"))
        }?;
        self.write_key(current).await?;
        Ok(())
    }

    pub async fn write_key(&self, new: Uuid) -> Result<()> {
        REDIS
            .pipe(|p| p.set(&self.rediskey.to_string(), new.to_string()))
            .await?;
        Ok(())
    }

    pub async fn get_current<'a>(&'a self) -> Result<&'a FSMState> {
        let current: String = REDIS.pipe(|p| p.get(&self.rediskey.to_string())).await?;
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

pub(crate) async fn get_or_create_conversation<F>(
    message: &Message,
    create: F,
) -> Result<Conversation>
where
    F: FnOnce(&Message) -> Result<Conversation>,
{
    let key = get_conversation_key_message(&message)?;
    let conversation: Option<RedisStr> = REDIS
        .query(|mut c| async move {
            if c.exists(&key).await? {
                let conv: RedisStr = c.get(&key).await?;
                Ok(Some(conv))
            } else {
                Ok(None)
            }
        })
        .await?;

    let conversation = if let Some(rstr) = conversation {
        rstr.get::<Conversation>()?
    } else {
        create(&message)?
    };
    Ok(conversation)
}

impl Dialog {
    pub fn new(chat: &Chat) -> Self {
        Dialog {
            chat_id: chat.id(),
            last_activity: Utc::now(),
        }
    }
}
