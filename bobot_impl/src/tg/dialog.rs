use std::collections::HashMap;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use grammers_client::types::Chat;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::util::error::BotError;

use crate::persist::Result;
#[cfg(test)]
mod tests {
    use crate::tg::dialog::ConversationData;

    use super::Conversation;

    #[test]
    fn conversation_transition() {
        let trans = "I eated";
        let mut data = ConversationData::new("fmef".to_string(), "fweef".to_string(), 0).unwrap();
        let fweef = data.add_state("fweef");
        let start = data.get_start().unwrap().state_id;
        data.add_transition(start, fweef, trans.clone());
        let conversation = data.build();
        let new = conversation.transition(trans).unwrap();
        assert_eq!(new.get_current().unwrap().state_id, fweef);
    }

    #[test]
    fn conversation_serde() {
        let trans = "I eated";
        let mut data = ConversationData::new("fmef".to_string(), "fweef".to_string(), 0).unwrap();
        let fweef = data.add_state("fweef");
        let start = data.get_start().unwrap().state_id;
        data.add_transition(start, fweef, trans.clone());
        let conversation = data.build();
        let json = serde_json::to_string(&conversation).unwrap();
        let newconversation: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(
            conversation.get_current().unwrap().state_id,
            newconversation.get_current().unwrap().state_id
        )
    }
}

pub const TYPE_DIALOG: &str = "DialogDb";

#[derive(Serialize, Deserialize)]
pub struct ConversationData {
    pub conversation_id: Uuid,
    pub triggerphrase: String,
    pub chat: Option<i64>,
    pub states: HashMap<Uuid, FSMState>,
    start: Uuid,
    pub transitions: HashMap<String, FSMTransition>,
}

#[derive(Serialize, Deserialize)]
pub struct Conversation {
    pub data: Arc<ConversationData>,
    current: Uuid,
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

impl Into<Conversation> for ConversationData {
    fn into(self) -> Conversation {
        let current = self.start;
        Conversation {
            data: Arc::new(self),
            current,
        }
    }
}

impl ConversationData {
    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
        if let Some(start) = self.states.get(&self.start) {
            Ok(start)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

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

    pub fn new(triggerphrase: String, reply: String, chat: i64) -> Result<Self> {
        ConversationData::new_option(triggerphrase, reply, Some(chat))
    }

    pub fn new_anonymous(triggerphrase: String, reply: String) -> Result<Self> {
        ConversationData::new_option(triggerphrase, reply, None)
    }

    fn new_option(triggerphrase: String, reply: String, chat: Option<i64>) -> Result<Self> {
        let conversation_id = Uuid::new_v4();
        let startstate = FSMState::new(conversation_id, true, reply);
        let mut states = HashMap::<Uuid, FSMState>::new();
        let start = startstate.state_id;
        states.insert(startstate.state_id, startstate);
        let data = ConversationData {
            conversation_id,
            triggerphrase,
            chat,
            states,
            start,
            transitions: HashMap::<String, FSMTransition>::new(),
        };

        Ok(data)
    }

    pub fn build(self) -> Conversation {
        let current = self.start;
        Conversation {
            data: Arc::new(self),
            current,
        }
    }
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
    pub fn get_start<'a>(&'a self) -> Result<&'a FSMState> {
        if let Some(start) = self.data.states.get(&self.data.start) {
            Ok(start)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub fn transition<S>(&self, next: S) -> Result<Self>
    where
        S: Into<String>,
    {
        let current = if let Some(next) = self.data.transitions.get(&next.into()) {
            if let Some(next) = self.data.states.get(&next.end_state) {
                Ok(next.state_id)
            } else {
                Err(BotError::new("invalid choice"))
            }
        } else {
            Err(BotError::new("invalid choice"))
        }?;
        let conversation = Conversation {
            data: self.data.clone(),
            current,
        };
        Ok(conversation)
    }

    pub fn get_current<'a>(&'a self) -> Result<&'a FSMState> {
        if let Some(current) = self.data.states.get(&self.current) {
            Ok(current)
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub fn get_current_text(&self) -> Result<String> {
        if let Some(current) = self.data.states.get(&self.current) {
            Ok(current.content.to_string())
        } else {
            Err(anyhow!(BotError::new("corrupt graph")))
        }
    }

    pub fn reset(self) -> Conversation {
        Conversation {
            data: self.data.clone(),
            current: self.data.start,
        }
    }
}

impl Dialog {
    pub fn new(chat: &Chat) -> Self {
        Dialog {
            chat_id: chat.id(),
            last_activity: Utc::now(),
        }
    }
}

impl Clone for Conversation {
    fn clone(&self) -> Self {
        Conversation {
            data: self.data.clone(),
            current: self.current,
        }
    }
}
