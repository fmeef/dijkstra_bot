//! macros for modules to register themselves with the bot
//! modules are registered with a name, description, and command list

use std::collections::HashMap;

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
macro_rules! metadata {
    ($name:expr, $description:expr) => {
        pub const METADATA: ::once_cell::sync::Lazy<crate::metadata::Metadata> =
            ::once_cell::sync::Lazy::new(|| crate::metadata::Metadata {
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
        pub const METADATA: ::once_cell::sync::Lazy<crate::metadata::Metadata> =
            ::once_cell::sync::Lazy::new(|| {
                let description = crate::metadata::markdownify($description);

                let mut c = crate::metadata::Metadata {
                    name: $name.into(),
                    description,
                    commands: ::std::collections::HashMap::new(),
                    sections: ::std::collections::HashMap::new()
                };
                $(c.commands.insert($command.into(), $help.into());)*
                $(c.sections.insert($sub.into(), $content.into());)*
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
    pub sections: HashMap<String, String>,
}
