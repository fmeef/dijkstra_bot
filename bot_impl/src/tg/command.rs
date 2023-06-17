//! Utilities exposing a unified interface for parsing slash commands and their arguments
//!
//! Commands can be either a normal telegram slash command, or a command preceeded with a
//! different character, currently "!". Command arguments are parsed using regex currently
//! but in the near future will be switched to a context-free grammar

use crate::{
    persist::redis::RedisStr,
    statics::{CONFIG, REDIS},
    util::{
        error::{BotError, Result},
        string::{get_chat_lang, Lang},
    },
};
use base64::{engine::general_purpose, Engine};
use botapi::gen_types::{Chat, Message, MessageEntity, UpdateExt, User};
use lazy_static::lazy_static;
use redis::AsyncCommands;
use regex::Regex;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::{borrow::Cow, collections::VecDeque};
use uuid::Uuid;
use yoke::{Yoke, Yokeable};

use super::{admin_helpers::UpdateHelpers, button::get_url};

lazy_static! {
    static ref COMMOND: Regex = Regex::new(r#"^(!|/)\w+(@\w)?\s+.*"#).unwrap();
    static ref COMMOND_HEAD: Regex = Regex::new(r#"^(!|/)\w+(@\w+)?"#).unwrap();
    static ref TOKENS: Regex = Regex::new(r#"([^\s"!/]+|"|^!|^/)"#).unwrap();
    static ref ARGS: Regex = Regex::new(r#"(".*"|[^"\s]+)"#).unwrap();
    static ref QUOTE: Regex = Regex::new(r#"".*""#).unwrap();
}

pub enum InputType<'a> {
    Reply(&'a str, Option<&'a str>, &'a Message),
    Command(&'a str, Option<&'a str>, &'a Message),
}

fn get_input_type<'a>(
    message: &'a Message,
    textargs: &'a TextArgs<'a>,
    name: &'a str,
    end: usize,
) -> InputType<'a> {
    if let Some(reply) = message.get_reply_to_message_ref() {
        InputType::Reply(name, reply.get_text_ref(), reply)
    } else {
        let tail = &textargs.text[end..];
        InputType::Command(name, Some(tail), message)
    }
}

/// Helper to parse a command with either the argument to the command as text or
/// the text of the message the command is replying to
pub fn get_content<'a>(
    message: &'a Message,
    textargs: &'a TextArgs<'a>,
) -> crate::util::error::Result<InputType<'a>> {
    match single_arg(textargs.text) {
        Some((TextArg::Arg(name), _, end)) => Ok(get_input_type(message, textargs, name, end)),
        Some((TextArg::Quote(name), _, end)) => Ok(get_input_type(message, textargs, name, end)),
        _ => Err(BotError::speak(
            "Invalid argument, need to specify name",
            message.get_chat().get_id(),
        )),
    }
}

/// type alias for MessageEntities in a message containing a command
pub type Entities<'a> = VecDeque<EntityArg<'a>>;

/// type alias for parsed argument list of a command
pub type Args<'a> = Vec<TextArg<'a>>;

/// Contains references to both the unparsed text of a command (not including the /command)
/// and the same text parsed into and argument list for convienience
#[derive(Clone)]
pub struct TextArgs<'a> {
    pub text: &'a str,
    pub args: Args<'a>,
}

/// A ranged slice of an argument list. Useful for recursively deconstructing commands
/// or implementing subcommands
pub struct ArgSlice<'a> {
    pub text: &'a str,
    pub args: &'a [TextArg<'a>],
}

/// A single argument, could be either raw text separated by whitespace or a quoted
/// text block
#[derive(Clone)]
pub enum TextArg<'a> {
    Arg(&'a str),
    Quote(&'a str),
}

impl<'a> TextArg<'a> {
    /// get the text of a single argument, whether or not it is quoted
    pub fn get_text(&self) -> &'a str {
        match self {
            TextArg::Arg(s) => s,
            TextArg::Quote(q) => q,
        }
    }
}

/// Helper for wrapping supported MessageEntities in arguments without cloning or owning
#[derive(Clone)]
pub enum EntityArg<'a> {
    Command(&'a str),
    Quote(&'a str),
    Hashtag(&'a str),
    Mention(&'a str),
    TextMention(&'a User),
    TextLink(&'a str),
    Url(&'a str),
}

impl<'a> TextArgs<'a> {
    /// Convert and argument list to a slice of equal size
    pub fn as_slice(&'a self) -> ArgSlice<'a> {
        ArgSlice {
            text: self.text,
            args: self.args.as_slice(),
        }
    }

    /// remove the first argument in an argument list as a slice
    pub fn pop_slice(&'a self) -> Option<ArgSlice<'a>> {
        if let Some(arg) = self.args.first() {
            let res = ArgSlice {
                text: &self.text[arg.get_text().len()..],
                args: &self.args.as_slice()[1..],
            };
            Some(res)
        } else {
            None
        }
    }
}

fn get_arg_type<'a>(message: &'a Message, entity: &'a MessageEntity) -> Option<EntityArg<'a>> {
    if let Some(text) = message.get_text_ref() {
        let start = entity.get_offset() as usize;
        let end = start + entity.get_length() as usize;
        let text = &text[start..end];
        match entity.get_tg_type_ref() {
            "hashtag" => Some(EntityArg::Hashtag(text)),
            "mention" => Some(EntityArg::Mention(&text[1..])), //do not include @ in mention
            "url" => Some(EntityArg::Url(text)),
            "text_mention" => entity.get_user_ref().map(|u| EntityArg::TextMention(&u)),
            "text_link" => entity.get_url_ref().map(|u| EntityArg::TextLink(&u)),
            _ => None,
        }
    } else {
        None
    }
}

/// Parse a single argument manually. Useful for when you don't need the full text of a command
pub fn single_arg<'a>(s: &'a str) -> Option<(TextArg<'a>, usize, usize)> {
    ARGS.find(s).map(|v| {
        if QUOTE.is_match(v.as_str()) {
            (TextArg::Quote(v.as_str()), v.start(), v.end())
        } else {
            (TextArg::Arg(v.as_str()), v.start(), v.end())
        }
    })
}

/// A full command including the /command or !command, the argument list, and any
/// MessageEntities
#[derive(Clone)]
pub struct Command<'a> {
    pub cmd: &'a str,
    pub args: TextArgs<'a>,
    pub entities: Entities<'a>,
}

pub struct StaticContext {
    pub update: UpdateExt,
    pub lang: Lang,
}

/// Everything needed to interact with user messages. Contains command and arguments, the message
/// API type itself, the current language, and the chat
pub struct Context(
    Yoke<(&'static StaticContext, Option<ContextYoke<'static>>), Arc<StaticContext>>,
);

#[derive(Yokeable, Clone)]
pub struct ContextYoke<'a> {
    pub update: &'a UpdateExt,
    pub command: Option<Command<'a>>,
    pub chat: &'a Chat,
    pub lang: &'a Lang,
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Self(Yoke::clone(&self.0))
    }
}

impl StaticContext {
    pub fn yoke(self: Arc<Self>) -> Context {
        let v = Yoke::attach_to_cart(self, |v| {
            (
                v,
                if let Some(chat) = v.chat() {
                    Some(ContextYoke {
                        update: v.update(),
                        chat,
                        lang: v.lang(),
                        command: v.parse_cmd(),
                    })
                } else {
                    None
                },
            )
        });
        Context(v)
    }

    pub fn parse_cmd<'a>(&'a self) -> Option<Command<'a>> {
        if let UpdateExt::Message(ref m) = self.update {
            parse_cmd_struct(m)
        } else {
            None
        }
    }

    pub fn chat_ok<'a>(&'a self) -> Result<&'a Chat> {
        let c = self
            .chat()
            .ok_or_else(|| BotError::Generic("no chat".to_owned()))?;
        Ok(c)
    }

    pub fn message<'a>(&'a self) -> Result<&'a Message> {
        if let UpdateExt::Message(ref message) = self.update {
            Ok(message)
        } else {
            Err(BotError::Generic("update is not a message".to_owned()))
        }
    }

    pub fn update<'a>(&'a self) -> &'a UpdateExt {
        &self.update
    }

    pub fn lang<'a>(&'a self) -> &'a Lang {
        &self.lang
    }

    pub fn chat<'a>(&'a self) -> Option<&'a Chat> {
        match self.update {
            UpdateExt::Message(ref m) => Some(m.get_chat_ref()),
            UpdateExt::EditedMessage(ref m) => Some(m.get_chat_ref()),
            UpdateExt::CallbackQuery(ref m) => m.get_message_ref().map(|m| m.get_chat_ref()),
            UpdateExt::ChatMember(ref m) => Some(m.get_chat_ref()),
            _ => None,
        }
    }

    /// Get a context from an update. Returns none if one or more fields aren't present
    /// Currently only Message updates return Some
    pub async fn get_context(update: UpdateExt) -> Result<Arc<Self>> {
        let lang = if let Some(chat) = match update {
            UpdateExt::Message(ref m) => Some(m.get_chat_ref().get_id()),
            UpdateExt::EditedMessage(ref m) => Some(m.get_chat_ref().get_id()),
            UpdateExt::CallbackQuery(ref m) => {
                m.get_message_ref().map(|m| m.get_chat_ref().get_id())
            }
            UpdateExt::ChatMember(ref m) => Some(m.get_chat_ref().get_id()),
            _ => None,
        } {
            get_chat_lang(chat).await?
        } else {
            Lang::En
        };
        Ok(Arc::new(Self { update, lang }))
    }
}

impl Context {
    pub fn update<'a>(&'a self) -> &'a UpdateExt {
        &self.0.get().0.update
    }
    pub fn get<'a>(&'a self) -> &'a Option<ContextYoke<'a>> {
        &self.0.get().1
    }

    pub fn get_static<'a>(&'a self) -> &'a StaticContext {
        &self.0.get().0
    }

    pub fn try_get<'a>(&'a self) -> Result<&'a ContextYoke<'a>> {
        self.get()
            .as_ref()
            .ok_or_else(|| BotError::Generic("Not a chat context".to_owned()))
    }

    pub fn chat<'a>(&'a self) -> Option<&'a Chat> {
        match self.get().as_ref().map(|v| v.update) {
            Some(UpdateExt::Message(ref m)) => Some(m.get_chat_ref()),
            Some(UpdateExt::EditedMessage(ref m)) => Some(m.get_chat_ref()),
            Some(UpdateExt::CallbackQuery(ref m)) => m.get_message_ref().map(|m| m.get_chat_ref()),
            Some(UpdateExt::ChatMember(ref m)) => Some(m.get_chat_ref()),
            _ => None,
        }
    }

    pub fn message<'a>(&'a self) -> Result<&'a Message> {
        if let Some(UpdateExt::Message(ref message)) = self.get().as_ref().map(|v| v.update) {
            Ok(message)
        } else {
            Err(BotError::Generic("update is not a message".to_owned()))
        }
    }

    /// Makes accessing command related fields more ergonomic
    pub fn cmd<'a>(
        &'a self,
    ) -> Option<(
        &'a str,
        &'a Entities<'a>,
        &'a TextArgs<'a>,
        &'a Message,
        &'a Lang,
    )> {
        if let Some(ctx) = self.get() {
            if let (UpdateExt::Message(message), Some(command)) = (ctx.update, &ctx.command) {
                Some((
                    command.cmd,
                    &command.entities,
                    &command.args,
                    &message,
                    &ctx.lang,
                ))
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl UpdateHelpers for Context {
    fn user_event<'a>(&'a self) -> Option<super::admin_helpers::UserChanged<'a>> {
        self.update().user_event()
    }
}

/// Parse a command from a message. Returns none if the message isn't a /command or !command
pub fn parse_cmd_struct<'a>(message: &'a Message) -> Option<Command<'a>> {
    parse_cmd(message).map(|(cmd, args, entities)| Command {
        cmd,
        args,
        entities,
    })
}

/// Parse individual components of a /command or !command
pub fn parse_cmd<'a>(message: &'a Message) -> Option<(&'a str, TextArgs<'a>, Entities<'a>)> {
    if let Some(Cow::Borrowed(cmd)) = message.get_text() {
        if let Some(head) = COMMOND_HEAD.find(&cmd) {
            let entities = if let Some(Cow::Borrowed(entities)) = message.get_entities() {
                let mut entities = entities
                    .iter()
                    .filter(|p| match p.get_tg_type().as_ref() {
                        "hashtag" => true,
                        "mention" => true,
                        "url" => true,
                        "text_mention" => true,
                        "text_link" => true,
                        _ => false,
                    })
                    .collect::<Vec<&MessageEntity>>();
                entities.sort_by(|o, n| n.get_offset().cmp(&o.get_offset()));
                entities
            } else {
                vec![]
            };
            let tail = &cmd[head.end()..].trim_start();

            let args = entities.iter().filter_map(|v| get_arg_type(message, v));

            let raw_args = ARGS
                .find_iter(tail)
                .map(|v| {
                    if QUOTE.is_match(v.as_str()) {
                        TextArg::Quote(v.as_str())
                    } else {
                        TextArg::Arg(v.as_str())
                    }
                })
                .collect();
            Some((
                &head.as_str()[1..head.end()],
                TextArgs {
                    text: &tail,
                    args: raw_args,
                },
                args.collect(),
            ))
        } else {
            None
        }
    } else {
        None
    }
}

pub async fn post_deep_link<T, F>(value: T, key_func: F) -> Result<String>
where
    T: Serialize,
    F: FnOnce(&str) -> String,
{
    let ser = RedisStr::new(&value)?;
    let r = Uuid::new_v4();
    let key = key_func(&r.to_string());
    REDIS
        .pipe(|q| q.set(&key, ser).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    let bs = general_purpose::URL_SAFE_NO_PAD.encode(r.into_bytes());
    let bs = get_url(bs)?;
    Ok(bs)
}

pub async fn handle_deep_link<F, R>(ctx: &Context, key_func: F) -> Result<Option<R>>
where
    F: FnOnce(&str) -> String,
    R: DeserializeOwned,
{
    if let Some((_, _, args, _, _)) = ctx.cmd() {
        if let Some(u) = args.args.first().map(|a| a.get_text()) {
            let base = general_purpose::URL_SAFE_NO_PAD.decode(u)?;
            let base = Uuid::from_slice(base.as_slice())?;
            let key = key_func(&base.to_string());
            let base: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
            if let Some(base) = base {
                return Ok(Some(base.get()?));
            }
        }
    }
    Ok(None)
}
