use std::{borrow::Cow, collections::VecDeque};

use botapi::gen_types::{Message, MessageEntity, User};
use lazy_static::lazy_static;

use regex::Regex;

lazy_static! {
    static ref COMMOND: Regex = Regex::new(r#"^(!|/)\w+(@\w)?\s+.*"#).unwrap();
    static ref COMMOND_HEAD: Regex = Regex::new(r#"^(!|/)\w+(@\w+)?"#).unwrap();
    static ref TOKENS: Regex = Regex::new(r#"([^\s"!/]+|"|^!|^/)"#).unwrap();
    static ref ARGS: Regex = Regex::new(r#"(".*"|[^"\s]+)"#).unwrap();
    static ref QUOTE: Regex = Regex::new(r#"".*""#).unwrap();
}

pub type Entities<'a> = VecDeque<EntityArg<'a>>;
pub type Args<'a> = VecDeque<TextArg<'a>>;

pub struct TextArgs<'a> {
    pub text: &'a str,
    pub args: Args<'a>,
}

pub enum TextArg<'a> {
    Arg(&'a str),
    Quote(&'a str),
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
            let tail = &cmd[head.end()..];

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
