//! Various tools for generating formatted telegram text using MessageEntities
//! There are two APIs here, a markdown like "murkdown" formatting language and
//! a builder api for manually generating formatted text

use botapi::gen_methods::CallSendMessage;
use botapi::gen_types::{
    EReplyMarkup, InlineKeyboardButton, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
    MessageEntity, MessageEntityBuilder, User,
};
use futures::future::BoxFuture;
use futures::FutureExt;
use markdown::{Block, ListItem, Span};
use uuid::Uuid;

use crate::statics::TG;
use crate::util::error::{BotError, Result};
use lazy_static::lazy_static;
use pomelo::pomelo;
use regex::Regex;
use std::fmt::Display;
use std::{iter::Peekable, str::Chars};
use thiserror::Error;

/// Custom error type for murkdown parse failure. TODO: add additional context here
#[derive(Debug, Error)]
pub struct DefaultParseErr {}

impl Display for DefaultParseErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("default parse error")?;
        Ok(())
    }
}

impl Default for DefaultParseErr {
    fn default() -> Self {
        DefaultParseErr {}
    }
}

/// Data for filter's header
#[derive(Clone)]
pub enum Header {
    List(Vec<String>),
    Arg(String),
}

/// Complete parsed filter with header, body, and footer
pub struct FilterCommond {
    pub header: Option<Header>,
    pub body: Vec<TgSpan>,
    pub footer: Option<String>,
}

/// Type for representing murkdown syntax tree
pub enum TgSpan {
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
    %token #[derive(Debug)] pub enum Token{};
    %type quote String;
    %type fw ParsedArg;
    %type fws String;
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

    input    ::= header(A) Eof {
        FilterCommond {
            header: Some(A),
            body: vec![],
            footer: None
        }
    }
    input    ::= header(A) Whitespace(_) main(W) Eof {
        FilterCommond {
            header: Some(A),
            body: W,
            footer: None
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
            body: A,
            footer: None
        }
    }

    main     ::= words?(A) { A.unwrap_or_else(Vec::new) }
    //main     ::= words?(A) { A.unwrap_or_else(Vec::new) }


    words    ::= words(mut L) Whitespace(S) word(W) { L.push(super::TgSpan::Raw(S)); L.push(W); L }
    words    ::= words(mut L) word(W) { L.push(W); L }
    words    ::= word(C) { vec![C] }
  //  words    ::= word(C) Whitespace(_) word(_) { vec![C] }

    //words    ::= Str(C) { vec![super::TgSpan::Raw(C)]}

//    text     ::=   Whitespace(_) { W }

 //   word      ::= LCurly RCurly { super::TgSpan::NoOp }
    word      ::= Str(S) { super::TgSpan::Raw(S) }
    word      ::= LCurly Str(W) RCurly { super::TgSpan::Filling(W) }
    word      ::= LSBracket Tick Str(W) RSBracket { super::TgSpan::Code(W) }
    word      ::= LSBracket Star main(S) RSBracket { super::TgSpan::Bold(S) }
    word      ::= LSBracket main(H) RSBracket LParen Str(L) RParen { super::TgSpan::Link(H, L) }
    word      ::= LSBracket Tilde main(R) RSBracket { super::TgSpan::Strikethrough(R) }
    word      ::= LSBracket Underscore main(R) RSBracket { super::TgSpan::Italic(R) }
    word      ::= LSBracket DoubleUnderscore main(R) RSBracket { super::TgSpan::Underline(R) }
    word      ::= LSBracket DoubleBar main(R) RSBracket { super::TgSpan::Spoiler(R) }
    word      ::= LTBracket Str(W) RTBracket LParen Str(L) RParen { super::TgSpan::Button(W, L) }
    word      ::= LTBracket LTBracket Str(W) RTBracket RTBracket LParen Str(L) RParen { super::TgSpan::NewlineButton(W, L) }

//   footer     ::= Fmuf { "".to_owned() }

    //footer   ::= LCurly Str(A) RCurly Eof { A }
    header   ::= Start multi(V)  { Header::List(V.into_iter().map(|v| v.get_text()).collect()) }
    header   ::= Start Str(S) { Header::Arg(S) }
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

    text     ::= Str(S) { S }
    text     ::= text(mut T) Whitespace(W) Str(S) { T.push_str(&W); T.push_str(&S); T }

    quote    ::= Quote text(A) Quote { A }
    multi    ::= LParen list(A) RParen {A }
    list     ::= ign(A) { vec![A] }
    list     ::= list(mut L) Comma ign(A) { L.push(A); L }
    list     ::= list(mut L) Comma quote(A) { L.push(ParsedArg::Quote(A)); L }

}

pub use parser::{Parser, Token};

use super::admin_helpers::{is_dm, ChatUser};
use super::button::InlineKeyboardBuilder;
use super::command::post_deep_link;
use super::user::Username;

/// Lexer to get murkdown tokens
pub struct Lexer<'a> {
    s: Peekable<Chars<'a>>,
    rawbuf: String,
    pos: usize,
    end: bool,
}

fn is_valid(token: char) -> bool {
    match token {
        '\\' => true,
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
        ',' => true,
        _ => false,
    }
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let chars = input.chars().peekable();
        Self {
            s: chars,
            rawbuf: String::new(),
            pos: 0,
            end: false,
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        self.pos += 1;
        if self.pos == 1 {
            return Some(Token::Start);
        }
        if let Some(char) = self.s.next() {
            match char {
                '\\' => self.s.next().map(|char| Token::RawChar(char)),
                '_' => {
                    if let Some('_') = self.s.peek() {
                        self.s.next();
                        Some(Token::DoubleUnderscore)
                    } else {
                        Some(Token::Underscore)
                    }
                }
                '|' => {
                    if let Some('|') = self.s.peek() {
                        self.s.next();
                        Some(Token::DoubleBar)
                    } else {
                        self.next_token()
                    }
                }
                '~' => Some(Token::Tilde),
                '`' => Some(Token::Tick),
                '*' => Some(Token::Star),
                '[' => Some(Token::LSBracket),
                ']' => Some(Token::RSBracket),
                '(' => Some(Token::LParen),
                ')' => Some(Token::RParen),
                '{' => Some(Token::LCurly),
                '}' => Some(Token::RCurly),
                '<' => Some(Token::LTBracket),
                '>' => Some(Token::RTBracket),
                ',' => Some(Token::Comma),
                _ => {
                    self.rawbuf.push(char);
                    if let Some(c) = self.s.peek() {
                        if is_valid(*c) || (char.is_whitespace() != c.is_whitespace()) {
                            let s = self.rawbuf.clone();
                            self.rawbuf.clear();

                            if char.is_whitespace() {
                                return Some(Token::Whitespace(s));
                            }
                            return Some(Token::Str(s));
                        }
                    } else {
                        let s = self.rawbuf.clone();
                        self.rawbuf.clear();

                        if char.is_whitespace() {
                            return Some(Token::Whitespace(s));
                        }
                        return Some(Token::Str(s));
                    }
                    self.next_token()
                }
            }
        } else {
            if !self.end {
                self.end = true;
                return Some(Token::Eof);
            }
            None
        }
    }
}

#[derive(Clone)]
pub struct MarkupBuilder {
    entities: Vec<MessageEntity>,
    pub buttons: InlineKeyboardBuilder,
    pub header: Option<Header>,
    pub footer: Option<String>,
    offset: i64,
    text: String,
}

pub fn button_deeplink_key(key: &str) -> String {
    format!("bdlk:{}", key)
}

#[inline(always)]
pub fn rules_deeplink_key(key: &str) -> String {
    format!("dlrules:{}", key)
}

/// Builder for MessageEntity formatting. Generates MessageEntities from either murkdown
/// or manually
impl MarkupBuilder {
    /// Constructs a new empty builder for manual formatting
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            offset: 0,
            text: String::new(),
            buttons: InlineKeyboardBuilder::default(),
            header: None,
            footer: None,
        }
    }

    async fn rules<'a>(&'a mut self, chatuser: Option<&'a ChatUser<'a>>) -> Result<()> {
        if let Some(ref chatuser) = chatuser {
            let url = post_deep_link(chatuser.chat.get_id(), |k| rules_deeplink_key(k)).await?;

            let button = InlineKeyboardButtonBuilder::new("Get rules".to_owned())
                .set_url(url)
                .build();
            self.buttons.button(button);
        }
        Ok(())
    }

    pub async fn button<'a, F>(
        &'a mut self,
        hint: String,
        button: String,
        chatuser: Option<&'a ChatUser<'a>>,
        callback: &'a F,
    ) -> Result<()>
    where
        F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
    {
        if let Some(ref chatuser) = chatuser {
            log::info!(
                "building button for note: {} with chat {}",
                button,
                chatuser.chat.name_humanreadable()
            );
        }
        let is_dm = chatuser.map(|v| is_dm(&v.chat)).unwrap_or(true);
        let button = if button.starts_with("#") && button.len() > 1 && is_dm {
            let tail = &button[1..];

            let button = InlineKeyboardButtonBuilder::new(hint)
                .set_callback_data(Uuid::new_v4().to_string())
                .build();

            callback(tail.to_owned(), &button).await?;
            button
        } else if !is_dm && button.starts_with("#") && button.len() > 1 {
            let chat = chatuser.ok_or_else(|| BotError::Generic("missing chatuser".to_owned()))?;
            let chat = chat.chat.get_id();
            let tail = &button[1..];

            let url = post_deep_link((chat, tail), |v| button_deeplink_key(v)).await?;

            InlineKeyboardButtonBuilder::new(hint).set_url(url).build()
        } else {
            InlineKeyboardButtonBuilder::new(hint)
                .set_url(button)
                .build()
        };

        self.buttons.button(button);

        Ok(())
    }

    fn parse_tgspan<'a, F>(
        &'a mut self,
        span: Vec<TgSpan>,
        message: Option<&'a ChatUser<'a>>,
        callback: &'a F,
    ) -> BoxFuture<Result<(i64, i64)>>
    where
        F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
    {
        async move {
            let mut size = 0;
            for span in span {
                match (span, message) {
                    (TgSpan::Code(code), _) => {
                        self.code(&code);
                    }
                    (TgSpan::Italic(s), _) => {
                        let (s, e) = self.parse_tgspan(s, message, callback).await?;
                        size += e;
                        self.manual("italic", s, e);
                    }
                    (TgSpan::Bold(s), _) => {
                        let (s, e) = self.parse_tgspan(s, message, callback).await?;
                        size += e;
                        self.manual("bold", s, e);
                    }
                    (TgSpan::Strikethrough(s), _) => {
                        let (s, e) = self.parse_tgspan(s, message, callback).await?;
                        size += e;
                        self.manual("strikethrough", s, e);
                    }
                    (TgSpan::Underline(s), _) => {
                        let (s, e) = self.parse_tgspan(s, message, callback).await?;
                        size += e;
                        self.manual("underline", s, e);
                    }
                    (TgSpan::Spoiler(s), _) => {
                        let (s, e) = self.parse_tgspan(s, message, callback).await?;
                        size += e;
                        self.manual("spoiler", s, e);
                    }
                    (TgSpan::Button(hint, button), _) => {
                        self.button(hint, button, message, callback).await?;
                    }
                    (TgSpan::NewlineButton(hint, button), _) => {
                        self.buttons.newline();
                        self.button(hint, button, message, callback).await?;
                    }
                    (TgSpan::Link(hint, link), _) => {
                        let (s, e) = self.parse_tgspan(hint, message, callback).await?;
                        size += e;
                        let entity = MessageEntityBuilder::new(s, e)
                            .set_type("text_link".to_owned())
                            .set_url(link)
                            .build();
                        self.entities.push(entity);
                    }
                    (TgSpan::Raw(s), _) => {
                        size += s.encode_utf16().count() as i64;
                        self.text(s);
                    }
                    (TgSpan::Filling(filling), Some(chatuser)) => match filling.as_str() {
                        "username" => {
                            let user = chatuser.user.as_ref().to_owned();
                            let name = user.name_humanreadable();
                            size += name.encode_utf16().count() as i64;
                            self.text_mention(name, user, None);
                        }
                        "first" => {
                            let first = chatuser.user.get_first_name();
                            size += first.encode_utf16().count() as i64;
                            self.text(first);
                        }
                        "last" => {
                            let last = chatuser
                                .user
                                .get_last_name()
                                .map(|v| v.into_owned())
                                .unwrap_or_else(|| "".to_owned());
                            size += last.encode_utf16().count() as i64;
                            self.text(last);
                        }
                        "mention" => {
                            let user = chatuser.user.as_ref().to_owned();
                            let first = user.get_first_name().into_owned();
                            size += first.encode_utf16().count() as i64;
                            self.text_mention(first, user, None);
                        }
                        "chatname" => {
                            let chat = chatuser.chat.name_humanreadable();
                            size += chat.encode_utf16().count() as i64;
                            self.text(chat);
                        }
                        "id" => {
                            let id = chatuser.user.get_id().to_string();
                            size += id.encode_utf16().count() as i64;
                            self.text(id);
                        }
                        "rules" => {
                            self.rules(Some(chatuser)).await?;
                        }
                        s => {
                            let s = format!("{{{}}}", s);
                            size += s.encode_utf16().count() as i64;
                            self.text(s);
                        }
                    },

                    (TgSpan::Filling(filling), _) => {
                        let s = format!("{{{}}}", filling);
                        size += s.encode_utf16().count() as i64;
                        self.text(s);
                    }
                    (TgSpan::NoOp, _) => (),
                };
            }
            let offset = self.offset - size;
            Ok((offset, size))
        }
        .boxed()
    }

    fn parse_listitem(&mut self, list_item: ListItem) {
        match list_item {
            ListItem::Simple(spans) => spans.into_iter().for_each(|i| {
                self.parse_span(i);
            }),
            ListItem::Paragraph(paragraphs) => {
                paragraphs.into_iter().for_each(|i| self.parse_block(i))
            }
        }
    }

    fn parse_block(&mut self, block: Block) {
        match block {
            Block::Header(spans, _) => spans.into_iter().for_each(|s| {
                self.parse_span(s);
            }),
            Block::Paragraph(spans) => spans.into_iter().for_each(|s| {
                self.parse_span(s);
            }),
            Block::Blockquote(blocks) => blocks.into_iter().for_each(|b| self.parse_block(b)),
            Block::CodeBlock(_, s) => {
                self.code(s);
            }
            Block::OrderedList(l, _) => l.into_iter().for_each(|i| self.parse_listitem(i)),
            Block::UnorderedList(l) => l.into_iter().for_each(|i| self.parse_listitem(i)),
            Block::Raw(str) => {
                self.text(str);
            }
            Block::Hr => (),
        };
    }

    fn parse_span(&mut self, span: Span) -> i64 {
        match span {
            Span::Break => {
                let s = "\n";
                self.text(s);
                s.encode_utf16().count() as i64
            }
            Span::Text(text) => {
                let i = text.encode_utf16().count() as i64;
                self.text(text);
                i
            }
            Span::Code(code) => {
                let i = code.encode_utf16().count() as i64;
                self.code(code);
                i
            }
            Span::Link(hint, link, _) => {
                let i = hint.encode_utf16().count() as i64;
                self.text_link(hint, link, None);
                i
            }
            Span::Image(_, _, _) => 0 as i64,
            Span::Emphasis(emp) => {
                let mut size: i64 = 0;
                let start = self.offset;
                emp.into_iter().for_each(|v| {
                    size += self.parse_span(v);
                });
                let bold = MessageEntityBuilder::new(start, size)
                    .set_type("italic".to_owned())
                    .build();
                self.entities.push(bold);
                size
            }

            Span::Strong(emp) => {
                let mut size: i64 = 0;
                let start = self.offset;
                emp.into_iter().for_each(|v| {
                    size += self.parse_span(v);
                });
                let bold = MessageEntityBuilder::new(start, size)
                    .set_type("bold".to_owned())
                    .build();
                self.entities.push(bold);
                size
            }
        }
    }

    /// Parses vanilla markdown and constructs a builder with the corresponding text
    /// and entities
    pub fn from_markdown<T: AsRef<str>>(text: T) -> Self {
        let text = text.as_ref();
        let mut s = Self::new();
        markdown::tokenize(text).into_iter().for_each(|v| {
            s.parse_block(v);
        });
        s
    }

    pub async fn from_tgspan<'a, F>(
        tgspan: Vec<TgSpan>,
        chatuser: Option<&'a ChatUser<'a>>,
        callback: F,
    ) -> Result<Self>
    where
        F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
    {
        let mut s = Self::new();
        s.parse_tgspan(tgspan, chatuser, &callback).await?;
        Ok(s)
    }

    /// Parses murkdown and constructs a builder with the corresponding text and
    /// entities
    pub async fn from_murkdown<T>(text: T) -> Result<Self>
    where
        T: AsRef<str>,
    {
        Self::from_murkdown_internal(text, None, &|_, _| async move { Ok(()) }.boxed()).await
    }

    /// Parses murkdown and constructs a builder with the corresponding text and
    /// entities. The provided ChatUser value is used to perform automated formfilling
    pub async fn from_murkdown_chatuser<'a, T>(
        text: T,
        chatuser: Option<&'a ChatUser<'a>>,
    ) -> Result<Self>
    where
        T: AsRef<str>,
    {
        Self::from_murkdown_internal(text, chatuser, &|_, _| async move { Ok(()) }.boxed()).await
    }

    /// parses murkdown with a callback called on every button requiring a callback
    pub async fn from_murkdown_button<'a, T, F>(
        text: T,
        chatuser: Option<&'a ChatUser<'a>>,
        callback: &F,
    ) -> Result<Self>
    where
        T: AsRef<str>,
        F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
    {
        Self::from_murkdown_internal(text, chatuser, callback).await
    }

    async fn from_murkdown_internal<'a, T, F>(
        text: T,
        chatuser: Option<&'a ChatUser<'a>>,
        callback: &F,
    ) -> Result<Self>
    where
        T: AsRef<str>,
        F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
    {
        let text = text.as_ref();
        let mut s = Self::new();
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(text);
        while let Some(token) = tokenizer.next_token() {
            log::info!("parsing token {:?}", token);
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        s.header = res.header;
        s.footer = res.body.iter().rev().find_map(|p| {
            if let TgSpan::Filling(f) = p {
                Some(f.to_owned())
            } else {
                None
            }
        });
        s.parse_tgspan(res.body, chatuser, &callback).await?;
        Ok(s)
    }

    /// Appends new unformated text
    pub fn text<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.text.push_str(text.as_ref());
        self.offset += text.as_ref().encode_utf16().count() as i64;
        self
    }

    fn manual(&mut self, entity_type: &str, start: i64, end: i64) {
        let entity = MessageEntityBuilder::new(start, end)
            .set_type(entity_type.to_owned())
            .build();
        self.entities.push(entity);
    }

    /// Appends a markup value
    pub fn regular<'a, T: AsRef<str>>(&'a mut self, entity_type: Markup<T>) -> &'a mut Self {
        let n = entity_type.get_text().encode_utf16().count() as i64;

        self.text.push_str(entity_type.get_text());
        match entity_type.markup_type {
            MarkupType::Text => {}
            _ => {
                let entity = MessageEntityBuilder::new(self.offset, n)
                    .set_type(entity_type.get_type().to_owned());
                let entity = match entity_type.markup_type {
                    MarkupType::TextLink(link) => entity.set_url(link),
                    MarkupType::TextMention(mention) => entity.set_user(mention),
                    MarkupType::Pre(pre) => entity.set_language(pre),
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

    /// Appends a new text link. Pass a number for advance to allow text/formatting overlap
    pub fn text_link<'a, T: AsRef<str>>(
        &'a mut self,
        text: T,
        link: String,
        advance: Option<i64>,
    ) -> &'a mut Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("text_link".to_owned())
            .set_url(link)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    /// Appends a new text mention. Pass a number for advance to allow text/formatting overlap
    pub fn text_mention<'a, T: AsRef<str>>(
        &'a mut self,
        text: T,
        mention: User,
        advance: Option<i64>,
    ) -> &'a Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("text_mention".to_owned())
            .set_user(mention)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    /// Appends a new pre block. Pass a number for advance to allow text/formatting overlap
    pub fn pre<'a, T: AsRef<str>>(
        &'a mut self,
        text: T,
        language: String,
        advance: Option<i64>,
    ) -> &'a Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("pre".to_owned())
            .set_language(language)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    /// Appends a new custom emoji. Pass a number for advance to allow text/formatting overlap
    pub fn custom_emoji<'a, T: AsRef<str>>(
        &'a mut self,
        text: T,
        emoji_id: String,
        advance: Option<i64>,
    ) -> &'a Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("custom_emoji".to_owned())
            .set_custom_emoji_id(emoji_id)
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    /// Appends strikethrouh text. Pass a number for advance to allow text/formatting overlap
    pub fn strikethrough<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::StrikeThrough.text(&text))
    }

    /// Appends a new hashtag. Pass a number for advance to allow text/formatting overlap
    pub fn hashtag<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::HashTag.text(&text))
    }

    /// Appends a new cashtag. Pass a number for advance to allow text/formatting overlap
    pub fn cashtag<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::CashTag.text(&text))
    }

    /// Appends a new bot command. Pass a number for advance to allow text/formatting overlap
    pub fn bot_command<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::BotCommand.text(&text))
    }

    /// Appends a new email. Pass a number for advance to allow text/formatting overlap
    pub fn email<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Email.text(&text))
    }

    /// Appends a new phone number. Pass a number for advance to allow text/formatting overlap
    pub fn phone_number<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::PhoneNumber.text(&text))
    }

    /// Appends bold text. Pass a number for advance to allow text/formatting overlap
    pub fn bold<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Bold.text(&text))
    }

    /// Appends a italic text. Pass a number for advance to allow text/formatting overlap
    pub fn italic<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Italic.text(&text))
    }

    /// Appends underline text. Pass a number for advance to allow text/formatting overlap
    pub fn underline<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Underline.text(&text))
    }

    /// Appends spoiler text. Pass a number for advance to allow text/formatting overlap
    pub fn spoiler<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Spoiler.text(&text))
    }

    /// Appends a formatted code block. Pass a number for advance to allow text/formatting overlap
    pub fn code<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Code.text(&text))
    }

    /// Appends a new mention. Pass a number for advance to allow text/formatting overlap
    pub fn mention<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(MarkupType::Mention.text(&text))
    }

    /// shortcut for adding whitespace
    pub fn s<'a>(&'a mut self) -> &'a mut Self {
        let t = " ";
        let count = t.encode_utf16().count() as i64;
        self.offset += count;
        self.text.push_str(t);
        self
    }

    /// return references to message text and MessageEntities
    pub fn build<'a>(&'a self) -> (&'a str, &'a Vec<MessageEntity>) {
        (&self.text, &self.entities)
    }

    /// Consume this builder and return owned text and MessageEntities in Vec form
    pub fn build_owned(self) -> (String, Vec<MessageEntity>, InlineKeyboardMarkup) {
        (self.text, self.entities, self.buttons.build())
    }

    pub fn build_filter(
        self,
    ) -> (
        String,
        Vec<MessageEntity>,
        InlineKeyboardMarkup,
        Option<Header>,
        Option<String>,
    ) {
        (
            self.text,
            self.entities,
            self.buttons.build(),
            self.header,
            self.footer,
        )
    }
}

lazy_static! {
    static ref FILLER_REGEX: Regex = Regex::new(r"{\w*}").unwrap();
}

pub fn retro_fillings(
    text: &str,
    entities: Vec<MessageEntity>,
    chatuser: &ChatUser,
) -> (String, Vec<MessageEntity>) {
    let mut res = String::with_capacity(text.len());
    let mut offsets = entities
        .iter()
        .map(|v| (v.get_offset(), v.get_length()))
        .collect::<Vec<(i64, i64)>>();
    while let Some(mat) = FILLER_REGEX.find(text) {
        let filling = &mat.as_str()[1..mat.len() - 1];

        let (text, entity) = match filling {
            "username" => {
                let user = chatuser.user.as_ref().to_owned();
                let name = user.name_humanreadable();
                let pos = text.len() as i64;
                let len = name.len() as i64;
                (
                    name,
                    Some(MessageEntityBuilder::new(pos, len).set_user(user).build()),
                )
            }
            "first" => {
                let first = chatuser.user.get_first_name().into_owned();
                (first, None)
            }
            "last" => {
                let last = chatuser
                    .user
                    .get_last_name()
                    .map(|v| v.into_owned())
                    .unwrap_or_else(|| "".to_owned());
                (last, None)
            }
            "mention" => {
                let user = chatuser.user.as_ref().to_owned();
                let first = user.get_first_name().into_owned();

                let pos = text.len() as i64;
                let len = first.len() as i64;
                (
                    first,
                    Some(MessageEntityBuilder::new(pos, len).set_user(user).build()),
                )
            }
            "chatname" => {
                let chat = chatuser.chat.name_humanreadable();
                (chat, None)
            }
            "id" => {
                let id = chatuser.user.get_id().to_string();
                (id, None)
            } // TODO: handle rules filler
            s => {
                let s = format!("{{{}}}", s);
                (s, None)
            }
        };

        res.push_str(&text);
        if let Some(entity) = entity {
            for v in offsets.as_mut_slice() {
                if v.0 >= entity.get_offset() {
                    v.0 += entity.get_length();
                }
            }
        }
    }
    let newoffsets = entities
        .into_iter()
        .zip(offsets)
        .map(|(entity, (off, len))| {
            let mut builder =
                MessageEntityBuilder::new(off, len).set_type(entity.get_tg_type().into_owned());
            if let Some(v) = entity.get_url() {
                builder = builder.set_url(v.into_owned());
            }

            if let Some(v) = entity.get_user() {
                builder = builder.set_user(v.into_owned());
            }

            if let Some(v) = entity.get_language() {
                builder = builder.set_language(v.into_owned());
            }

            if let Some(v) = entity.get_custom_emoji_id() {
                builder = builder.set_custom_emoji_id(v.into_owned());
            }
            builder.build()
        })
        .collect::<Vec<MessageEntity>>();
    (res, newoffsets)
}

/// Represents metadata for a single MessageEntity. Useful when programatically
/// constructing formatted text using MarkupBuilder
pub struct Markup<T: AsRef<str>> {
    markup_type: MarkupType,
    text: T,
    advance: Option<i64>,
}

/// Enum with varients for every kind of MessageEntity
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
    TextLink(String),
    TextMention(User),
    Pre(String),
    CustomEmoji(String),
}

impl<T> From<T> for Markup<T>
where
    T: AsRef<str>,
{
    fn from(value: T) -> Self {
        MarkupType::Text.text(value)
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
            MarkupType::CustomEmoji(_) => "custom_emoji",
            MarkupType::StrikeThrough => "strikethrough",
            MarkupType::HashTag => "hashtag",
            MarkupType::CashTag => "cashtag",
            MarkupType::BotCommand => "botcommand",
            MarkupType::Email => "email",
            MarkupType::PhoneNumber => "phone_number",
            MarkupType::Bold => "bold",
            MarkupType::Italic => "italic",
            MarkupType::Underline => "underline",
            MarkupType::Spoiler => "spoiler",
            MarkupType::Code => "code",
            MarkupType::Mention => "mention",
        }
    }

    /// gets the unformatted text for this markup
    fn get_text<'a>(&'a self) -> &'a str {
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
    builder: MarkupBuilder,
    chat: i64,
    reply_markup: Option<EReplyMarkup>,
}

impl EntityMessage {
    pub fn new(chat: i64) -> Self {
        Self {
            builder: MarkupBuilder::new(),
            chat,
            reply_markup: None,
        }
    }

    pub fn reply_markup(mut self, reply_markup: EReplyMarkup) -> Self {
        self.reply_markup = Some(reply_markup);
        self
    }

    pub fn builder<'a>(&'a mut self) -> &'a mut MarkupBuilder {
        &mut self.builder
    }
    pub fn call<'a>(&'a mut self) -> CallSendMessage<'a> {
        let (text, entities) = self.builder.build();
        let call = TG
            .client
            .build_send_message(self.chat, text)
            .entities(entities);
        if let Some(ref reply_markup) = self.reply_markup {
            call.reply_markup(reply_markup)
        } else {
            call
        }
    }

    pub fn textentities<'a>(&'a self) -> (&'a str, &'a Vec<MessageEntity>) {
        self.builder.build()
    }
}

#[allow(dead_code)]
mod test {
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
        let mut tokenizer = Lexer::new(markdown);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap()
    }

    #[test]
    fn button() {
        let mut tokenizer = Lexer::new("<button>(https://example.com)");
        let mut parser = Parser::new();
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap();
    }

    #[test]
    fn tokenize_test() {
        let mut tokenizer = Lexer::new(MARKDOWN_SIMPLE);
        if let Some(Token::LSBracket) = tokenizer.next_token() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Star) = tokenizer.next_token() {
        } else {
            panic!("got invalid token");
        }

        for c in ['b', 'o', 'l', 'd'] {
            if let Some(Token::RawChar(s)) = tokenizer.next_token() {
                assert_eq!(s, c);
            } else {
                panic!("got invalid token");
            }
        }

        if let Some(Token::RSBracket) = tokenizer.next_token() {
        } else {
            panic!("got invalid token");
        }

        if let Some(_) = tokenizer.next_token() {
            assert!(false);
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
        let tokens = test_parse(MARKDOWN_TEST);
        let mut counter = 0;
        for token in tokens {
            if let TgSpan::Raw(raw) = token {
                println!("RAW {}", raw);
                counter += 1;
            }
        }
        assert_eq!(counter, 3);
    }

    #[test]
    fn raw_test() {
        if let Some(TgSpan::Raw(res)) = test_parse(RAW).get(0) {
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
    fn escape() {
        if let Some(TgSpan::Raw(res)) = test_parse(ESCAPE).get(0) {
            assert_eq!(res, ESCAPE.replace("\\", "").as_str());
        } else {
            panic!("failed to parse");
        }
    }

    #[test]
    fn markup_builder() {
        let p = test_parse(MARKDOWN_TEST);
        let mut b = MarkupBuilder::new();
        b.parse_tgspan(p, None, &|_, _| async move { Ok(()) });
        assert_eq!(b.entities.len(), 2);
        println!("{}", b.text);
    }
}
