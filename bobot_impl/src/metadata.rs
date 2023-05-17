//! macros for modules to register themselves with the bot
//! modules are registered with a name, description, and command list

use std::collections::HashMap;

lazy_static! {
    pub static ref NEWLINE: Regex = Regex::new(r#"\s\s+\n\s*"#).unwrap();
}

/// Macro for registering a module. Generates a metadata getter out of a name, description, and
/// command list
macro_rules! metadata {
    ($name:expr, $description:expr) => {
        pub const METADATA: ::once_cell::sync::Lazy<crate::metadata::Metadata> =
            ::once_cell::sync::Lazy::new(|| crate::metadata::Metadata {
                name: $name.into(),
                description: $description.into(),
                commands: ::std::collections::HashMap::new(),
            });
    };
    ($name:expr, $description:expr,
        $( { command = $command:expr, help = $help:expr } ),*) => {
        pub const METADATA: ::once_cell::sync::Lazy<crate::metadata::Metadata> =
            ::once_cell::sync::Lazy::new(|| {

                let description = crate::metadata::NEWLINE.replace_all($description, "\\n");
                let description = description.trim_start().lines().map(|v| v.trim_start()).collect::<Vec<&str>>().join(" ");

                let description: String =  description.replace("\\n", "\n").into();

                let mut c = crate::metadata::Metadata {
                    name: $name.into(),
                    description,
                    commands: ::std::collections::HashMap::new(),
                };
                $(c.commands.insert($command.into(), $help.into());)+
                c
            });
    };
}
use lazy_static::lazy_static;
pub(crate) use metadata;
use regex::Regex;

/// metadata for a single module
#[derive(Clone)]
pub struct Metadata {
    pub name: String,
    pub description: String,
    pub commands: HashMap<String, String>,
}
