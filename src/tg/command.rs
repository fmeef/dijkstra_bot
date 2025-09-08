//! Utilities exposing a unified interface for parsing slash commands and their arguments
//!
//! Commands can be either a normal telegram slash command, or a command preceeded with a
//! different character, currently "!". Command arguments are parsed using regex currently
//! but in the near future will be switched to a context-free grammar

use crate::statics::{AT_HANDLE, USERNAME};
use crate::util::error::Fail;
use crate::util::string::AlignCharBoundry;
use crate::{
    persist::redis::RedisStr,
    statics::{CONFIG, REDIS},
    util::{
        error::{BotError, Result},
        string::{get_chat_lang, Lang, Speak},
    },
};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use botapi::gen_types::{
    Chat, MaybeInaccessibleMessage, Message, MessageBuilder, MessageEntity, UpdateExt, User,
};
use lazy_static::lazy_static;
use macros::lang_fmt;
use redis::AsyncCommands;
use regex::Regex;
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;
use yoke::{Yoke, Yokeable};

use super::admin_helpers::is_dm;
use super::{
    admin_helpers::{ChatUser, IntoChatUser, UpdateHelpers},
    button::get_url,
    markdown::EntityMessage,
    permissions::{BotPermissions, IsGroupAdmin, NamedBotPermissions, NamedPermission},
};

lazy_static! {
    static ref COMMOND: Regex = Regex::new(&format!(r#"^(!|/)\w+(@{})?\s+.*"#, *USERNAME)).unwrap();
    static ref COMMOND_HEAD: Regex =
        Regex::new(&format!(r#"^(!|/)\w+(@{}|\s|$)"#, *USERNAME)).unwrap();
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
    if let Some(reply) = message.get_reply_to_message() {
        InputType::Reply(name, reply.get_text(), reply)
    } else {
        let end = textargs.text.align_char_boundry(end);
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
            message.chat.id,
            Some(message.message_id),
        )),
    }
}

/// type alias for MessageEntities in a message containing a command
pub type Entities<'a> = VecDeque<EntityArg<'a>>;

/// type alias for parsed argument list of a command
pub type Args<'a> = Vec<TextArg<'a>>;

pub type OwnedArgs = Vec<OwnedTextArg>;

/// Contains references to both the unparsed text of a command (not including the /command)
/// and the same text parsed into and argument list for convienience
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub struct TextArgs<'a> {
    pub text: &'a str,
    pub args: Args<'a>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub struct OwnedTextArgs {
    pub text: String,
    pub args: OwnedArgs,
}

impl OwnedTextArgs {
    pub fn get_ref(&self) -> TextArgs<'_> {
        let mut args = TextArgs {
            text: &self.text,
            args: Vec::with_capacity(self.args.len()),
        };
        for a in self.args.as_slice() {
            args.args.push(a.get_ref());
        }
        args
    }
}

/// A single argument, could be either raw text separated by whitespace or a quoted
/// text block
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum OwnedTextArg {
    Arg(String),
    Quote(String),
}

impl OwnedTextArg {
    pub fn get_ref(&'_ self) -> TextArg<'_> {
        match self {
            Self::Arg(a) => TextArg::Arg(a),
            Self::Quote(a) => TextArg::Quote(a),
        }
    }
}

/// A ranged slice of an argument list. Useful for recursively deconstructing commands
/// or implementing subcommands
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ArgSlice<'a> {
    pub text: &'a str,
    pub args: &'a [TextArg<'a>],
}

/// A single argument, could be either raw text separated by whitespace or a quoted
/// text block
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum TextArg<'a> {
    Arg(&'a str),
    Quote(&'a str),
}

impl<'a> TextArg<'a> {
    fn r(&self) -> TextArg<'a> {
        match self {
            TextArg::Arg(a) => TextArg::Arg(a),
            TextArg::Quote(q) => TextArg::Quote(q),
        }
    }
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EntityArg<'a> {
    Command(&'a str),
    Quote(&'a str),
    Hashtag(&'a str),
    Mention(&'a str),
    TextMention(&'a User),
    TextLink(&'a str),
    Url(&'a str),
}

pub trait PopSlice<'a, 'b> {
    /// remove the first argument in an argument list as a slice
    fn pop_slice(&'a self) -> Option<(TextArg<'b>, ArgSlice<'b>)>;
    fn pop_slice_tail(&'a self) -> Option<ArgSlice<'b>> {
        if let Some((_, slice)) = self.pop_slice() {
            Some(slice)
        } else {
            None
        }
    }
}

impl<'a> TextArgs<'a> {
    /// Convert and argument list to a slice of equal size
    pub fn as_slice(&'a self) -> ArgSlice<'a> {
        ArgSlice {
            text: self.text,
            args: self.args.as_slice(),
        }
    }
}

impl<'a, 'b> PopSlice<'b, 'a> for TextArgs<'a>
where
    'b: 'a,
{
    fn pop_slice(&'b self) -> Option<(TextArg<'a>, ArgSlice<'a>)> {
        if let Some(arg) = self.args.first() {
            let text = match arg {
                TextArg::Arg(arg) => self.text[arg.len()..].trim(),
                TextArg::Quote(arg) => {
                    self.text[self.text.align_char_boundry(arg.len() + 2)..].trim()
                }
            };
            let res = ArgSlice {
                text,
                args: &self.args.as_slice()[1..],
            };
            Some((arg.r(), res))
        } else {
            None
        }
    }
}

impl<'a, 'b> PopSlice<'b, 'a> for ArgSlice<'a> {
    /// remove the first argument in an argument list as a slice
    fn pop_slice(&'b self) -> Option<(TextArg<'a>, ArgSlice<'a>)> {
        if let Some(arg) = self.args.first() {
            let res = ArgSlice {
                text: self.text[arg.get_text().len()..].trim(),
                args: &self.args[1..],
            };
            Some((arg.r(), res))
        } else {
            None
        }
    }
}

fn get_arg_type<'a>(message: &'a Message, entity: &'a MessageEntity) -> Option<EntityArg<'a>> {
    if let Some(text) = message.get_text().map_or(message.get_caption(), Some) {
        let start = entity.get_offset() as usize;
        let end = start + entity.get_length() as usize;
        let end = text.align_char_boundry(end);
        let start = text.align_char_boundry(start);
        let text = &text[start..end];
        match entity.get_tg_type() {
            "hashtag" => Some(EntityArg::Hashtag(text)),
            "mention" => Some(EntityArg::Mention(&text[text.align_char_boundry(1)..])), //do not include @ in mention
            "url" => Some(EntityArg::Url(text)),
            "text_mention" => entity.get_user().map(EntityArg::TextMention),
            "text_link" => entity.get_url().map(EntityArg::TextLink),
            _ => None,
        }
    } else {
        None
    }
}

/// Parse a single argument manually. Useful for when you don't need the full text of a command
pub fn single_arg(s: &str) -> Option<(TextArg<'_>, usize, usize)> {
    ARGS.find(s).map(|v| {
        if QUOTE.is_match(v.as_str()) {
            (
                TextArg::Quote(v.as_str().trim_matches(&['\"'])),
                v.start(),
                v.end(),
            )
        } else {
            (TextArg::Arg(v.as_str()), v.start(), v.end())
        }
    })
}

/// A full command including the /command or !command, the argument list, and any
/// MessageEntities
#[derive(Clone)]
pub struct Cmd<'a> {
    pub cmd: &'a str,
    pub args: TextArgs<'a>,
    pub entities: Entities<'a>,
    pub message: &'a Message,
    pub lang: &'a Lang,
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
    pub command: Option<Cmd<'a>>,
    pub chat: &'a Chat,
    pub lang: &'a Lang,
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Self(Yoke::clone(&self.0))
    }
}

impl StaticContext {
    pub(crate) fn yoke(self: Arc<Self>) -> Context {
        let v = Yoke::attach_to_cart(self, |v| {
            (
                v,
                v.chat().map(|chat| ContextYoke {
                    update: v.update(),
                    chat,
                    lang: v.lang(),
                    command: v.parse_cmd_struct(),
                }),
            )
        });
        Context(v)
    }

    /// Parse a command from a message. Returns none if the message isn't a /command or !command
    pub fn parse_cmd_struct(&self) -> Option<Cmd<'_>> {
        self.parse_cmd().map(|(cmd, args, entities)| Cmd {
            cmd,
            args,
            entities,
            message: self.message().unwrap(), //note this is safe trust me
            lang: &self.lang,
        })
    }

    /// Parse individual components of a /command or !command
    pub fn parse_cmd(&self) -> Option<(&'_ str, TextArgs<'_>, Entities<'_>)> {
        if let Ok(message) = self.message() {
            if let Some(cmd) = message
                .get_text()
                .map_or_else(|| message.get_caption(), Some)
            {
                log::info!("cmd {}", cmd);
                if let Some(head) = COMMOND_HEAD.find(cmd) {
                    let entities = if let Some(entities) = message.get_entities() {
                        let mut entities = entities
                            .iter()
                            .filter(|p| {
                                matches!(
                                    p.get_tg_type(),
                                    "hashtag" | "mention" | "url" | "text_mention" | "text_link"
                                )
                            })
                            .collect::<Vec<&MessageEntity>>();
                        entities.sort_by_key(|n| n.get_offset());
                        entities
                    } else {
                        vec![]
                    };
                    let tail = &cmd[head.end()..].trim_start();

                    let args = entities.iter().filter_map(|v| get_arg_type(message, v));

                    let raw_args = ARGS
                        .find_iter(tail)
                        .map(|v| {
                            if let Some(m) = QUOTE.find(v.as_str()) {
                                TextArg::Quote(m.as_str().trim_matches(&['\"']))
                            } else {
                                TextArg::Arg(v.as_str())
                            }
                        })
                        .collect();
                    let mut cb = 1;
                    cb = head.as_str().align_char_boundry(cb);

                    Some((
                        (head.as_str()[cb..head.end()]
                            .trim_end()
                            .trim_end_matches(&*AT_HANDLE)),
                        TextArgs {
                            text: tail,
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
        } else {
            None
        }
    }

    pub fn chat_ok(&self) -> Result<&'_ Chat> {
        let c = self
            .chat()
            .ok_or_else(|| BotError::Generic("no chat".to_owned()))?;
        Ok(c)
    }

    pub fn message(&self) -> Result<&'_ Message> {
        if let UpdateExt::Message(ref message) = self.update {
            Ok(message)
        } else {
            Err(BotError::Generic("update is not a message".to_owned()))
        }
    }

    pub fn update(&self) -> &'_ UpdateExt {
        &self.update
    }

    pub fn lang(&self) -> &'_ Lang {
        &self.lang
    }

    pub fn chat(&self) -> Option<&'_ Chat> {
        match self.update {
            UpdateExt::Message(ref m) => Some(m.get_chat()),
            UpdateExt::EditedMessage(ref m) => Some(m.get_chat()),
            UpdateExt::CallbackQuery(ref m) => m.get_message().map(|m| match m {
                MaybeInaccessibleMessage::Message(m) => m.get_chat(),
                MaybeInaccessibleMessage::InaccessibleMessage(m) => m.get_chat(),
            }),
            UpdateExt::ChatMember(ref m) => Some(m.get_chat()),
            _ => None,
        }
    }

    pub fn chatuser<'a>(&'a self) -> Option<ChatUser<'a>> {
        match self.update {
            UpdateExt::Message(ref m) => m.get_chatuser(),
            UpdateExt::EditedMessage(ref m) => m.get_chatuser(),
            UpdateExt::CallbackQuery(ref m) => m.get_message().and_then(|m| match m {
                MaybeInaccessibleMessage::Message(m) => m.get_chatuser(),
                MaybeInaccessibleMessage::InaccessibleMessage(_) => None,
            }),
            UpdateExt::ChatMember(ref m) => Some(ChatUser {
                chat: m.get_chat(),
                user: m.get_from(),
            }),
            _ => None,
        }
    }

    /// Get a context from an update. Returns none if one or more fields aren't present
    /// Currently only Message updates return Some
    pub async fn get_context(update: UpdateExt) -> Result<Arc<Self>> {
        let lang = if let Some(chat) = match update {
            UpdateExt::Message(ref m) => Some(m.chat.id),
            UpdateExt::EditedMessage(ref m) => Some(m.chat.id),
            UpdateExt::CallbackQuery(ref m) => m.get_message().map(|m| {
                match m {
                    MaybeInaccessibleMessage::Message(m) => m.get_chat(),
                    MaybeInaccessibleMessage::InaccessibleMessage(m) => m.get_chat(),
                }
                .id
            }),
            UpdateExt::ChatMember(ref m) => Some(m.chat.id),
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
    pub fn is_supergroup_or_die(&self) -> Result<()> {
        if let Some(chat) = self.chat() {
            match chat.get_tg_type() {
                "private" => self.fail(lang_fmt!(self, "baddm")),
                "group" => self.fail(lang_fmt!(self, "notsupergroup")),
                _ => Ok(()),
            }
        } else {
            self.fail(lang_fmt!(self, "notsupergroup"))
        }
    }

    pub fn is_dm(&self) -> bool {
        self.chat().map(is_dm).unwrap_or(false)
    }
    pub fn update(&self) -> &'_ UpdateExt {
        &self.0.get().0.update
    }
    pub fn get(&self) -> &'_ Option<ContextYoke<'_>> {
        &self.0.get().1
    }

    pub fn get_static(&self) -> &'_ StaticContext {
        self.0.get().0
    }

    pub fn try_get(&self) -> Result<&'_ ContextYoke<'_>> {
        self.get()
            .as_ref()
            .ok_or_else(|| BotError::Generic("Not a chat context".to_owned()))
    }

    pub fn get_real_from(&self) -> Result<&'_ User> {
        let message = self.message()?;
        if message.get_sender_chat().is_some() {
            return self.fail(lang_fmt!(self, "anonchannelbad"));
        }
        if let Some(ref user) = message.from {
            Ok(user)
        } else {
            self.fail(lang_fmt!(self, "nosender"))
        }
    }

    pub fn chat(&self) -> Option<&'_ Chat> {
        match self.get().as_ref().map(|v| v.update) {
            Some(UpdateExt::Message(ref m)) => Some(m.get_chat()),
            Some(UpdateExt::EditedMessage(ref m)) => Some(m.get_chat()),
            Some(UpdateExt::CallbackQuery(ref m)) => m.get_message().map(|m| match m {
                MaybeInaccessibleMessage::Message(m) => m.get_chat(),
                MaybeInaccessibleMessage::InaccessibleMessage(m) => m.get_chat(),
            }),
            Some(UpdateExt::ChatMember(ref m)) => Some(m.get_chat()),
            _ => None,
        }
    }

    pub fn message(&self) -> Result<&'_ Message> {
        if let Some(UpdateExt::Message(ref message)) = self.get().as_ref().map(|v| v.update) {
            Ok(message)
        } else {
            Err(BotError::Generic("update is not a message".to_owned()))
        }
    }

    /// Makes accessing command related fields more ergonomic
    pub fn cmd(&self) -> Option<&'_ Cmd<'_>> {
        self.get().as_ref().and_then(|v| v.command.as_ref())
    }
}

#[async_trait]
impl IsGroupAdmin for Context {
    /// If the user is not admin or the group is not a supergroup, return a printable error
    async fn legacy_check_permissions(&self) -> Result<()> {
        self.message()?.legacy_check_permissions().await
    }

    /// return true if the group is a supergroup and the user is an admin
    async fn is_group_admin(&self) -> Result<bool> {
        self.message()?.is_group_admin().await
    }

    /// get the permissions for a user
    async fn get_permissions(&self) -> Result<BotPermissions> {
        self.message()?.get_permissions().await
    }

    /// Apply the mapper function to the permissions, if it returns false NamedPermissions,
    /// return with error
    async fn check_permissions<F>(&self, func: F) -> Result<()>
    where
        F: Fn(NamedBotPermissions) -> NamedPermission + Send,
    {
        self.message()?.check_permissions(func).await
    }
}

impl Context {
    /// Parse a command from a message. Returns none if the message isn't a /command or !command
    pub fn parse_cmd_struct(&self) -> Option<Cmd<'_>> {
        self.get_static().parse_cmd_struct()
    }

    /// Parse individual components of a /command or !command
    pub fn parse_cmd(&self) -> Option<(&'_ str, TextArgs<'_>, Entities<'_>)> {
        self.get_static().parse_cmd()
    }

    pub fn lang(&self) -> &'_ Lang {
        &self.get_static().lang
    }
}

#[async_trait]
impl Speak for Context {
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        self.message()?.speak(message).await
    }

    async fn speak_fmt(&self, messsage: EntityMessage) -> Result<Option<Message>> {
        self.message()?.speak_fmt(messsage).await
    }

    async fn reply_fmt(&self, messsage: EntityMessage) -> Result<Option<Message>> {
        self.message()?.reply_fmt(messsage).await
    }

    async fn reply<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        self.message()?.reply(message).await
    }

    async fn force_reply<T>(&self, message: T, reply: i64) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        self.message()?.force_reply(message, reply).await
    }
}

#[async_trait]
impl UpdateHelpers for Context {
    fn user_event(&self) -> Option<super::admin_helpers::UserChanged<'_>> {
        self.update().user_event()
    }

    async fn should_moderate(&self) -> Option<&'_ Message> {
        self.update().should_moderate().await
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
    let _: () = REDIS
        .pipe(|q| q.set(&key, ser).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    let bs = general_purpose::URL_SAFE_NO_PAD.encode(r.into_bytes());
    let bs = get_url(bs)?;
    log::info!("post_deep_link {}", bs);
    Ok(bs)
}

pub async fn handle_deep_link<F, R>(ctx: &Context, key_func: F) -> Result<Option<R>>
where
    F: FnOnce(&str) -> String,
    R: DeserializeOwned,
{
    if let Some(Cmd { ref args, .. }) = ctx.cmd() {
        if let Some(u) = args.args.first().map(|a| a.get_text()) {
            if let Ok(base) = general_purpose::URL_SAFE_NO_PAD.decode(u) {
                if let Ok(base) = Uuid::from_slice(base.as_slice()) {
                    let key = key_func(&base.to_string());
                    let base: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
                    if let Some(base) = base {
                        return Ok(Some(base.get()?));
                    }
                }
            }
        }
    }
    Ok(None)
}

#[allow(dead_code)]
mod test {
    use botapi::gen_types::UserBuilder;

    use crate::statics::{Args, Config, ARGS, CONFIG_BACKEND, ME};

    use super::*;

    fn default_message(text: String) -> Result<Message> {
        let chat = Chat::default();
        let message =
            MessageBuilder::new(1000, SystemTime::now().elapsed()?.as_secs() as i64, chat)
                .set_text(text)
                .build();

        Ok(message)
    }

    fn default_context(text: String) -> Result<Context> {
        let message = default_message(text)?;
        ARGS.set(Args::default()).ok();
        CONFIG_BACKEND.set(Config::default()).ok();
        ME.set(
            UserBuilder::new(0, true, "test".to_owned())
                .set_username("testbot".to_owned())
                .build(),
        )
        .ok();
        let ctx = StaticContext {
            update: UpdateExt::Message(message),
            lang: Lang::En,
        };
        let ctx = Arc::new(ctx);
        Ok(ctx.yoke())
    }

    #[tokio::test]
    async fn pop_slice() {
        let ctx = default_context("/This command rocks".to_owned()).unwrap();

        let (cmd, textargs, _) = ctx.parse_cmd().unwrap();
        assert_eq!(cmd, "This");

        let (arg, args) = textargs.pop_slice().unwrap();
        assert_eq!(arg.get_text(), "command");
        assert_eq!(args.text, "rocks");
    }

    #[tokio::test]
    async fn pop_slice_emoji() {
        let ctx = default_context("/This 🧋com🧋mand🧋 🧋ro🧋cks🧋".to_owned()).unwrap();

        let (cmd, textargs, _) = ctx.parse_cmd().unwrap();
        assert_eq!(cmd, "This");

        let (arg, args) = textargs.pop_slice().unwrap();
        assert_eq!(arg.get_text(), "🧋com🧋mand🧋");
        assert_eq!(args.text, "🧋ro🧋cks🧋");
    }

    #[tokio::test]
    async fn pop_slice_quote() {
        let ctx = default_context("/This \"command rocks\"".to_owned()).unwrap();

        let (cmd, textargs, _) = ctx.parse_cmd().unwrap();
        assert_eq!(cmd, "This");

        let (arg, _) = textargs.pop_slice().unwrap();
        if let TextArg::Quote(quote) = arg {
            assert_eq!(arg.get_text(), "command rocks");
            assert_eq!(quote, "command rocks");
        } else {
            panic!("not a quote");
        }
    }

    #[tokio::test]
    async fn pop_slice_emoji_quote() {
        let ctx = default_context("/This \"🧋com🧋mand🧋 🧋ro🧋cks🧋\"".to_owned()).unwrap();

        let (cmd, textargs, _) = ctx.parse_cmd().unwrap();
        assert_eq!(cmd, "This");

        println!("{:?}", textargs);
        let (arg, _) = textargs.pop_slice().unwrap();
        if let TextArg::Quote(quote) = arg {
            assert_eq!(arg.get_text(), "🧋com🧋mand🧋 🧋ro🧋cks🧋");

            assert_eq!(quote, "🧋com🧋mand🧋 🧋ro🧋cks🧋");
        } else {
            panic!("not a quote");
        }
    }

    async fn command_emoji() {
        let ctx = default_context("/😍🧋".to_owned()).unwrap();

        let cmd = ctx.parse_cmd();
        assert_eq!(cmd, None);

        let ctx = default_context("/fm😍🧋".to_owned()).unwrap();

        let cmd = ctx.parse_cmd();
        assert_eq!(cmd, None);

        let ctx = default_context("/😍🧋fm".to_owned()).unwrap();

        let cmd = ctx.parse_cmd();
        assert_eq!(cmd, None);
    }
}
