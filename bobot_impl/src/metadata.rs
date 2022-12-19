use std::collections::HashMap;
macro_rules! metadata {
    ($name:expr) => {
        pub(crate) const METADATA: ::once_cell::sync::Lazy<crate::metadata::Metadata> =
            ::once_cell::sync::Lazy::new(|| crate::metadata::Metadata {
                name: $name.into(),
                commands: ::std::collections::HashMap::new(),
            });
    };
    ($name:expr,
        $( { command = $command:expr, help = $help:expr } ),*) => {
        pub(crate) const METADATA: ::once_cell::sync::Lazy<crate::metadata::Metadata> =
            ::once_cell::sync::Lazy::new(|| {
                let mut c = crate::metadata::Metadata {
                    name: $name.into(),
                    commands: ::std::collections::HashMap::new(),
                };
                $(c.commands.insert($command.into(), $help.into());)+
                c
            });
    };
}
pub(crate) use metadata;

#[derive(Clone)]
pub struct Metadata {
    pub name: String,
    pub commands: HashMap<String, String>,
}
