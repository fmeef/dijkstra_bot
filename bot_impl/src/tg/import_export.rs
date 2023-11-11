use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::statics::ME;

#[derive(Serialize, Deserialize)]
pub struct RoseExport {
    pub bot_id: i64,
    pub data: HashMap<String, serde_json::Value>,
}

impl RoseExport {
    pub fn new() -> Self {
        let bot_id = ME.get().unwrap().get_id();
        Self {
            bot_id,
            data: HashMap::new(),
        }
    }
}
