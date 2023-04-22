use std::{borrow::Cow, collections::VecDeque};

use crate::util::{
    error::{BotError, Result},
    string::{get_chat_lang, Lang},
};
use botapi::gen_types::{Chat, Message, MessageEntity, UpdateExt, User};
use lazy_static::lazy_static;
use regex::Regex;

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

pub fn get_input_type<'a>(
    message: &'a Message,
    textargs: &'a TextArgs<'a>,
    name: &'a str,
    end: usize,
) -> InputType<'a> {
    log::info!("get:{}", textargs.text);
    if let Some(reply) = message.get_reply_to_message_ref() {
        InputType::Reply(name, reply.get_text_ref(), reply)
    } else {
        let tail = &textargs.text[end..];
        InputType::Command(name, Some(tail), message)
    }
}

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

pub type Entities<'a> = VecDeque<EntityArg<'a>>;
pub type Args<'a> = Vec<TextArg<'a>>;

pub struct TextArgs<'a> {
    pub text: &'a str,
    pub args: Args<'a>,
}

pub struct ArgSlice<'a> {
    pub text: &'a str,
    pub args: &'a [TextArg<'a>],
}

pub enum TextArg<'a> {
    Arg(&'a str),
    Quote(&'a str),
}

impl<'a> TextArg<'a> {
    pub fn get_text(&self) -> &'a str {
        match self {
            TextArg::Arg(s) => s,
            TextArg::Quote(q) => q,
        }
    }
}

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
    pub fn as_slice(&'a self) -> ArgSlice<'a> {
        ArgSlice {
            text: self.text,
            args: self.args.as_slice(),
        }
    }
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

pub fn single_arg<'a>(s: &'a str) -> Option<(TextArg<'a>, usize, usize)> {
    ARGS.find(s).map(|v| {
        if QUOTE.is_match(v.as_str()) {
            (TextArg::Quote(v.as_str()), v.start(), v.end())
        } else {
            (TextArg::Arg(v.as_str()), v.start(), v.end())
        }
    })
}

pub struct Command<'a> {
    pub cmd: &'a str,
    pub args: TextArgs<'a>,
    pub entities: Entities<'a>,
}

pub struct Context<'a> {
    pub message: Option<&'a Message>,
    pub command: Option<Command<'a>>,
    pub chat: &'a Chat,
    pub lang: Lang,
}

impl<'a> Context<'a> {
    pub async fn get_context(update: &'a UpdateExt) -> Result<Option<Context<'a>>> {
        let message = match update {
            UpdateExt::Message(message) => Some(message),
            _ => None,
        };

        let command = message.map(|m| parse_cmd_struct(&m)).flatten();
        let chat = match update {
            UpdateExt::Message(m) => Some(m.get_chat_ref()),
            UpdateExt::EditedMessage(m) => Some(m.get_chat_ref()),
            UpdateExt::CallbackQuery(m) => m.get_message_ref().map(|m| m.get_chat_ref()),
            UpdateExt::ChatMember(m) => Some(m.get_chat_ref()),
            _ => None,
        };

        if let Some(chat) = chat {
            let lang = get_chat_lang(chat.get_id()).await?;
            Ok(Some(Self {
                message,
                command,
                chat,
                lang,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn cmd(
        &'a self,
    ) -> Option<(
        &'a str,
        &'a Entities<'a>,
        &'a TextArgs<'a>,
        &'a Message,
        &'a Lang,
    )> {
        if let (Some(message), Some(command)) = (self.message, &self.command) {
            Some((
                command.cmd,
                &command.entities,
                &command.args,
                message,
                &self.lang,
            ))
        } else {
            None
        }
    }
}

pub fn parse_cmd_struct<'a>(message: &'a Message) -> Option<Command<'a>> {
    parse_cmd(message).map(|(cmd, args, entities)| Command {
        cmd,
        args,
        entities,
    })
}

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
