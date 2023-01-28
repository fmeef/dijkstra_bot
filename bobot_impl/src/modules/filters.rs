use crate::tg::command::Command;
use crate::tg::command::TextArg;
use crate::tg::command::TextArgs;
use crate::util::error::BotError;
use crate::util::error::Result;
use crate::util::string::Speak;
use crate::{metadata::metadata, util::string::should_ignore_chat};
use botapi::gen_types::{Message, UpdateExt};
use lazy_static::lazy_static;
use macros::rlformat;
use pomelo::pomelo;
use regex::Regex;
use sea_orm_migration::{MigrationName, MigrationTrait};
metadata!("Filters",
    { command = "filter", help = "<trigger> <reply>: Trigger a reply when soemone says something" },
    { command = "filters", help = "List all filters" },
    { command = "stop", help = "Stop a filter" }
);

pub enum Header<'a> {
    List(Vec<TextArg<'a>>),
    Arg(TextArg<'a>),
}

pub struct FilterCommond<'a> {
    header: Header<'a>,
    body: Option<String>,
    footer: Option<&'a str>,
}

pomelo! {
    %include {
             use super::{FilterCommond, Header};
             use crate::tg::command::TextArg;
        }
    %error crate::tg::markdown::DefaultParseErr;
    %parser pub struct Parser<'e>{};
    %type input FilterCommond<'e>;
    %token #[derive(Debug)] pub enum Token<'e>{};
    %type quote TextArg<'e>;
    %type word TextArg<'e>;
    %type Whitespace &'e str;
    %type multi Vec<TextArg<'e>>;
    %type list Vec<TextArg<'e>>;
    %type Str &'e str;
    %type footer &'e str;
    %type words String;
    %type header Header<'e>;

    input    ::= header(A) {
        FilterCommond {
            header: A,
            body: None,
            footer: None
        }
    }
    input    ::= header(A) words(W) {
        FilterCommond {
            header: A,
            body: Some(W),
            footer: None
        }
    }
    input    ::= header(A) words(W) footer(F) {
        FilterCommond {
            header: A,
            body: Some(W),
            footer: Some(F)
        }
    }
    footer   ::= LBrace Str(A) Rbrace { A }
    header   ::= multi(V)  { Header::List(V) }
    header   ::= word(S) { Header::Arg(S) }
    word     ::= quote(A) { A }
    word     ::= Str(A) { TextArg::Arg(A) }
    words    ::= Whitespace(S) { S.to_owned() }
    words    ::= word(W) { match W {
        TextArg::Arg(arg) => arg.to_owned(),
        TextArg::Quote(quote) => quote.to_owned()
    } }
    words    ::= words(mut L) Whitespace(S) word(W) {
        let w = match W {
            TextArg::Arg(arg) => arg.to_owned(),
            TextArg::Quote(quote) => quote.to_owned()
        };
        let r = format!("{}{}", w, S);
        L.push_str(&r);
        L
    }

   words    ::= words(mut L) word(W)  Whitespace(S) {
        let w = match W {
            TextArg::Arg(arg) => arg.to_owned(),
            TextArg::Quote(quote) => quote.to_owned()
        };
        let r = format!("{}{}", w, S);
        L.push_str(&r);
        L
    }
    quote    ::= Quote Str(A) Quote { TextArg::Quote(A) }
    multi    ::= LParen list(A) RParen {A }
    list     ::= word(A) { vec![A] }
    list     ::= list(mut L) Comma word(A) { L.push(A); L }
}

use parser::{Parser, Token};

lazy_static! {
    static ref TOKENS: Regex = Regex::new(r#"([\{\}\(\),(\s+)]|[^\{\}\(\),(\s+)]+)"#).unwrap();
}

struct Lexer<'a>(&'a str);

impl<'a> Lexer<'a> {
    fn all_tokens(&'a self) -> impl Iterator<Item = Token<'a>> {
        TOKENS.find_iter(self.0).map(|t| match t.as_str() {
            "(" => Token::LParen,
            ")" => Token::RParen,
            "{" => Token::LBrace,
            "}" => Token::Rbrace,
            "," => Token::Comma,
            s if t.as_str().trim().is_empty() => Token::Whitespace(s),
            s => Token::Str(s),
        })
    }
}

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230127_000001_create_filters"
    }
}

pub mod entities {
    use crate::persist::migrate::ManagerHelper;
    use ::sea_orm_migration::prelude::*;

    #[async_trait::async_trait]
    impl MigrationTrait for super::Migration {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(filters::Entity)
                        .col(
                            ColumnDef::new(filters::Column::Id)
                                .big_integer()
                                .not_null()
                                .unique_key()
                                .auto_increment(),
                        )
                        .col(
                            ColumnDef::new(filters::Column::Chat)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(filters::Column::Text).text())
                        .col(ColumnDef::new(filters::Column::MediaId).text())
                        .col(
                            ColumnDef::new(filters::Column::MediaType)
                                .integer()
                                .not_null(),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(filters::Column::Id)
                                .col(filters::Column::Chat)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(triggers::Entity)
                        .col(ColumnDef::new(triggers::Column::Trigger).text().not_null())
                        .col(
                            ColumnDef::new(triggers::Column::FilterId)
                                .big_integer()
                                .unique_key()
                                .not_null(),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(triggers::Column::Trigger)
                                .col(triggers::Column::FilterId)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("trigger_id_fk")
                        .from(triggers::Entity, triggers::Column::FilterId)
                        .to(filters::Entity, filters::Column::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .await?;
            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .drop_foreign_key(
                    ForeignKey::drop()
                        .table(triggers::Entity)
                        .name("trigger_id_fk")
                        .to_owned(),
                )
                .await?;
            manager.drop_table_auto(filters::Entity).await?;
            manager.drop_table_auto(triggers::Entity).await?;
            Ok(())
        }
    }

    pub mod triggers {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "triggers")]
        pub struct Model {
            #[sea_orm(primary_key, column_type = "Text")]
            pub trigger: String,
            #[sea_orm(primay_key)]
            pub filter_id: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::filters::Entity",
                from = "Column::FilterId",
                to = "super::filters::Column::Id"
            )]
            Filters,
        }
        impl Related<super::filters::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Filters.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod filters {

        use crate::persist::core::media::*;
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "filters")]
        pub struct Model {
            #[sea_orm(primary_key, autoincrement = true)]
            pub id: i64,
            pub chat: i64,
            #[sea_orm(column_type = "Text")]
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: MediaType,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::triggers::Entity")]
            Triggers,
        }
        impl Related<super::filters::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Triggers.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

async fn command_filter<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    let lexer = Lexer(args.text);
    let mut parser = Parser::new();
    for token in lexer.all_tokens() {
        parser
            .parse(token)
            .map_err(|e| BotError::speak(e.to_string(), message.get_chat().get_id()))?;
    }

    let cmd = parser
        .end_of_input()
        .map_err(|e| BotError::speak(e.to_string(), message.get_chat().get_id()))?;

    message
        .get_chat()
        .speak(format!(
            "Parsed filter {}",
            cmd.body.unwrap_or("".to_owned())
        ))
        .await?;
    Ok(())
}

#[allow(dead_code)]
async fn handle_command<'a>(message: &Message, command: Option<&'a Command<'a>>) -> Result<()> {
    if should_ignore_chat(message.get_chat().get_id()).await? {
        return Ok(());
    }
    if let Some(&Command { cmd, ref args, .. }) = command {
        match cmd {
            "filter" => command_filter(message, &args).await?,
            _ => (),
        };
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_update<'a>(update: &UpdateExt, cmd: Option<&'a Command<'a>>) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message, cmd).await?,
        _ => (),
    };
    Ok(())
}

mod test {
    use super::Lexer;

    #[test]
    fn parse_cmd() {
        let cmd = "test test2 {tag}";
        let lexer = Lexer(cmd);
        for token in lexer.all_tokens() {
            println!("token {:?}", token);
        }
    }
}
