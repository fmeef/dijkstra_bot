//! macros for modules to register themselves with the bot
//! modules are registered with a name, description, and command list

use std::collections::HashMap;
use std::sync::Arc;

lazy_static! {
    pub static ref NEWLINE: Regex = Regex::new(r#"(  +\n[^\S\r\n]*)"#).unwrap();
    pub static ref DOUBLE_NEWLINE: Regex = Regex::new(r#"[^\S\r\n]*\n\n[^\S\r\n]*"#).unwrap();
//    pub static ref WHITESPACE: Regex = Regex::new(r#"[^(\\n)][^\S\r\n]+"#).unwrap();
}

pub fn markdownify<T: AsRef<str>>(description: T) -> String {
    let description = NEWLINE.replace_all(description.as_ref(), r#"\n"#);
    let description = DOUBLE_NEWLINE.replace_all(description.as_ref(), r#"\n\n"#);

    //    let description = description.replace("\n\n", "\\n\\n");
    let len = description.len();
    let v = description
        .trim_start()
        .lines()
        .map(|v| v.trim_start())
        .collect::<Vec<&str>>();
    let mut description = String::with_capacity(len);
    let mut prev = "nil";
    for line in v {
        description.push_str(line);
        if prev.trim_end().len() == prev.len() {
            description.push_str(" ");
        }
        prev = line;
    }
    //let description = WHITESPACE.replace_all(&description, " ");
    description.replace(r#"\n"#, "\n").into()
}

/// Macro for registering a module. Generates a metadata getter out of a name, description, and
/// command list
#[macro_export]
macro_rules! metadata {
    ($name:expr, $description:expr) => {
        pub const METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| $crate::metadata::Metadata {
                name: $name.into(),
                description: $description.into(),
                commands: ::std::collections::HashMap::new(),
                sections: ::std::collections::HashMap::new()
            });
    };

    ($name:expr, $description:expr
        $( , { sub = $sub:expr, content = $content:expr } )*
        $( , { command = $command:expr, help = $help:expr } )*
    ) => {
        pub const METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| {
                let description = $crate::metadata::markdownify($description);

                let mut c = $crate::metadata::Metadata {
                    name: $name.into(),
                    description,
                    commands: ::std::collections::HashMap::new(),
                    sections: ::std::collections::HashMap::new(),
                    state: None
                };
                $(c.commands.insert($command.into(), $help.into());)*
                $(c.sections.insert($sub.into(), $content.into());)*
                c
            });
    };

    ($name:expr, $description:expr, $serialize:expr
        $( , { sub = $sub:expr, content = $content:expr } )*
        $( , { command = $command:expr, help = $help:expr } )*
    ) => {
        pub const METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| {
                let description = $crate::metadata::markdownify($description);

                let mut c = $crate::metadata::Metadata {
                    name: $name.into(),
                    description,
                    commands: ::std::collections::HashMap::new(),
                    sections: ::std::collections::HashMap::new(),
                    state: Some(::std::sync::Arc::new($serialize))
                };
                $(c.commands.insert($command.into(), $help.into());)*
                $(c.sections.insert($sub.into(), $content.into());)*
                c
            });
    };
}
use async_trait::async_trait;
use lazy_static::lazy_static;
pub use metadata;
use regex::Regex;
use sea_orm_migration::MigrationTrait;

use crate::util::error::Result;

/// metadata for a single module
#[derive(Clone, Debug)]
pub struct Metadata {
    pub name: String,
    pub description: String,
    pub commands: HashMap<String, String>,
    pub sections: HashMap<String, String>,
    pub state: Option<Arc<dyn ModuleHelpers + Send + Sync>>,
}

impl Metadata {
    pub fn new(name: String, description: String) -> Self {
        Self {
            name,
            description,
            commands: HashMap::new(),
            sections: HashMap::new(),
            state: None,
        }
    }

    pub fn add_command(mut self, command: String, help: String) -> Self {
        self.commands.insert(command, help);
        self
    }

    pub fn add_section(mut self, sub: String, content: String) -> Self {
        self.sections.insert(sub, content);
        self
    }
}

#[async_trait]
pub trait ModuleHelpers: std::fmt::Debug {
    async fn export(&self, chat: i64) -> Result<Option<serde_json::Value>>;
    async fn import(&self, chat: i64, value: serde_json::Value) -> Result<()>;
    fn supports_export(&self) -> Option<&'static str>;
    fn get_migrations(&self) -> Vec<Box<dyn MigrationTrait>>;
}
