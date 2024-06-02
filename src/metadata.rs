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
    let description = [""]
        .into_iter()
        .chain(description.as_ref().trim_start().lines())
        .map(|v| v.trim_start())
        .map(|v| {
            if !v.ends_with(" ") && !v.ends_with("\\n") {
                format!("{} ", v)
            } else {
                format!("{}", v)
            }
        })
        .join("");

    description.replace(r#"\n"#, "\n")
}

/// Macro for registering a module. Generates a metadata getter out of a name, description, and
/// command list
#[macro_export]
macro_rules! metadata {
    ($name:expr, $description:expr) => {
        pub static METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| $crate::metadata::Metadata {
                name: $name.into(),
                priority: None,
                description: $description.into(),
                commands: ::std::collections::HashMap::new(),
                sections: ::std::collections::HashMap::new(),
                state: None
            });
    };

    ($name:expr, $description:expr
        $( , { sub = $sub:expr, content = $content:expr } )*
        $( , { command = $command:expr, help = $help:expr } )*
    ) => {
        #[allow(unused_mut)]
        pub static METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| {
                let description = $crate::metadata::markdownify($description);

                let mut c = $crate::metadata::Metadata {
                    name: $name.into(),
                    priority: None,
                    description,
                    commands: ::std::collections::HashMap::new(),
                    sections: ::std::collections::HashMap::new(),
                    state: None
                };
                $(c.commands.insert($command.into(), $help.into());)*
                $(
                    let content = $crate::metadata::markdownify($content);
                    c.sections.insert(&sub.into(), content.into());
                )*
                c
            });
    };

    ($name:expr, $description:expr, $serialize:expr
        $( , { sub = $sub:expr, content = $content:expr } )*
        $( , { command = $command:expr, help = $help:expr } )*
    ) => {
        #[allow(unused_mut)]
        pub static METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| {
                let description = $crate::metadata::markdownify($description);

                let mut c = $crate::metadata::Metadata {
                    name: $name.into(),
                    priority: None,
                    description,
                    commands: ::std::collections::HashMap::new(),
                    sections: ::std::collections::HashMap::new(),
                    state: Some(::std::sync::Arc::new($serialize))
                };
                $(c.commands.insert($command.into(), $help.into());)*
                $(
                    let content = $crate::metadata::markdownify($content);
                    c.sections.insert($sub.into(), content.into());
                )*
                c
            });

    };
    ($name:expr, $description:expr, $serialize:expr, $priority:expr
        $( , { sub = $sub:expr, content = $content:expr } )*
        $( , { command = $command:expr, help = $help:expr } )*
    ) => {
        #[allow(unused_mut)]
        pub static METADATA: $crate::once_cell::sync::Lazy<$crate::metadata::Metadata> =
            $crate::once_cell::sync::Lazy::new(|| {
                let description = $crate::metadata::markdownify($description);

                let mut c = $crate::metadata::Metadata {
                    name: $name.into(),
                    priority: Some($priority),
                    description,
                    commands: ::std::collections::HashMap::new(),
                    sections: ::std::collections::HashMap::new(),
                    state: Some(::std::sync::Arc::new($serialize))
                };
                $(c.commands.insert($command.into(), $help.into());)*
                $(
                    let content = $crate::metadata::markdownify($content);
                    c.sections.insert($sub.into(), content.into());
                )*
                c
            });
    };
}
use async_trait::async_trait;
use itertools::Itertools;
use lazy_static::lazy_static;
pub use metadata;
use regex::Regex;
use sea_orm_migration::MigrationTrait;

use crate::util::error::Result;

/// metadata for a single module
#[derive(Clone, Debug)]
pub struct Metadata {
    pub name: String,
    pub priority: Option<i32>,
    pub description: String,
    pub commands: HashMap<String, String>,
    pub sections: HashMap<String, String>,
    pub state: Option<Arc<dyn ModuleHelpers + Send + Sync>>,
}

impl Metadata {
    pub fn new(name: String, description: String, priority: Option<i32>) -> Self {
        Self {
            name,
            priority,
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
