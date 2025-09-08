//! Various tools for generating formatted telegram text using MessageEntities
//! There are two APIs here, a markdown like "murkdown" formatting language and
//! a builder api for manually generating formatted text

use crate::persist::core::button;
use crate::statics::TG;
use crate::util::error::{BotError, Result};
use crate::util::string::AlignCharBoundry;
use botapi::gen_methods::CallSendMessage;
use botapi::gen_types::{
    Chat, EReplyMarkup, InlineKeyboardButton, InlineKeyboardButtonBuilder, MessageEntity,
    MessageEntityBuilder, User,
};
use futures::future::BoxFuture;
use futures::FutureExt;
use itertools::Itertools;
use lazy_static::lazy_static;

use markdown::mdast::Node;
use markdown::ParseOptions;
use pomelo::pomelo;
use regex::Regex;

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// Custom error type for murkdown parse failure. TODO: add additional context here
#[derive(Debug, Error, Default)]
pub struct DefaultParseErr {}

impl Display for DefaultParseErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("default parse error")?;
        Ok(())
    }
}

/// Data for filter's header
#[derive(Clone, Debug)]
pub enum Header {
    List(Vec<String>),
    Arg(String),
}

/// Complete parsed filter with header, body, and footer
pub struct FilterCommond {
    pub header: Option<Header>,
    pub body: Vec<TgSpan>,
}

/// Type for representing murkdown syntax tree
#[derive(Debug)]
pub enum TgSpan {
    Pre((String, String)),
    Code(String),
    Italic(Vec<TgSpan>),
    Bold(Vec<TgSpan>),
    Strikethrough(Vec<TgSpan>),
    Underline(Vec<TgSpan>),
    Spoiler(Vec<TgSpan>),
    Link(Vec<TgSpan>, String),
    Raw(String),
    Filling(String),
    Button(String, String),
    NewlineButton(String, String),
    NoOp,
}

#[derive(Debug)]
pub enum ParsedArg {
    Arg(String),
    Quote(String),
}

impl ParsedArg {
    /// get the text of a single argument, whether or not it is quoted
    pub fn get_text(self) -> String {
        match self {
            ParsedArg::Arg(s) => s,
            ParsedArg::Quote(q) => q,
        }
    }
}

lazy_static! {
    /// regex for matching whitespace-separated string not containing murkdown reserved characters
    static ref RAWSTR: Regex = Regex::new(r#"([^\s"]+|")"#).unwrap();

    /// static empty vec used for internal optimization
    pub static ref EMPTY_ENTITIES: Vec<MessageEntity> = vec![];
}

// // Pomello parser generator macro call for murkdown context-free grammar
// pomelo! {
//     %error super::DefaultParseErr;
//     %type input Vec<super::TgSpan>;
//     %type words Vec<super::TgSpan>;
//     %type main Vec<super::TgSpan>;
//     %type word super::TgSpan;
//     %type wordraw (super::TgSpan, super::TgSpan);
//     %type RawChar char;
//     %type raw String;

//     input     ::= main(A) { A }

//     main     ::= words?(A) { A.unwrap_or_else(Vec::new) }

//     wordraw  ::= word(W) raw(R) { (W, super::TgSpan::Raw(R)) }

//     words    ::=  words(mut L) word(C) { L.push(C); L }
//     words    ::= words(mut L) wordraw(W) { L.push(W.0); L.push(W.1); L }
//     words    ::= wordraw(W) { vec![W.0, W.1] }
//     words    ::= word(C) { vec![C] }
//     words    ::= raw(R) { vec![super::TgSpan::Raw(R)]}

//     raw       ::= RawChar(C) { C.into() }
//     raw       ::= raw(mut R) RawChar(C) { R.push(C); R }
//     word      ::= LCurly RCurly { super::TgSpan::NoOp }
//     word      ::= LCurly raw(W) RCurly { super::TgSpan::Filling(W) }
//     word      ::= LSBracket Tick raw(W) RSBracket { super::TgSpan::Code(W) }
//     word      ::= LSBracket Star main(S) RSBracket { super::TgSpan::Bold(S) }
//     word      ::= LSBracket main(H) RSBracket LParen raw(L) RParen { super::TgSpan::Link(H, L) }
//     word      ::= LSBracket Tilde words(R) RSBracket { super::TgSpan::Strikethrough(R) }
//     word      ::= LSBracket Underscore main(R) RSBracket { super::TgSpan::Italic(R) }
//     word      ::= LSBracket DoubleUnderscore main(R) RSBracket { super::TgSpan::Underline(R) }
//     word      ::= LSBracket DoubleBar main(R) RSBracket { super::TgSpan::Spoiler(R) }
//     word      ::= LTBracket raw(W) RTBracket LParen raw(L) RParen { super::TgSpan::Button(W, L) }
//     word      ::= LTBracket LTBracket raw(W) RTBracket RTBracket LParen raw(L) RParen { super::TgSpan::NewlineButton(W, L) }
// }
pomelo! {
    %include {
             use super::{FilterCommond, Header};
             use super::ParsedArg;
        }
    %error crate::tg::markdown::DefaultParseErr;
    %parser pub struct Parser{};
    %type input FilterCommond;
    %token #[derive(Debug)] pub(crate) enum Token{};
    %type quote String;
    %type fw ParsedArg;
    %type fws String;
    %type LangCode (String, String);
    %type multi Vec<ParsedArg>;
    %type list Vec<ParsedArg>;
    %type Str String;
    %type footer String;
    %type filterws String;
    %type ign ParsedArg;
    %type header Header;
    %type Whitespace String;
    %type inputm Vec<super::TgSpan>;
    %type words Vec<super::TgSpan>;
    %type main Vec<super::TgSpan>;
    %type word super::TgSpan;
    %type wordraw (super::TgSpan, super::TgSpan);
    %type RawChar char;
    %type Space char;
    %type text String;
    %type wstr String;
    %type blocklist String;
    %type blockstr String;
    %type Mono String;

    input    ::= header(A) Eof {
        FilterCommond {
            header: Some(A),
            body: vec![]
        }
    }
    input    ::= header(A) Whitespace(_) main(W) Eof {
        FilterCommond {
            header: Some(A),
            body: W
        }
    }
    // input    ::= header(A)  footer(F) {
    //     FilterCommond {
    //         header: Some(A),
    //         body: None,
    //         footer: Some(F)
    //     }
    // }

    // input    ::= header(A) Whitespace(_) main(W) footer(F) {
    //     FilterCommond {
    //         header: Some(A),
    //         body: Some(W),
    //         footer: Some(F)
    //     }
    // }

    input    ::= main(A) Eof {
         FilterCommond {
            header: None,
            body: A
        }
    }

    main     ::= words?(A) { A.unwrap_or_else(Vec::new) }
    main     ::= Whitespace(_) words?(A) { A.unwrap_or_else(Vec::new) }



        //main     ::= words?(A) { A.unwrap_or_else(Vec::new) }



    words    ::= words(mut L) Whitespace(S) word(W) { L.push(super::TgSpan::Raw(S)); L.push(W); L }

    words    ::= words(mut L) word(W) { L.push(W); L }
    words    ::= word(C) { vec![C] }
    word      ::= Str(S) { super::TgSpan::Raw(S) }
    word      ::= LCurly wstr(W) RCurly { super::TgSpan::Filling(W) }
    word      ::= LangCode((L, W)) { super::TgSpan::Pre((L, W)) }
    word      ::= Mono(C) { super::TgSpan::Code(C) }
    word      ::= LSBracket Star main(S) RSBracket { super::TgSpan::Bold(S) }
    word      ::= LSBracket main(H) RSBracket LParen Str(L) RParen { super::TgSpan::Link(H, L) }
    word      ::= LSBracket Tilde main(R) RSBracket { super::TgSpan::Strikethrough(R) }
    word      ::= LSBracket Underscore main(R) RSBracket { super::TgSpan::Italic(R) }
    word      ::= LSBracket DoubleUnderscore main(R) RSBracket { super::TgSpan::Underline(R) }
    word      ::= LSBracket DoubleBar main(R) RSBracket { super::TgSpan::Spoiler(R) }
    word      ::= LTBracket wstr(W) RTBracket LParen wstr(L) RParen { super::TgSpan::Button(W, L) }
    word      ::= LTBracket LTBracket wstr(W) RTBracket RTBracket LParen wstr(L) RParen { super::TgSpan::NewlineButton(W, L) }

    wstr      ::= Str(S) { S }
    wstr      ::= Str(mut S) Whitespace(W) wstr(L){ S.push_str(&W); S.push_str(&L); S}
    wstr      ::= Str(mut S) Whitespace(W) { S.push_str(&W); S}



//   footer     ::= Fmuf { "".to_owned() }

    //footer   ::= LCurly Str(A) RCurly Eof { A }
    header   ::= Start multi(V)  { Header::List(V.into_iter().map(|v| v.get_text()).collect()) }
    header   ::= Start blockstr(S) { Header::Arg(S) }
    header   ::= Start quote(S) { Header::Arg(S) }
    fw     ::= Str(A) { ParsedArg::Arg(A) }
    ign      ::= fw(W) { W }
    ign      ::= fw(W) Whitespace(_) { W }
    ign      ::= Whitespace(_) fw(W) { W }
    ign      ::= Whitespace(_) fw(W) Whitespace(_) { W }
    // fws    ::= fw(W) { W.get_text().to_owned() }
    // fws    ::= fws(mut L) Whitespace(S) fw(W) {
    //   L.push_str(&S);
    //   L.push_str(&W.get_text());
    //   L
    // }

    blocklist ::= wstr(S) { S }
    blocklist ::= wstr(mut S) Star { S.push('*'); S }
    blocklist ::= wstr(mut S) Star blocklist(V) { S.push('*'); S.push_str(&V); S }


    blockstr ::= Str(S) { S }
    blockstr ::= Str(mut S) Star { S.push('*'); S }
    blockstr ::= Str(mut S) Star blockstr(V) { S.push('*'); S.push_str(&V); S }

    quote    ::= Quote blocklist(A) Quote { A }
    quote    ::= Quote Whitespace(A) Quote { A }
    quote    ::= Quote Quote { "".to_owned() }
    multi    ::= LParen list(A) RParen {A }
    list     ::= ign(A) { vec![A] }
    list     ::= quote(A) { vec![ParsedArg::Quote(A)] }
    list     ::= list(mut L) Comma ign(A) { L.push(A); L }
    list     ::= list(mut L) Comma Whitespace(_) quote(A) { L.push(ParsedArg::Quote(A)); L }
    list     ::= list(mut L) Comma Whitespace(_) quote(A) Whitespace(_) { L.push(ParsedArg::Quote(A)); L }
}

use parser::{Parser, Token};

use super::admin_helpers::{is_dm, ChatUser};
use super::button::InlineKeyboardBuilder;
use super::command::post_deep_link;
use super::user::Username;

#[derive(Debug)]
enum MonoMode {
    Code(String),
    Mono,
}

/// Lexer to get murkdown tokens
pub struct Lexer {
    s: Vec<char>,
    header: bool,
    escape: bool,
    code: Option<MonoMode>,
    rawbuf: String,
}

fn is_valid(token: char, header: bool) -> bool {
    match token {
        '_' => true,
        '|' => true,
        '~' => true,
        '`' => true,
        '*' => true,
        '[' => true,
        ']' => true,
        '(' => true,
        ')' => true,
        '{' => true,
        '}' => true,
        '<' => true,
        '>' => true,
        ',' if header => true,
        '"' if header => true,
        _ => false,
    }
}

impl Lexer {
    fn new(input: &str, header: bool) -> Self {
        Self {
            s: input.trim().chars().collect(),
            header,
            escape: false,
            code: None,
            rawbuf: String::new(),
        }
    }

    fn next_token(&mut self) -> Vec<Token> {
        let mut output = if self.header {
            vec![Token::Start]
        } else {
            Vec::new()
        };
        let mut idx = 0;
        while let Some(char) = self.s.get(idx) {
            // log::info!("parsing {} {}", idx, char);
            if self.code.is_some() {
                if *char == ']' && idx > 0 && self.s.get(idx - 1) != Some(&'\\') {
                    // log::info!("COMMIT! {:?}", self.s.get(idx - 1));
                    let s: String = self.rawbuf.drain(..).collect();
                    match self.code.take().unwrap() {
                        MonoMode::Code(lang) => output.push(Token::LangCode((lang, s))),
                        MonoMode::Mono => output.push(Token::Mono(s)),
                    }
                } else {
                    self.rawbuf.push(*char);
                }
                idx += 1;
                continue;
            }
            if self.escape {
                self.escape = false;
                self.rawbuf.push(*char);
                if let Some(c) = self.s.get(idx + 1) {
                    if is_valid(*c, self.header) || (char.is_whitespace() != c.is_whitespace()) {
                        let s: String = self.rawbuf.drain(..).collect();
                        if char.is_whitespace() && !s.is_empty() && s.trim().is_empty() {
                            output.push(Token::Whitespace(s));
                        } else {
                            output.push(Token::Str(s));
                        }
                    }
                } else {
                    let s: String = self.rawbuf.drain(..).collect();

                    if char.is_whitespace() && !s.is_empty() && s.trim().is_empty() {
                        output.push(Token::Whitespace(s));
                    } else {
                        output.push(Token::Str(s));
                    }
                }
                idx += 1;
                continue;
            }
            match char {
                '\\' => {
                    self.escape = true;
                    idx += 1;
                    continue;
                }
                '_' => {
                    if let Some('_') = self.s.get(idx + 1) {
                        output.push(Token::DoubleUnderscore);
                        idx += 1;
                        continue;
                    }
                    if idx > 0 && self.s.get(idx - 1).map(|v| *v != '_').unwrap_or(true) {
                        output.push(Token::Underscore);
                    }
                }
                '|' => {
                    if let Some('|') = self.s.get(idx + 1) {
                        output.push(Token::DoubleBar);
                        idx += 1;
                        continue;
                    }
                }
                '~' => output.push(Token::Tilde),
                '*' => output.push(Token::Star),
                '[' => {
                    if let (Some('`'), Some(false)) = (
                        self.s.get(idx + 1),
                        if idx > 0 {
                            self.s.get(idx - 1).map(|v| *v == '\\')
                        } else {
                            Some(false)
                        },
                    ) {
                        if let Some((tick, _)) = self.s.get(idx + 2..).and_then(|thing| {
                            thing.iter().find_position(|p| **p == '`' || **p == ']')
                        }) {
                            // if tick > 0 {
                            //     log::info!("escape2 {:?} ", self.s.get(tick + idx + 1));
                            // }
                            if tick > 0
                                && self.s.get(tick + idx + 1) != Some(&'\\')
                                && self.s.get(tick + idx + 2) != Some(&']')
                            {
                                self.code = self
                                    .s
                                    .get(idx + 2..tick + idx + 2)
                                    .map(|v| MonoMode::Code(v.iter().collect()));

                                if tick + 3 < self.s.len() {
                                    idx += tick + 2;
                                }
                            } else {
                                self.code = Some(MonoMode::Mono);
                                idx += 1;
                            }
                            // log::info!("found tick {} {} {:?}", tick, char, self.code);
                        } else {
                            self.code = Some(MonoMode::Mono);
                            idx += 1;
                        }

                        while let Some(true) = self.s.get(idx + 1).map(|v| v.is_whitespace()) {
                            idx += 1;
                        }
                    //                        iter.take_while_ref(|v| v.1.is_whitespace()).for_each(drop);
                    } else {
                        output.push(Token::LSBracket)
                    }
                }
                ']' => output.push(Token::RSBracket),
                '(' => output.push(Token::LParen),
                ')' => output.push(Token::RParen),
                '{' => output.push(Token::LCurly),
                '}' => output.push(Token::RCurly),
                '<' => output.push(Token::LTBracket),
                '>' => output.push(Token::RTBracket),
                ',' if self.header => output.push(Token::Comma),
                '"' if self.header => output.push(Token::Quote),
                _ => {
                    self.rawbuf.push(*char);
                    if let Some(c) = self.s.get(idx + 1) {
                        if is_valid(*c, self.header) || (char.is_whitespace() != c.is_whitespace())
                        {
                            let s = self.rawbuf.clone();
                            self.rawbuf.clear();

                            if char.is_whitespace() && !s.is_empty() && s.trim().is_empty() {
                                output.push(Token::Whitespace(s));
                            } else {
                                output.push(Token::Str(s));
                            }
                        }
                    } else {
                        let s: String = self.rawbuf.drain(..).collect();

                        if char.is_whitespace() && !s.is_empty() && s.trim().is_empty() {
                            output.push(Token::Whitespace(s));
                        } else {
                            output.push(Token::Str(s));
                        }
                    }
                }
            };
            idx += 1;
        }
        output.push(Token::Eof);

        output
    }
}

pub type ButtonFn = Arc<
    dyn for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
>;

pub trait Escape {
    fn unescape(&self, header: bool) -> Cow<'_, str>;
    fn escape(&self, header: bool) -> Cow<'_, str>;
    fn is_escaped(&self, header: bool) -> bool;
}

impl<T> Escape for T
where
    T: AsRef<str>,
{
    fn escape(&self, header: bool) -> Cow<'_, str> {
        let s = self.as_ref();
        // if self.is_escaped(header) {
        //     return Cow::Borrowed(s);
        // }

        let mut out = String::with_capacity(s.len());
        let mut iter = s.chars().peekable();
        while let Some(c) = iter.next() {
            if let Some(n) = iter.peek() {
                if c == '\\' && (*n == '\\' || is_valid(*n, header) || c != '}' || c != '{') {
                    continue;
                }
            }

            if c == '\\' || (is_valid(c, header) || c == '}' || c == '{') {
                out.push('\\');
            }

            out.push(c);
        }

        Cow::Owned(out)
    }

    fn is_escaped(&self, header: bool) -> bool {
        let mut iter = self.as_ref().chars().peekable();
        while let Some(c) = iter.next() {
            if let Some(n) = iter.peek() {
                if (is_valid(*n, header) && c != '}' && c != '{') && c != '\\' && *n != '\\' {
                    return false;
                }
            }
        }

        true
    }

    fn unescape(&self, header: bool) -> Cow<'_, str> {
        let s = self.as_ref();
        let mut res = String::with_capacity(s.len());
        let mut iter = s.chars().peekable();

        while let Some(c) = iter.next() {
            if let Some(n) = iter.peek() {
                if is_valid(*n, header) && c == '\\' {
                    res.push(*n);
                    iter.next();
                    continue;
                } else {
                    res.push(c);
                }
            } else {
                res.push(c);
            }
        }
        Cow::Owned(res)
    }
}

#[derive(Clone)]
struct OwnedChatUser {
    chat: Chat,
    user: User,
}

impl<'a, T> From<T> for OwnedChatUser
where
    T: AsRef<ChatUser<'a>>,
{
    fn from(value: T) -> Self {
        let value = value.as_ref();
        Self {
            user: value.user.to_owned(),
            chat: value.chat.to_owned(),
        }
    }
}

impl<'a> From<&'a ChatUser<'a>> for OwnedChatUser {
    fn from(value: &'a ChatUser<'a>) -> Self {
        Self {
            user: value.user.to_owned(),
            chat: value.chat.to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct MarkupBuilder {
    existing_entities: Option<Vec<MessageEntity>>,
    pub entities: Vec<MessageEntity>,
    pub buttons: InlineKeyboardBuilder,
    pub header: Option<Header>,
    pub offset: i64,
    pub diff: i64,
    pub text: String,
    pub filling: bool,
    pub enabled_header: bool,
    pub enabled_fillings: bool,
    button_function: ButtonFn,
    chatuser: Option<OwnedChatUser>,
    pub built_markup: Option<EReplyMarkup>,
    pub fillings: BTreeSet<String>,
}

#[inline(always)]
pub(crate) fn button_deeplink_key(key: &str) -> String {
    format!("bdlk:{}", key)
}

#[inline(always)]
pub(crate) fn rules_deeplink_key(key: &str) -> String {
    format!("dlrules:{}", key)
}

pub fn get_markup_for_buttons(button: Vec<button::Model>) -> Option<InlineKeyboardBuilder> {
    if button.is_empty() {
        None
    } else {
        let b = button
            .into_iter()
            .fold(InlineKeyboardBuilder::default(), |mut acc, b| {
                let v = acc.get_mut();
                let x = b.pos_x as usize;
                let y = b.pos_y as usize;
                if let Some(ve) = v.get_mut(b.pos_y as usize) {
                    ve.insert(x, b);
                } else {
                    let mut ve = Vec::new();
                    ve.insert(x, b);
                    v.insert(y, ve);
                }
                acc
            });
        Some(b)
    }
}

/// Builder for MessageEntity formatting. Generates MessageEntities from either murkdown
/// or manually
impl MarkupBuilder {
    /// Constructs a new empty builder for manual formatting
    pub fn new(existing: Option<Vec<MessageEntity>>) -> Self {
        Self {
            existing_entities: existing.map(|v| {
                v.into_iter()
                    .sorted_by_key(|v| v.get_offset())
                    .collect_vec()
            }),
            entities: Vec::new(),
            offset: 0,
            diff: 0,
            text: String::new(),
            buttons: InlineKeyboardBuilder::default(),
            header: None,
            filling: false,
            enabled_header: false,
            enabled_fillings: true,
            button_function: Arc::new(|_, _| async move { Ok(()) }.boxed()),
            chatuser: None,
            built_markup: None,
            fillings: BTreeSet::new(),
        }
    }

    async fn rules(&mut self) -> Result<()> {
        log::info!("adding rules {}", self.chatuser.is_some());
        if let Some(ref chatuser) = self.chatuser {
            let url = post_deep_link(chatuser.chat.get_id(), rules_deeplink_key).await?;

            let button = InlineKeyboardButtonBuilder::new("Get rules".to_owned())
                .set_url(url)
                .build();
            self.buttons.button(button);
        }
        Ok(())
    }

    pub async fn button(&mut self, hint: String, button_text: String) -> Result<()> {
        if let Some(ref chatuser) = self.chatuser {
            log::info!(
                "building button for note: {} with chat {}",
                button_text,
                chatuser.chat.name_humanreadable()
            );
        }
        let is_dm = self
            .chatuser
            .as_ref()
            .map(|v| is_dm(&v.chat))
            .unwrap_or(true);
        let button = if button_text.starts_with('#') && button_text.len() > 1 && is_dm {
            let idx = button_text.align_char_boundry(1);
            let tail = &button_text[idx..];

            let button = InlineKeyboardButtonBuilder::new(hint)
                .set_callback_data(Uuid::new_v4().to_string())
                .build();

            (*self.button_function)(tail.to_owned(), &button).await?;
            button
        } else if !is_dm && button_text.starts_with('#') && button_text.len() > 1 {
            let chat = self
                .chatuser
                .as_ref()
                .ok_or_else(|| BotError::Generic("missing chatuser".to_owned()))?;
            let chat = chat.chat.get_id();
            let tail = &button_text[1..];

            let url = post_deep_link((chat, tail), button_deeplink_key).await?;

            InlineKeyboardButtonBuilder::new(hint).set_url(url).build()
        } else {
            InlineKeyboardButtonBuilder::new(hint)
                .set_url(button_text.clone())
                .build()
        };

        self.buttons.button_raw(button, Some(button_text));

        Ok(())
    }

    fn parse_tgspan<'a>(&'a mut self, span: Vec<TgSpan>) -> BoxFuture<'a, Result<(i64, i64)>> {
        async move {
            // if topplevel {
            //     log::info!("parse_tgspan {:?}", span);
            // }
            let mut size = 0;

            for span in span {
                //a               log::info!("diff {} offset {}", self.diff, self.offset);
                match (span, self.chatuser.as_ref()) {
                    (TgSpan::Code(code), _) => {
                        self.code(&code);
                    }

                    (TgSpan::Pre((lang, code)), _) => {
                        self.pre(&code, lang, None);
                    }
                    (TgSpan::Italic(s), _) => {
                        let (s, e) = self.parse_tgspan(s).await?;
                        size += e;
                        self.manual("italic", s, e);
                    }
                    (TgSpan::Bold(s), _) => {
                        self.diff += "[*".encode_utf16().count() as i64;
                        let (s, e) = self.parse_tgspan(s).await?;
                        self.diff += "]".encode_utf16().count() as i64;
                        size += e;
                        self.manual("bold", s, e);
                    }
                    (TgSpan::Strikethrough(s), _) => {
                        let (s, e) = self.parse_tgspan(s).await?;
                        size += e;
                        self.manual("strikethrough", s, e);
                    }
                    (TgSpan::Underline(s), _) => {
                        let (s, e) = self.parse_tgspan(s).await?;
                        size += e;
                        self.manual("underline", s, e);
                    }
                    (TgSpan::Spoiler(s), _) => {
                        let (s, e) = self.parse_tgspan(s).await?;
                        size += e;
                        self.manual("spoiler", s, e);
                    }
                    (TgSpan::Button(hint, button), _) => {
                        self.button(hint, button).await?;
                    }
                    (TgSpan::NewlineButton(hint, button), _) => {
                        self.buttons.newline();
                        self.button(hint, button).await?;
                    }
                    (TgSpan::Link(hint, link), _) => {
                        let (s, e) = self.parse_tgspan(hint).await?;
                        size += e;
                        let entity = MessageEntityBuilder::new(s, e)
                            .set_type("text_link".to_owned())
                            .set_url(link)
                            .build();
                        self.entities.push(entity);
                    }
                    (TgSpan::Raw(s), _) => {
                        size += s.encode_utf16().count() as i64;

                        self.text_internal(&s);
                    }
                    (TgSpan::Filling(filling), Some(chatuser)) if self.filling => {
                        match filling.as_str() {
                            "username" => {
                                let user = chatuser.user.clone();
                                let name = user.name_humanreadable().into_owned();
                                size += name.encode_utf16().count() as i64;
                                self.text_mention(name, user, None);
                            }
                            "first" => {
                                let first = chatuser.user.get_first_name().to_owned();
                                size += first.encode_utf16().count() as i64;
                                self.text_internal(&first);
                            }
                            "last" => {
                                let last = chatuser
                                    .user
                                    .get_last_name()
                                    .map(|v| v.to_owned())
                                    .unwrap_or_else(|| "".to_owned());
                                size += last.encode_utf16().count() as i64;
                                self.text_internal(&last);
                            }
                            "mention" => {
                                let user = chatuser.user.clone();
                                let first = user.get_first_name().to_owned();
                                size += first.encode_utf16().count() as i64;
                                self.text_mention(first, user, None);
                            }
                            "chatname" => {
                                let chat = chatuser.chat.name_humanreadable().into_owned();
                                size += chat.encode_utf16().count() as i64;
                                self.text_internal(&chat);
                            }
                            "id" => {
                                let id = chatuser.user.get_id().to_string();
                                size += id.encode_utf16().count() as i64;
                                self.text_internal(&id);
                            }
                            "rules" => {
                                self.rules().await?;
                            }
                            s => {
                                let s = format!("{{{}}}", s);
                                size += s.encode_utf16().count() as i64;
                                self.text_internal(&s);
                            }
                        }
                    }
                    (TgSpan::Filling(filling), _) => {
                        if filling.trim().is_empty() {
                            let s = format!("{{{}}}", filling);
                            size += s.encode_utf16().count() as i64;
                            self.text_internal(&s);
                        } else {
                            if self.enabled_fillings {
                                let s = format!("{{{}}}", filling);
                                size += s.encode_utf16().count() as i64;
                                self.text_internal(&s);
                            }
                            self.fillings.insert(filling);
                        }
                    }
                    (TgSpan::NoOp, _) => (),
                };
                self.patch_entities(size);
            }

            let offset = self.offset - size;
            Ok((offset, size))
        }
        .boxed()
    }

    fn patch_entities(&mut self, size: i64) {
        if let Some(existing_entities) = self.existing_entities.as_mut() {
            if self.diff != 0 {
                for entity in existing_entities.iter_mut() {
                    if entity.get_offset() >= self.offset {
                        log::info!("patching entity {} {} {}", self.offset, size, self.diff);
                        entity.set_offset(entity.get_offset() - self.diff);
                    }
                }
                self.diff = 0;
            }
        }
    }

    // fn parse_listitem(&mut self, list_item: ListItem) -> i64 {
    //     list_item
    //         .children
    //         .into_iter()
    //         .map(|i| self.parse_block(i))
    //         .sum()
    // }

    fn parse_block(&mut self, block: Node) -> i64 {
        match block {
            Node::Heading(spans) => spans
                .children
                .into_iter()
                .map(|s| self.parse_block(s))
                .sum::<i64>(),
            Node::Paragraph(spans) => spans
                .children
                .into_iter()
                .map(|s| self.parse_block(s))
                .sum::<i64>(),
            Node::Blockquote(blocks) => blocks
                .children
                .into_iter()
                .map(|b| self.parse_block(b))
                .sum::<i64>(),
            Node::List(l) => l
                .children
                .into_iter()
                .map(|i| self.parse_block(i))
                .sum::<i64>(),
            // Node::Break => {
            //     let s = "\n";
            //     self.text_internal(s);
            //     s.encode_utf16().count() as i64
            // }
            Node::Text(text) => {
                let i = text.value.encode_utf16().count() as i64;
                self.text_internal(&text.value);
                i
            }
            Node::Code(code) => {
                let i = code.value.encode_utf16().count() as i64;
                self.code(&code.value);
                i
            }
            Node::Link(link) => {
                if let Some(hint) = link.title {
                    let i = hint.encode_utf16().count() as i64;
                    self.text_link(hint, link.url, None);
                    i
                } else {
                    0
                }
            }
            Node::Image(_) => 0,
            Node::Emphasis(emp) => {
                let mut size: i64 = 0;
                let start = self.offset;
                emp.children.into_iter().for_each(|v| {
                    size += self.parse_block(v);
                });
                let bold = MessageEntityBuilder::new(start, size)
                    .set_type("italic".to_owned())
                    .build();
                self.entities.push(bold);
                size
            }

            Node::Strong(emp) => {
                let mut size: i64 = 0;
                let start = self.offset;
                emp.children.into_iter().for_each(|v| {
                    size += self.parse_block(v);
                });
                let bold = MessageEntityBuilder::new(start, size)
                    .set_type("bold".to_owned())
                    .build();
                self.entities.push(bold);
                size
            }
            v => {
                self.text_internal(&v.to_string());
                0
            } // TODO: handle more markdown
        }
    }
    /// Parses vanilla markdown and constructs a builder with the corresponding text
    /// and entities
    pub fn from_markdown<T: AsRef<str>>(text: T, existing: Option<Vec<MessageEntity>>) -> Self {
        let text = text.as_ref();
        let mut s = Self::new(existing);
        markdown::to_mdast(text, &ParseOptions::default())
            .into_iter()
            .for_each(|v| {
                s.parse_block(v);
            });
        s
    }

    pub async fn from_tgspan<'a>(
        tgspan: Vec<TgSpan>,
        chatuser: Option<&'a ChatUser<'a>>,
        existing: Option<Vec<MessageEntity>>,
    ) -> Result<Self> {
        let mut s = Self::new(existing);
        s.chatuser = chatuser.map(|v| v.into());
        s.parse_tgspan(tgspan).await?;
        Ok(s)
    }

    pub async fn append<'a, T>(
        &mut self,
        text: T,
        existing: Option<Vec<MessageEntity>>,
    ) -> Result<&'_ mut Self>
    where
        T: AsRef<str>,
    {
        let text = text.as_ref();
        if let (Some(existing), Some(ref existingnew)) = (self.existing_entities.as_mut(), existing)
        {
            existing.extend_from_slice(existingnew);
        }
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(text, false);
        for token in tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        self.parse_tgspan(res.body).await?;
        if let Some(ref existing) = self.existing_entities {
            self.entities.extend_from_slice(existing.as_slice());
        }
        Ok(self)
    }

    pub fn chatuser(mut self, chatuser: Option<&ChatUser<'_>>) -> Self {
        self.chatuser = chatuser.map(|v| v.into());
        self
    }

    pub fn callback<F>(mut self, callback: F) -> Self
    where
        F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>>
            + Send
            + Sync
            + 'static,
    {
        self.button_function = Arc::new(callback);
        self
    }

    pub fn build(&self) -> (&'_ str, &'_ Vec<MessageEntity>) {
        (&self.text, &self.entities)
    }

    pub async fn build_murkdown<'a>(
        mut self,
    ) -> Result<(String, Vec<MessageEntity>, InlineKeyboardBuilder)> {
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(&self.text, self.enabled_header);
        for token in tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        self.offset = 0;
        self.text.clear();
        self.parse_tgspan(res.body).await?;

        if self.enabled_header {
            self.header = res.header;
        }

        if let Some(ref existing) = self.existing_entities {
            self.entities.extend_from_slice(existing.as_slice());
        }

        Ok((self.text, self.entities, self.buttons))
    }

    async fn nofail_internal(&mut self) -> Result<()> {
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(&self.text, self.enabled_header);
        for token in tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        self.offset = 0;
        self.text.clear();
        self.parse_tgspan(res.body).await?;

        if self.enabled_header {
            self.header = res.header;
        }

        if let Some(ref existing) = self.existing_entities {
            self.entities.extend_from_slice(existing.as_slice());
        }

        Ok(())
    }

    pub async fn build_murkdown_nofail<'a>(
        mut self,
    ) -> (String, Vec<MessageEntity>, InlineKeyboardBuilder) {
        if let Ok(()) = self.nofail_internal().await {
            (self.text, self.entities, self.buttons)
        } else {
            (self.text, self.entities, self.buttons)
        }
    }

    pub async fn build_murkdown_nofail_ref(
        &mut self,
    ) -> (
        &'_ mut String,
        &'_ mut Vec<MessageEntity>,
        Option<&'_ mut EReplyMarkup>,
    ) {
        if let Ok(()) = self.nofail_internal().await {
            self.built_markup = Some(EReplyMarkup::InlineKeyboardMarkup(
                self.buttons.build_owned(),
            ));

            (
                &mut self.text,
                &mut self.entities,
                self.built_markup.as_mut(),
            )
        } else {
            self.built_markup = Some(EReplyMarkup::InlineKeyboardMarkup(
                self.buttons.build_owned(),
            ));

            (
                &mut self.text,
                &mut self.entities,
                self.built_markup.as_mut(),
            )
        }
    }

    pub fn filling(mut self, filling: bool) -> Self {
        self.filling = filling;
        self
    }

    pub fn header(mut self, header: bool) -> Self {
        self.enabled_header = header;
        self
    }

    pub fn show_fillings(mut self, fillings: bool) -> Self {
        self.enabled_fillings = fillings;
        self
    }

    /// Appends new unformated text
    pub fn text<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.offset += text.unescape(self.enabled_header).encode_utf16().count() as i64;
        self.push_text(text);
        self
    }

    fn text_internal(&mut self, text: &str) -> &'_ mut Self {
        self.offset += self.push_text(text);
        self
    }

    pub fn set_text(mut self, text: String) -> Self {
        self.text = text;
        self
    }

    fn manual(&mut self, entity_type: &str, start: i64, end: i64) {
        let entity = MessageEntityBuilder::new(start, end)
            .set_type(entity_type.to_owned())
            .build();
        self.entities.push(entity);
    }

    /// Appends a markup value
    pub fn regular_fmt<T: AsRef<str>>(&mut self, entity_type: Markup<T>) -> &'_ mut Self {
        let text = entity_type.get_text();
        let n = text.unescape(self.enabled_header).encode_utf16().count() as i64;
        // let v = text.chars().filter(|p| *p == '\\').count() as i64;

        self.text.push_str(&text.escape(self.enabled_header));
        match entity_type.markup_type {
            MarkupType::Text => {}
            _ => {
                let entity = MessageEntityBuilder::new(self.offset, n)
                    .set_type(entity_type.get_type().to_owned());
                let entity = match entity_type.markup_type {
                    MarkupType::TextLink(link) => entity.set_url(link),
                    MarkupType::TextMention(mention) => entity.set_user(mention),
                    MarkupType::Pre(Some(pre)) => entity.set_language(pre),
                    MarkupType::CustomEmoji(emoji) => entity.set_custom_emoji_id(emoji),
                    _ => entity,
                };

                let entity = entity.build();
                self.existing_entities
                    .get_or_insert_with(Vec::new)
                    .push(entity);
            }
        }

        self.offset += entity_type.advance.unwrap_or(n);
        self
    }

    /// Appends a markup value
    pub fn regular<T: AsRef<str>>(&mut self, entity_type: Markup<T>) -> &'_ mut Self {
        let text = entity_type.get_text();
        let n = text.encode_utf16().count() as i64;

        self.text.push_str(text);
        match entity_type.markup_type {
            MarkupType::Text => {}
            _ => {
                let entity = MessageEntityBuilder::new(self.offset, n)
                    .set_type(entity_type.get_type().to_owned());
                let entity = match entity_type.markup_type {
                    MarkupType::TextLink(link) => entity.set_url(link),
                    MarkupType::TextMention(mention) => entity.set_user(mention),
                    MarkupType::Pre(Some(pre)) => entity.set_language(pre),
                    MarkupType::CustomEmoji(emoji) => entity.set_custom_emoji_id(emoji),
                    _ => entity,
                };

                let entity = entity.build();
                self.entities.push(entity);
            }
        }

        self.offset += entity_type.advance.unwrap_or(n);
        self
    }

    fn push_text<T: AsRef<str>>(&mut self, text: T) -> i64 {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        self.text.push_str(text);
        n
    }

    /// Appends a new text link. Pass a number for advance to allow text/formatting overlap
    pub fn text_link<T: AsRef<str>>(
        &mut self,
        text: T,
        link: String,
        advance: Option<i64>,
    ) -> &'_ mut Self {
        let text = text.as_ref();
        let n = self.push_text(text);
        //        let text = text.escape(self.enabled_header);
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("text_link".to_owned())
            .set_url(link)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self
    }

    /// Appends a new text mention. Pass a number for advance to allow text/formatting overlap
    pub fn text_mention<T: AsRef<str>>(
        &mut self,
        text: T,
        mention: User,
        advance: Option<i64>,
    ) -> &'_ Self {
        let text = text.as_ref();
        let n = self.push_text(text);
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("text_mention".to_owned())
            .set_user(mention)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);

        self
    }

    /// Appends a new pre block. Pass a number for advance to allow text/formatting overlap
    pub fn pre<T: AsRef<str>>(
        &mut self,
        text: T,
        language: String,
        advance: Option<i64>,
    ) -> &'_ Self {
        let text = text.as_ref();
        let n = self.push_text(text);
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("pre".to_owned())
            .set_language(language)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self
    }

    /// Appends a new custom emoji. Pass a number for advance to allow text/formatting overlap
    pub fn custom_emoji<T: AsRef<str>>(
        &mut self,
        text: T,
        emoji_id: String,
        advance: Option<i64>,
    ) -> &'_ Self {
        let text = text.as_ref();
        let n = self.push_text(text);
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("custom_emoji".to_owned())
            .set_custom_emoji_id(emoji_id)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self
    }

    /// Appends strikethrouh text. Pass a number for advance to allow text/formatting overlap
    pub fn strikethrough<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::StrikeThrough.text(&text))
    }

    /// Appends a new hashtag. Pass a number for advance to allow text/formatting overlap
    pub fn hashtag<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::HashTag.text(&text))
    }

    /// Appends a new cashtag. Pass a number for advance to allow text/formatting overlap
    pub fn cashtag<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::CashTag.text(&text))
    }

    /// Appends a new bot command. Pass a number for advance to allow text/formatting overlap
    pub fn bot_command<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::BotCommand.text(&text))
    }

    /// Appends a new email. Pass a number for advance to allow text/formatting overlap
    pub fn email<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Email.text(&text))
    }

    /// Appends a new phone number. Pass a number for advance to allow text/formatting overlap
    pub fn phone_number<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::PhoneNumber.text(&text))
    }

    /// Appends bold text. Pass a number for advance to allow text/formatting overlap
    pub fn bold<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Bold.text(&text))
    }

    /// Appends a italic text. Pass a number for advance to allow text/formatting overlap
    pub fn italic<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Italic.text(&text))
    }

    /// Appends underline text. Pass a number for advance to allow text/formatting overlap
    pub fn underline<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Underline.text(&text))
    }

    /// Appends spoiler text. Pass a number for advance to allow text/formatting overlap
    pub fn spoiler<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Spoiler.text(&text))
    }

    /// Appends a formatted code block. Pass a number for advance to allow text/formatting overlap
    pub fn code<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Code.text(&text))
    }

    /// Appends a new mention. Pass a number for advance to allow text/formatting overlap
    pub fn mention<T: AsRef<str>>(&mut self, text: T) -> &'_ mut Self {
        self.regular(MarkupType::Mention.text(&text))
    }

    /// shortcut for adding whitespace
    pub fn s(&mut self) -> &'_ mut Self {
        let t = " ";
        let count = t.encode_utf16().count() as i64;

        self.offset += count;
        self.text.push_str(t);
        self
    }

    pub async fn build_filter(
        mut self,
    ) -> (
        String,
        Vec<MessageEntity>,
        InlineKeyboardBuilder,
        Option<Header>,
        BTreeSet<String>,
    ) {
        self.enabled_header = true;
        if let Ok(()) = self.nofail_internal().await {
            (
                self.text,
                self.entities,
                self.buttons,
                self.header,
                self.fillings,
            )
        } else {
            (
                self.text,
                self.entities,
                self.buttons,
                self.header,
                self.fillings,
            )
        }
    }
}

lazy_static! {
    static ref FILLER_REGEX: Regex = Regex::new(r"\{\w*\}").unwrap();
}

pub fn remove_fillings(text: &str) -> String {
    FILLER_REGEX.replace_all(text, "").into_owned()
}

pub async fn retro_fillings<'a>(
    text: String,
    entities: Vec<MessageEntity>,
    mut buttons: Option<&mut InlineKeyboardBuilder>,
    chatuser: &ChatUser<'a>,
) -> Result<(String, Vec<MessageEntity>)> {
    let mut res = String::with_capacity(text.len());
    let mut extra_entities = Vec::<MessageEntity>::new();
    let mut offsets = entities
        .iter()
        .map(|v| (v.get_offset(), v.get_length()))
        .collect::<Vec<(i64, i64)>>();
    let mut pos = 0;
    let mut prev = 0;
    let iter: Vec<regex::Match<'_>> = FILLER_REGEX.find_iter(&text).collect();
    if iter.is_empty() {
        return Ok((text, entities));
    }
    for mat in iter {
        // I promise no UTF-8 weirdness here. Regex is {.*} so always has
        // ascii ends
        let filling = &mat.as_str()[1..mat.len() - 1];
        let regular = &text[prev..mat.start()];
        res.push_str(regular);
        pos += regular.encode_utf16().count() as i64;
        prev = mat.end();
        // log::info!("matching {}: {}", filling, pos);
        let (text, entity) = match filling {
            "username" => {
                let user = chatuser.user;
                let name = user.name_humanreadable_unescape();
                let start = pos;
                let len = name.encode_utf16().count() as i64;
                (
                    name,
                    Some(
                        MessageEntityBuilder::new(start, len)
                            .set_type("text_mention".to_owned())
                            .set_user(user.to_owned())
                            .build(),
                    ),
                )
            }
            "first" => {
                let first = chatuser.user.get_first_name();
                (Cow::Borrowed(first), None)
            }
            "last" => {
                let last = chatuser
                    .user
                    .get_last_name()
                    .map(Cow::Borrowed)
                    .unwrap_or(Cow::Borrowed(""));
                (last, None)
            }
            "mention" => {
                let user = chatuser.user;
                let first = user.get_first_name();
                let start = pos;
                let len = first.encode_utf16().count() as i64;
                (
                    Cow::Borrowed(first),
                    Some(
                        MessageEntityBuilder::new(start, len)
                            .set_type("text_mention".to_owned())
                            .set_user(user.to_owned())
                            .build(),
                    ),
                )
            }
            "chatname" => {
                let chat = chatuser.chat.name_humanreadable_unescape();
                (chat, None)
            }
            "rules" => {
                if let Some(buttons) = buttons.as_mut() {
                    let url = post_deep_link(chatuser.chat.get_id(), rules_deeplink_key).await?;

                    let button = InlineKeyboardButtonBuilder::new("Get rules".to_owned())
                        .set_url(url)
                        .build();
                    buttons.button(button);

                    (Cow::Owned("".to_owned()), None)
                } else {
                    (Cow::Owned("{rules}".to_owned()), None)
                }
            }
            "id" => {
                let id = chatuser.user.get_id().to_string();
                (Cow::Owned(id), None)
            } // TODO: handle rules filler
            s => {
                let s = format!("{{{}}}", s);
                (Cow::Owned(s), None)
            }
        };

        let diff = text.encode_utf16().count() as i64 - mat.as_str().encode_utf16().count() as i64;
        res.push_str(&text);
        pos += text.encode_utf16().count() as i64;
        log::info!(
            "retro_fillings pos {} diff {} text {} mat {} regular {}",
            pos,
            diff,
            text,
            mat.as_str(),
            regular
        );
        for v in offsets.as_mut_slice() {
            if v.0 >= pos - text.encode_utf16().count() as i64 {
                log::info!("reloacating {:?}", v);
                v.0 += diff;
            }
        }

        if let Some(entity) = entity {
            extra_entities.push(entity);
        }
    }

    let regular = &text[prev..];
    res.push_str(regular);
    let newoffsets = entities
        .into_iter()
        .zip(offsets)
        .map(|(mut entity, (off, len))| {
            entity.set_offset(off).set_length(len);
            entity
        })
        .chain(extra_entities)
        .collect::<Vec<MessageEntity>>();
    log::info!("retro_fillings final {}", res.encode_utf16().count());
    Ok((res, newoffsets))
}

/// Represents metadata for a single MessageEntity. Useful when programatically
/// constructing formatted text using MarkupBuilder
#[derive(Clone, Debug, Hash)]
pub struct Markup<T: AsRef<str>> {
    markup_type: MarkupType,
    text: T,
    advance: Option<i64>,
}

/// Enum with varients for every kind of MessageEntity
#[derive(Clone, Debug, Hash)]
pub enum MarkupType {
    Text,
    StrikeThrough,
    HashTag,
    CashTag,
    BotCommand,
    Email,
    PhoneNumber,
    Bold,
    Italic,
    Underline,
    Spoiler,
    Code,
    Mention,
    Url,
    BlockQuote,
    TextLink(String),
    TextMention(User),
    Pre(Option<String>),
    CustomEmoji(String),
}

impl<T> From<T> for Markup<String>
where
    T: AsRef<str>,
{
    fn from(value: T) -> Self {
        MarkupType::Text.text(value.as_ref().escape(false).into_owned())
    }
}

impl MarkupType {
    /// Adds text to an existing MarkupType, preserving current formatting
    pub fn text<T: AsRef<str>>(self, text: T) -> Markup<T> {
        Markup {
            markup_type: self,
            text,
            advance: None,
        }
    }
}

impl<T> Markup<T>
where
    T: AsRef<str>,
{
    /// Gets the telegram api type for this Markup
    fn get_type(&self) -> &str {
        match &self.markup_type {
            MarkupType::Text => "",
            MarkupType::TextMention(_) => "text_mention",
            MarkupType::TextLink(_) => "text_link",
            MarkupType::Pre(_) => "pre",
            MarkupType::BlockQuote => "blockquote",
            MarkupType::CustomEmoji(_) => "custom_emoji",
            MarkupType::StrikeThrough => "strikethrough",
            MarkupType::HashTag => "hashtag",
            MarkupType::CashTag => "cashtag",
            MarkupType::BotCommand => "bot_command",
            MarkupType::Email => "email",
            MarkupType::PhoneNumber => "phone_number",
            MarkupType::Bold => "bold",
            MarkupType::Italic => "italic",
            MarkupType::Underline => "underline",
            MarkupType::Spoiler => "spoiler",
            MarkupType::Code => "code",
            MarkupType::Mention => "mention",
            MarkupType::Url => "url",
        }
    }

    /// gets the unformatted text for this markup
    fn get_text(&self) -> &'_ str {
        self.text.as_ref()
    }

    /// sets the "advance" for this markup, essentially how overlapped it is
    /// with previous entities
    pub fn advance(mut self, advance: i64) -> Self {
        self.advance = Some(advance);
        self
    }
}

/// Type used by proc macros for hygiene purposes and to get the borrow checker
/// to not complain. Don't use this manually
pub struct EntityMessage {
    pub builder: MarkupBuilder,
    pub chat: i64,
    pub reply_markup: Option<EReplyMarkup>,
    pub disable_murkdown: bool,
}

impl EntityMessage {
    pub fn new(chat: i64) -> Self {
        Self {
            builder: MarkupBuilder::new(None),
            chat,
            reply_markup: None,
            disable_murkdown: false,
        }
    }

    pub fn from_text<T>(chat: i64, text: T) -> Self
    where
        T: AsRef<str>,
    {
        let mut s = Self {
            builder: MarkupBuilder::new(None),
            chat,
            reply_markup: None,
            disable_murkdown: false,
        };

        s.builder.text(text);
        s
    }

    pub fn reply_markup(mut self, reply_markup: EReplyMarkup) -> Self {
        self.reply_markup = Some(reply_markup);
        self
    }

    pub fn disable_murkdown(mut self, disable: bool) -> Self {
        self.disable_murkdown = disable;
        self
    }

    pub async fn call(&mut self) -> CallSendMessage<'_, i64> {
        if self.disable_murkdown {
            self.builder.build_murkdown_nofail_ref().await;
            let call = TG
                .client
                .build_send_message(self.chat, &self.builder.text)
                .entities(&self.builder.entities);
            if let Some(ref reply_markup) = self.reply_markup {
                call.reply_markup(reply_markup)
            } else {
                call
            }
        } else {
            let (text, entities, buttons) = self.builder.build_murkdown_nofail_ref().await;
            log::info!("call {} {}", text, self.reply_markup.is_some());
            let call = TG
                .client
                .build_send_message(self.chat, text)
                .entities(entities);
            if let Some(ref reply_markup) = self.reply_markup {
                call.reply_markup(reply_markup)
            } else if let Some(buttons) = buttons.map(|v| &*v) {
                call.reply_markup(buttons)
            } else {
                call
            }
        }
    }

    pub fn textentities(&self) -> (&'_ str, &'_ Vec<MessageEntity>) {
        (&self.builder.text, &self.builder.entities)
    }
}

#[allow(dead_code, unused_imports)]
mod test {
    use std::borrow::Cow;

    use botapi::gen_types::{ChatBuilder, UserBuilder};
    use futures::executor::block_on;

    use super::*;
    const MARKDOWN_TEST: &str = "what
        [*bold]
        thing
        [coinscam](http://coinscam.org)
        ";

    const MARKDOWN_SIMPLE: &str = "[*bold]";

    const NESTED_BOLD: &str = "[*[*bold] [*bold]]";
    const EMPTY: &str = "";

    const RAW: &str = "thing";

    const ESCAPE: &str = "\\[ thing";

    fn test_parse(markdown: &str) -> FilterCommond {
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(markdown, false);
        for token in tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap()
    }

    #[test]
    fn button() {
        let mut tokenizer = Lexer::new("<button>(https://example.com)", false);
        let mut parser = Parser::new();
        for token in tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap();
    }

    #[test]
    fn tokenize_test() {
        let mut tokenizer = Lexer::new(MARKDOWN_SIMPLE, false);
        let tokens = tokenizer.next_token();
        let mut tokens = tokens.iter();
        if let Some(Token::LSBracket) = tokens.next() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Star) = tokens.next() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Str(s)) = tokens.next() {
            assert_eq!(s, "bold");
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::RSBracket) = tokens.next() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Eof) = tokens.next() {
        } else {
            panic!("Missing Eof");
        }

        if tokens.next().is_some() {
            panic!("Extra token?");
        }
    }

    #[test]
    fn parse_simple() {
        test_parse(MARKDOWN_SIMPLE);
    }

    #[test]
    fn parse_empty() {
        test_parse(EMPTY);
    }

    #[test]
    fn parse_nested() {
        test_parse(EMPTY);
    }

    #[test]
    fn parse_multi() {
        let tokens = test_parse(MARKDOWN_TEST).body;
        let mut counter = 0;
        for token in tokens {
            if let TgSpan::Raw(raw) = token {
                println!("RAW {}", raw);
                counter += 1;
            }
        }
        assert_eq!(counter, 5);
    }

    #[test]
    fn double_escape() {
        let st = "this_is_a_string";
        let escape = st.escape(false);
        let d = escape.escape(false);
        assert_eq!(escape, d);
    }

    #[test]
    fn escape_multipoint() {
        let s = "help me 😄";
        assert_eq!(s, s.escape(false));
    }

    #[test]
    fn raw_test() {
        if let Some(TgSpan::Raw(res)) = test_parse(RAW).body.first() {
            assert_eq!(res, RAW);
        } else {
            panic!("failed to parse");
        }
    }

    #[test]
    fn nested_bold() {
        test_parse(NESTED_BOLD);
    }

    #[test]
    fn escape_trait() {
        let test = "This is a | string";
        let res = test.escape(false);
        assert_eq!(res, "This is a \\| string");
    }

    #[test]
    fn unescape() {
        let test = "this is a test\\_message";
        let control = "already unescaped";
        assert_eq!(test.unescape(false), "this is a test_message");
        assert_eq!(control.unescape(false), "already unescaped");
    }

    #[test]
    fn is_escaped() {
        let is_escaped = "this is a test\\_message";
        let not = "this is a test_message";
        let control = "this is a test message";

        assert!(is_escaped.is_escaped(true));
        assert!(!not.is_escaped(true));
        assert!(control.is_escaped(true));
    }

    #[test]
    fn escape() {
        if let [TgSpan::Raw(ref res), TgSpan::Raw(ref ws), TgSpan::Raw(ref res2)] =
            test_parse(ESCAPE).body.as_slice()[0..]
        {
            let mut r = String::new();
            r.push_str(res);
            r.push_str(ws);
            r.push_str(res2);
            assert_eq!(r, ESCAPE.replace('\\', "").as_str());
        } else {
            panic!("failed to parse");
        }
    }

    #[tokio::test]
    async fn retro_fillings_wide() {
        let dumpling = "🥟";
        let test = "[*Hi] there {mention} welcome [*to] {chatname} [*bold]";
        let (test, entities, mut buttons) = MarkupBuilder::new(None)
            .set_text(test.to_owned())
            .filling(false)
            .header(false)
            .build_murkdown()
            .await
            .unwrap();
        let chatuser = ChatUser {
            chat: &ChatBuilder::new(0)
                .set_title("goth group".to_owned())
                .build(),
            user: &UserBuilder::new(1, false, dumpling.to_owned()).build(),
        };

        println!("text: {}", test);

        let (test, entities) = retro_fillings(test, entities, Some(&mut buttons), &chatuser)
            .await
            .unwrap();
        let len = test.encode_utf16().count() as i64;
        assert_eq!(entities.len(), 4);
        for entity in entities {
            assert!(entity.get_offset() + entity.get_length() <= len);
        }
    }

    #[tokio::test]
    async fn parse_help() {
        let test = r#"
    [__underline text]
        "#;
        MarkupBuilder::new(None)
            .set_text(test.to_owned())
            .filling(false)
            .header(false)
            .build_murkdown()
            .await
            .unwrap();
    }
}
