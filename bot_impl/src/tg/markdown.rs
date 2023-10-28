//! Various tools for generating formatted telegram text using MessageEntities
//! There are two APIs here, a markdown like "murkdown" formatting language and
//! a builder api for manually generating formatted text

use crate::persist::core::button;
use crate::statics::TG;
use crate::util::error::{BotError, Result};
use botapi::gen_methods::CallSendMessage;
use botapi::gen_types::{
    Chat, EReplyMarkup, InlineKeyboardButton, InlineKeyboardButtonBuilder, MessageEntity,
    MessageEntityBuilder, User,
};
use futures::future::BoxFuture;
use futures::FutureExt;
use itertools::Itertools;
use lazy_static::lazy_static;
use markdown::{Block, ListItem, Span};
use pomelo::pomelo;
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;
use std::sync::Arc;
use std::{iter::Peekable, str::Chars};
use thiserror::Error;
use uuid::Uuid;

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
    %type wstr String;

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
    main     ::= Whitespace(_) words?(A) { A.unwrap_or_else(Vec::new) }


        //main     ::= words?(A) { A.unwrap_or_else(Vec::new) }



    words    ::= words(mut L) Whitespace(S) word(W) { L.push(super::TgSpan::Raw(S)); L.push(W); L }
    words    ::= words(mut L) word(W) { L.push(W); L }
    words    ::= word(C) { vec![C] }
    word      ::= Str(S) { super::TgSpan::Raw(S) }
    word      ::= LCurly wstr(W) RCurly { super::TgSpan::Filling(W) }
    word      ::= LSBracket Tick wstr(W) RSBracket { super::TgSpan::Code(W) }
    word      ::= LSBracket Star main(S) RSBracket { super::TgSpan::Bold(S) }
    word      ::= LSBracket main(H) RSBracket LParen Str(L) RParen { super::TgSpan::Link(H, L) }
    word      ::= LSBracket Tilde main(R) RSBracket { super::TgSpan::Strikethrough(R) }
    word      ::= LSBracket Underscore main(R) RSBracket { super::TgSpan::Italic(R) }
    word      ::= LSBracket DoubleUnderscore main(R) RSBracket { super::TgSpan::Underline(R) }
    word      ::= LSBracket DoubleBar main(R) RSBracket { super::TgSpan::Spoiler(R) }
    word      ::= LTBracket wstr(W) RTBracket LParen wstr(L) RParen { super::TgSpan::Button(W, L) }
    word      ::= LTBracket LTBracket wstr(W) RTBracket RTBracket LParen wstr(L) RParen { super::TgSpan::NewlineButton(W, L) }

    wstr      ::= Str(S) { S }
    wstr      ::= Str(S) Whitespace(W) wstr(mut L){ L.push_str(&S); L.push_str(&W); L}


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
    header: bool,
}

fn is_valid(token: char, header: bool) -> bool {
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
        ',' if header => true,
        '"' if header => true,
        _ => false,
    }
}

fn is_valid_rose(token: &str) -> bool {
    match token {
        "`" => true,
        "```" => true,
        "_" => true,
        "__" => true,
        "~" => true,
        "*" => true,
        "|" => true,
        "||" => true,
        "!" => true,
        "[" => true,
        "]" => true,
        "(" => true,
        ")" => true,
        "\\" => true,
        _ => false,
    }
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str, header: bool) -> Self {
        let chars = input.chars().peekable();
        Self {
            s: chars,
            rawbuf: String::new(),
            pos: 0,
            end: false,
            header,
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        self.pos += 1;
        if self.pos == 1 {
            if self.header {
                return Some(Token::Start);
            }
        }
        if let Some(char) = self.s.next() {
            //     log::info!("parsing {}", char);
            match char {
                '\\' => self.s.next().map(|char| Token::Str(char.to_string())),
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
                ',' if self.header => Some(Token::Comma),
                '"' if self.header => Some(Token::Quote),
                _ => {
                    self.rawbuf.push(char);
                    if let Some(c) = self.s.peek() {
                        if is_valid(*c, self.header) || (char.is_whitespace() != c.is_whitespace())
                        {
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

pub type ButtonFn = Arc<
    dyn for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
>;

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
            user: value.user.to_owned().into_owned(),
            chat: value.chat.to_owned().into_owned(),
        }
    }
}

impl<'a> From<&'a ChatUser<'a>> for OwnedChatUser {
    fn from(value: &'a ChatUser<'a>) -> Self {
        Self {
            user: value.user.to_owned().into_owned(),
            chat: value.chat.to_owned().into_owned(),
        }
    }
}

#[derive(Clone)]
pub struct MarkupBuilder {
    existing_entities: Option<Vec<MessageEntity>>,
    pub entities: Vec<MessageEntity>,
    pub buttons: InlineKeyboardBuilder,
    pub header: Option<Header>,
    pub footer: Option<String>,
    pub offset: i64,
    pub text: String,
    pub filling: bool,
    pub enabled_header: bool,
    button_function: ButtonFn,
    chatuser: Option<OwnedChatUser>,
    pub built_markup: Option<EReplyMarkup>,
}

pub fn button_deeplink_key(key: &str) -> String {
    format!("bdlk:{}", key)
}

#[inline(always)]
pub fn rules_deeplink_key(key: &str) -> String {
    format!("dlrules:{}", key)
}

pub fn get_markup_for_buttons(button: Vec<button::Model>) -> Option<InlineKeyboardBuilder> {
    if button.len() == 0 {
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

trait IntoUtf16Chars {
    fn into_utf16_chars(&self) -> Vec<char>;
}

impl IntoUtf16Chars for &str {
    fn into_utf16_chars(&self) -> Vec<char> {
        self.encode_utf16()
            .map(|v| v.into())
            .map(|v| char::from_u32(v).unwrap())
            .collect()
    }
}

impl IntoUtf16Chars for String {
    fn into_utf16_chars(&self) -> Vec<char> {
        self.encode_utf16()
            .map(|v| v.into())
            .map(|v| char::from_u32(v).unwrap())
            .collect()
    }
}

fn string_index(chars: &[char], idx: &str) -> Option<usize> {
    let idx = idx.into_utf16_chars();
    chars
        .iter()
        .enumerate()
        .find(|(x, _)| (&chars[*x..]).starts_with(idx.as_slice()))
        .map(|(v, _)| v)
}

fn valid_start(chars: &[char], pos: usize) -> bool {
    let r = (pos == 0 || !chars[pos - 1].is_alphanumeric())
        && !(pos == chars.len() - 1 || chars[pos + 1].is_whitespace());
    // println!("valid_start {}", r);
    r
}

fn valid_end(chars: &[char], pos: usize) -> bool {
    let r = !(pos == 0 || chars[pos - 1].is_whitespace())
        && (pos == chars.len() - 1 || !chars[pos + 1].is_alphanumeric());
    // println!("valid_end {}", r);
    r
}

fn is_escaped(chars: &[char], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }

    let mut i = pos - 1;
    for (x, ch) in chars[0..i + 1].iter().enumerate().rev() {
        i = x;
        if *ch == '\\' {
            continue;
        }
        break;
    }
    let r = (pos - i) % 2 == 0;
    // println!("is_escaped {}", r);
    r
}

fn get_valid_end(chars: &[char], item: &str) -> Option<usize> {
    // println!("get_valid_end {}", item);
    let mut offset = 0;
    while offset < chars.len() {
        if let Some(idx) = string_index(&chars[offset..], item) {
            // println!("get_valid_end string_index {}", idx);
            let mut end = offset + idx;
            if valid_end(chars, end)
                && valid_end(chars, end + item.encode_utf16().count() - 1)
                && !is_escaped(chars, end)
            {
                // println!("get_valid_end got");
                let mut idx = string_index(&chars[end + 1..], item);
                while let Some(0) = idx {
                    end += 1;
                    idx = string_index(&chars[end + 1..], item);
                }
                return Some(end);
            }
            offset = end + 1;
        } else {
            return None;
        }
    }

    None
}

fn get_valid_link_end(chars: &[char]) -> Option<usize> {
    let mut offset = 0;

    while offset < chars.len() {
        if let Some(idx) = chars[offset..].iter().position(|v| *v == ')') {
            let end = offset + idx;
            if valid_end(chars, end) && !is_escaped(chars, end) {
                return Some(end);
            }
            offset = end + 1;
        } else {
            return None;
        }
    }

    None
}

fn find_link_sections_idx(chars: &[char]) -> Option<(usize, usize)> {
    let mut text_end = 0;
    let mut link_end = 0;
    let mut offset = 0;
    let mut found_text_end = false;

    while offset < chars.len() {
        if let Some(idx) = string_index(&chars[offset..], "](") {
            text_end = offset + idx;
            if !is_escaped(chars, text_end) {
                found_text_end = true;
                break;
            }
            offset = text_end + 1;
        } else {
            return None;
        }

        if !found_text_end {
            return None;
        }
    }

    offset = text_end;
    while offset < chars.len() {
        if let Some(idx) = get_valid_link_end(&chars[offset..]) {
            link_end = offset + idx;
            if !is_escaped(chars, link_end) {
                return Some((text_end, link_end));
            }
            offset = link_end + 1;
        } else {
            return None;
        }
    }
    None
}

fn get_link_contents<'a>(chars: &'a [char]) -> Option<(&'a [char], String, usize)> {
    if let Some((link_text, link_url)) = find_link_sections_idx(chars) {
        let content: String = chars[link_text + 2..link_url].iter().collect();
        let text = &chars[1..link_text];
        Some((text, content, link_url + 1))
    } else {
        None
    }
}

struct RoseMdParser {
    prefixes: HashMap<String, String>,
    same_line_suffix: String,
    chars: Vec<char>,
    current: HashSet<char>,
}

struct RoseMdDecompiler<'a> {
    out: &'a str,
    entities: BTreeMap<i64, Vec<&'a MessageEntity>>,
    buttons: &'a Vec<InlineKeyboardButton>,
    current: BTreeMap<i64, Vec<&'a MessageEntity>>,
}

impl<'a> RoseMdDecompiler<'a> {
    fn new(
        out: &'a str,
        entities: &'a Vec<MessageEntity>,
        buttons: &'a Vec<InlineKeyboardButton>,
    ) -> Self {
        Self {
            out,
            buttons,
            entities: entities.iter().fold(BTreeMap::new(), |mut acc, value| {
                let v = acc.entry(value.get_offset()).or_insert_with(|| Vec::new());
                v.push(value);
                acc
            }),
            current: BTreeMap::new(),
        }
    }

    fn decompile(mut self) -> String {
        let mut out = String::new();
        for (offset, ch) in self.out.into_utf16_chars().into_iter().enumerate() {
            if let Some(entity) = self.entities.remove(&(offset as i64)) {
                for entity in entity.into_iter().rev() {
                    println!(
                        "match entity: {} {}",
                        entity.get_tg_type_ref(),
                        entity.get_offset()
                    );
                    match entity.get_tg_type_ref() {
                        "spoiler" => out.push_str("||"),
                        "italic" => out.push('_'),
                        "underline" => out.push_str("__"),
                        "bold" => out.push('*'),
                        "strikethrough" => out.push('~'),
                        "code" => out.push('`'),
                        "pre" => out.push_str("```"),
                        "text_link" => out.push('['),
                        _ => (),
                    };

                    let e = self
                        .current
                        .entry(entity.get_offset() + entity.get_length())
                        .or_insert_with(|| Vec::new());
                    e.push(entity);
                }
            }

            out.push(ch);
            // println!("writing {}", offset);
            if let Some(v) = self.current.remove(&((offset + 1) as i64)) {
                for entity in v.into_iter().rev() {
                    match entity.get_tg_type_ref() {
                        "spoiler" => out.push_str("||"),
                        "italic" => out.push('_'),
                        "underline" => out.push_str("__"),
                        "bold" => out.push('*'),
                        "strikethrough" => out.push('~'),
                        "code" => out.push('`'),
                        "pre" => out.push_str("```"),
                        "text_link" => {
                            if let Some(url) = entity.get_url_ref() {
                                out.push_str(&format!("]({})", url));
                            }
                        }
                        _ => (),
                    }
                }
            }
        }
        out
    }
}

impl RoseMdParser {
    fn new(chars: &str) -> Self {
        let mut prefixes = HashMap::with_capacity(1);
        prefixes.insert("url".to_owned(), "buttonurl:".to_owned());
        Self {
            prefixes,
            same_line_suffix: "same".to_owned(),
            chars: chars.into_utf16_chars(),
            current: HashSet::new(),
        }
    }

    fn parse(&self) -> (String, Vec<MessageEntity>, InlineKeyboardBuilder) {
        self.parse_ch(&self.chars, 0)
    }

    fn parse_ch(
        &self,
        chars: &[char],
        offset: i64,
    ) -> (String, Vec<MessageEntity>, InlineKeyboardBuilder) {
        let mut res = Vec::new();
        let mut text = String::new();
        let mut builder = InlineKeyboardBuilder::default();
        let mut i = chars.iter().enumerate();
        while let Some((mut x, ch)) = i.next() {
            let mut ch = *ch;
            // println!("parsing {} {}", x, ch);

            if !is_valid_rose(ch.to_string().as_str()) {
                text.push(ch);
                continue;
            }

            if !valid_start(chars, x) {
                if ch == '\\' && x + 1 < chars.len() {
                    text.push(chars[x + 1]);
                    continue;
                }
                println!("regular {} ", ch);
                text.push(ch);
                continue;
            }

            match ch {
                '`' | '*' | '~' | '|' | '_' => {
                    let mut item = String::from(ch);

                    match ch {
                        '|' => {
                            if x + 1 >= chars.len() || chars[x + 1] != '|' {
                                text.push(ch);
                                continue;
                            }
                            item = "||".to_owned();
                            if let Some((i, c)) = i.next() {
                                x = i;
                                ch = *c;
                            }
                        }
                        '_' if x + 1 < chars.len() && chars[x + 1] == '_' => {
                            item = "__".to_owned();
                            if let Some((i, c)) = i.next() {
                                x = i;
                                ch = *c;
                            }
                        }
                        '`' if x + 2 < chars.len()
                            && chars[x + 1] == '`'
                            && chars[x + 2] == '`' =>
                        {
                            item = "```".to_owned();
                            i.next();

                            if let Some((i, c)) = i.next() {
                                x = i;
                                ch = *c;
                            }
                        }
                        _ => (),
                    }

                    if x + 1 >= chars.len() {
                        text.push_str(&item);
                        continue;
                    }

                    if let Some(idx) = get_valid_end(&chars[x + 1..], &item) {
                        // println!("got valid end idx {}", idx);
                        let start = x + 1;
                        let end = x + idx + 1;

                        let (nested_text, nested_entities, nested_buttons) = if ch == '`' {
                            (
                                chars[start..end].iter().collect(),
                                Vec::new(),
                                InlineKeyboardBuilder::default(),
                            )
                        } else {
                            self.parse_ch(
                                &chars[start..end],
                                offset + text.encode_utf16().count() as i64,
                            )
                        };

                        let b = MessageEntityBuilder::new(
                            offset + text.encode_utf16().count() as i64,
                            nested_text.encode_utf16().count() as i64,
                        );

                        if let Some(entity) = match item.as_str() {
                            "||" => Some(b.set_type("spoiler".to_owned()).build()),
                            "_" => Some(b.set_type("italic".to_owned()).build()),
                            "__" => Some(b.set_type("underline".to_owned()).build()),
                            "*" => Some(b.set_type("bold".to_owned()).build()),
                            "~" => Some(b.set_type("strikethrough".to_owned()).build()),
                            "`" => Some(b.set_type("code".to_owned()).build()),
                            "```" => Some(b.set_type("pre".to_owned()).build()),
                            _ => None,
                        } {
                            println!(
                                "parsed nested {} {} {}",
                                entity.get_tg_type_ref(),
                                x,
                                offset
                            );

                            res.push(entity);
                        }

                        for button in nested_buttons
                            .into_inner()
                            .into_iter()
                            .flat_map(|v| v.into_iter())
                        {
                            builder.button(button.to_button());
                        }

                        let (follow_text, follow_entities, follow_buttons) = self.parse_ch(
                            &chars[end + item.len()..],
                            nested_text.encode_utf16().count() as i64
                                + offset
                                + text.encode_utf16().count() as i64,
                        );

                        for button in follow_buttons
                            .into_inner()
                            .into_iter()
                            .flat_map(|v| v.into_iter())
                        {
                            builder.button(button.to_button());
                        }

                        res.extend_from_slice(&follow_entities);
                        res.extend_from_slice(&nested_entities);
                        let t = format!("{}{}{}", text, nested_text, follow_text);
                        return (t, res, builder);
                    } else {
                        text.push_str(&item);
                        continue;
                    }
                }
                '!' => (),
                '[' => {
                    if let Some((link_text, content, new_end)) = get_link_contents(&chars[x..]) {
                        let end = x + new_end;

                        let (nested_text, nested_entities, nested_buttons) =
                            self.parse_ch(link_text, offset);

                        let (follow_text, follow_entities, follow_buttons) = self.parse_ch(
                            &chars[end..],
                            offset + nested_text.encode_utf16().count() as i64,
                        );

                        //TODO: handle buttons

                        let e = MessageEntityBuilder::new(
                            offset,
                            nested_text.encode_utf16().count() as i64,
                        )
                        .set_type("text_link".to_owned())
                        .set_url(content)
                        .build();

                        res.push(e);
                        res.extend_from_slice(&nested_entities);
                        res.extend_from_slice(&follow_entities);
                        let t = format!("{}{}{}", text, nested_text, follow_text);
                        return (t, res, builder);
                    } else {
                        text.push(ch);
                        continue;
                    }
                }
                ']' | '(' | ')' => {
                    text.push(ch);
                }
                '\\' => {
                    if x + 1 < chars.len() {
                        if is_valid_rose(chars[x + 1].to_string().as_str()) {
                            text.push(chars[x + 1]);
                            i.next();
                            continue;
                        }
                    }
                }
                _ => (),
            }
        }

        (text, res, builder)
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
            text: String::new(),
            buttons: InlineKeyboardBuilder::default(),
            header: None,
            footer: None,
            filling: false,
            enabled_header: false,
            button_function: Arc::new(|_, _| async move { Ok(()) }.boxed()),
            chatuser: None,
            built_markup: None,
        }
    }

    async fn rules<'a>(&'a mut self) -> Result<()> {
        log::info!("adding rules {}", self.chatuser.is_some());
        if let Some(ref chatuser) = self.chatuser {
            let url = post_deep_link(chatuser.chat.get_id(), |k| rules_deeplink_key(k)).await?;

            let button = InlineKeyboardButtonBuilder::new("Get rules".to_owned())
                .set_url(url)
                .build();
            self.buttons.button(button);
        }
        Ok(())
    }

    pub async fn button<'a>(&'a mut self, hint: String, button_text: String) -> Result<()> {
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
        let button = if button_text.starts_with("#") && button_text.len() > 1 && is_dm {
            let tail = &button_text[1..];

            let button = InlineKeyboardButtonBuilder::new(hint)
                .set_callback_data(Uuid::new_v4().to_string())
                .build();

            (*self.button_function)(tail.to_owned(), &button).await?;
            button
        } else if !is_dm && button_text.starts_with("#") && button_text.len() > 1 {
            let chat = self
                .chatuser
                .as_ref()
                .ok_or_else(|| BotError::Generic("missing chatuser".to_owned()))?;
            let chat = chat.chat.get_id();
            let tail = &button_text[1..];

            let url = post_deep_link((chat, tail), |v| button_deeplink_key(v)).await?;

            InlineKeyboardButtonBuilder::new(hint).set_url(url).build()
        } else {
            InlineKeyboardButtonBuilder::new(hint)
                .set_url(button_text.clone())
                .build()
        };

        self.buttons.button_raw(button, Some(button_text));

        Ok(())
    }

    fn parse_tgspan<'a>(
        &'a mut self,
        span: Vec<TgSpan>,
        topplevel: bool,
    ) -> BoxFuture<Result<(i64, i64, i64)>> {
        async move {
            let mut diff = 0;
            let mut size = 0;
            for span in span {
                match (span, self.chatuser.as_ref()) {
                    (TgSpan::Code(code), _) => {
                        self.code(&code);
                    }
                    (TgSpan::Italic(s), _) => {
                        let (s, e, d) = self.parse_tgspan(s, false).await?;
                        diff += d;
                        size += e;
                        self.manual("italic", s, e);
                    }
                    (TgSpan::Bold(s), _) => {
                        let (s, e, d) = self.parse_tgspan(s, false).await?;
                        diff += d;
                        size += e;
                        self.manual("bold", s, e);
                    }
                    (TgSpan::Strikethrough(s), _) => {
                        let (s, e, d) = self.parse_tgspan(s, false).await?;
                        diff += d;
                        size += e;
                        self.manual("strikethrough", s, e);
                    }
                    (TgSpan::Underline(s), _) => {
                        let (s, e, d) = self.parse_tgspan(s, false).await?;
                        diff += d;
                        size += e;
                        self.manual("underline", s, e);
                    }
                    (TgSpan::Spoiler(s), _) => {
                        let (s, e, d) = self.parse_tgspan(s, false).await?;
                        diff += d;
                        size += e;
                        self.manual("spoiler", s, e);
                    }
                    (TgSpan::Button(hint, button), _) => {
                        diff += hint.encode_utf16().count() as i64
                            + button.encode_utf16().count() as i64
                            + "<>()".encode_utf16().count() as i64;
                        self.button(hint, button).await?;
                    }
                    (TgSpan::NewlineButton(hint, button), _) => {
                        diff += hint.encode_utf16().count() as i64
                            + button.encode_utf16().count() as i64
                            + "<>()".encode_utf16().count() as i64;
                        self.buttons.newline();
                        self.button(hint, button).await?;
                    }
                    (TgSpan::Link(hint, link), _) => {
                        let (s, e, d) = self.parse_tgspan(hint, false).await?;
                        diff += d;

                        diff += link.encode_utf16().count() as i64
                            + "[]()".encode_utf16().count() as i64;
                        size += e;
                        let entity = MessageEntityBuilder::new(s, e)
                            .set_type("text_link".to_owned())
                            .set_url(link)
                            .build();
                        self.entities.push(entity);
                    }
                    (TgSpan::Raw(s), _) => {
                        size += s.encode_utf16().count() as i64;
                        diff += s.encode_utf16().count() as i64;
                        self.text(s);
                    }
                    (TgSpan::Filling(filling), Some(ref chatuser)) if self.filling => {
                        match filling.as_str() {
                            "username" => {
                                let user = chatuser.user.clone();
                                let name = user.name_humanreadable();
                                size += name.encode_utf16().count() as i64;
                                self.text_mention(name, user, None);
                            }
                            "first" => {
                                let first = chatuser.user.get_first_name().into_owned();
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
                                let user = chatuser.user.clone();
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
                                self.rules().await?;
                            }
                            s => {
                                let s = format!("{{{}}}", s);
                                diff += s.encode_utf16().count() as i64;
                                size += s.encode_utf16().count() as i64;

                                self.text(s);
                            }
                        }
                    }
                    (TgSpan::Filling(filling), _) => {
                        let s = format!("{{{}}}", filling);
                        size += s.encode_utf16().count() as i64;
                        diff += s.encode_utf16().count() as i64;
                        self.text(s);
                    }
                    (TgSpan::NoOp, _) => (),
                };
            }
            let offset = self.offset - size;

            if let Some(existing_entities) = self.existing_entities.as_mut() {
                if topplevel {
                    for entity in existing_entities.iter_mut() {
                        if entity.get_offset() >= offset {
                            log::info!("patching entity {} {} {}", self.offset, size, diff);
                            entity.set_offset(entity.get_offset() - (diff - size));
                        }
                    }
                }
            }
            Ok((offset, size, diff))
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
        log::info!("parse_span {:?}", span);
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
    pub fn from_markdown<T: AsRef<str>>(text: T, existing: Option<Vec<MessageEntity>>) -> Self {
        let text = text.as_ref();
        let mut s = Self::new(existing);
        markdown::tokenize(text).into_iter().for_each(|v| {
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
        s.parse_tgspan(tgspan, true).await?;
        Ok(s)
    }

    pub async fn append<'a, T>(
        &'a mut self,
        text: T,
        existing: Option<Vec<MessageEntity>>,
    ) -> Result<&'a mut Self>
    where
        T: AsRef<str>,
    {
        let text = text.as_ref();
        if let (Some(existing), Some(ref existingnew)) = (self.existing_entities.as_mut(), existing)
        {
            existing.extend_from_slice(&existingnew);
        }
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(text, false);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        self.parse_tgspan(res.body, true).await?;
        if let Some(ref existing) = self.existing_entities {
            self.entities.extend_from_slice(&existing.as_slice());
        }
        Ok(self)
    }

    pub fn chatuser<'a>(mut self, chatuser: Option<&ChatUser<'a>>) -> Self {
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

    pub fn build<'a>(&'a self) -> (&'a str, &'a Vec<MessageEntity>) {
        (&self.text, &self.entities)
    }

    pub async fn build_murkdown<'a>(
        mut self,
    ) -> Result<(String, Vec<MessageEntity>, InlineKeyboardBuilder)> {
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(&self.text, self.enabled_header);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        self.offset = 0;
        self.text.clear();
        self.parse_tgspan(res.body, true).await?;

        if let Some(ref existing) = self.existing_entities {
            self.entities.extend_from_slice(&existing.as_slice());
        }
        Ok((self.text, self.entities, self.buttons))
    }

    async fn nofail_internal(&mut self) -> Result<()> {
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(&self.text, self.enabled_header);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        self.offset = 0;
        self.text.clear();
        self.parse_tgspan(res.body, true).await?;

        if let Some(ref existing) = self.existing_entities {
            self.entities.extend_from_slice(&existing.as_slice());
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

    pub async fn build_murkdown_nofail_ref<'a>(
        &'a mut self,
    ) -> (
        &'a mut String,
        &'a mut Vec<MessageEntity>,
        Option<&'a mut EReplyMarkup>,
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

    /// Appends new unformated text
    pub fn text<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.text.push_str(text.as_ref());
        self.offset += text.as_ref().encode_utf16().count() as i64;
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

    pub async fn build_filter(
        mut self,
    ) -> (
        String,
        Vec<MessageEntity>,
        InlineKeyboardBuilder,
        Option<Header>,
        Option<String>,
    ) {
        if let Ok(()) = self.nofail_internal().await {
            (
                self.text,
                self.entities,
                self.buttons,
                self.header,
                self.footer,
            )
        } else {
            (
                self.text,
                self.entities,
                self.buttons,
                self.header,
                self.footer,
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
    if iter.len() == 0 {
        return Ok((text, entities));
    }
    for mat in iter {
        let filling = &mat.as_str()[1..mat.len() - 1];
        let regular = &text[prev..mat.start()];
        res.push_str(regular);
        pos += regular.encode_utf16().count() as i64 + 1;
        prev = mat.end();
        log::info!("matching {}: {}", filling, pos);
        let (text, entity) = match filling {
            "username" => {
                let user = chatuser.user.as_ref().to_owned();
                let name = user.name_humanreadable();
                let start = pos;
                let len = name.encode_utf16().count() as i64;
                pos += len;
                (
                    name,
                    Some(
                        MessageEntityBuilder::new(start, len)
                            .set_type("text_mention".to_owned())
                            .set_user(user)
                            .build(),
                    ),
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
                let start = pos;
                let len = first.encode_utf16().count() as i64;
                pos += len;
                (
                    first,
                    Some(
                        MessageEntityBuilder::new(start, len)
                            .set_type("text_mention".to_owned())
                            .set_user(user)
                            .build(),
                    ),
                )
            }
            "chatname" => {
                let chat = chatuser.chat.name_humanreadable();
                (chat, None)
            }
            "rules" => {
                if let Some(buttons) = buttons.as_mut() {
                    let url =
                        post_deep_link(chatuser.chat.get_id(), |k| rules_deeplink_key(k)).await?;

                    let button = InlineKeyboardButtonBuilder::new("Get rules".to_owned())
                        .set_url(url)
                        .build();
                    buttons.button(button);

                    ("".to_owned(), None)
                } else {
                    ("{rules}".to_owned(), None)
                }
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

        let diff = text.encode_utf16().count() as i64 - mat.as_str().encode_utf16().count() as i64;
        res.push_str(&text);

        for v in offsets.as_mut_slice() {
            if v.0 >= pos - text.encode_utf16().count() as i64 {
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
    Ok((res, newoffsets))
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
    Url,
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

    pub fn reply_markup(mut self, reply_markup: EReplyMarkup) -> Self {
        self.reply_markup = Some(reply_markup);
        self
    }

    pub fn disable_murkdown(mut self, disable: bool) -> Self {
        self.disable_murkdown = disable;
        self
    }

    pub async fn call<'a>(&'a mut self) -> CallSendMessage<'a> {
        if self.disable_murkdown {
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
            if let Some(EReplyMarkup::InlineKeyboardMarkup(ref buttons)) = buttons {
                log::info!(
                    "call {} {}",
                    buttons.get_inline_keyboard_ref().len(),
                    self.reply_markup.is_some()
                );
            }
            let call = TG
                .client
                .build_send_message(self.chat, text)
                .entities(entities);
            if let Some(ref reply_markup) = self.reply_markup {
                call.reply_markup(reply_markup)
            } else if let Some(buttons) = buttons {
                call.reply_markup(buttons)
            } else {
                call
            }
        }
    }

    pub fn textentities<'a>(&'a self) -> (&'a str, &'a Vec<MessageEntity>) {
        (&self.builder.text, &self.builder.entities)
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
        let mut tokenizer = Lexer::new(markdown, false);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap()
    }

    #[test]
    fn button() {
        let mut tokenizer = Lexer::new("<button>(https://example.com)", false);
        let mut parser = Parser::new();
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap();
    }

    #[test]
    fn tokenize_test() {
        let mut tokenizer = Lexer::new(MARKDOWN_SIMPLE, false);
        if let Some(Token::LSBracket) = tokenizer.next_token() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Star) = tokenizer.next_token() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Str(s)) = tokenizer.next_token() {
            assert_eq!(s, "bold");
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::RSBracket) = tokenizer.next_token() {
        } else {
            panic!("got invalid token");
        }

        if let Some(Token::Eof) = tokenizer.next_token() {
        } else {
            panic!("Missing Eof");
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
        let tokens = test_parse(MARKDOWN_TEST).body;
        let mut counter = 0;
        for token in tokens {
            if let TgSpan::Raw(raw) = token {
                println!("RAW {}", raw);
                counter += 1;
            }
        }
        assert_eq!(counter, 6);
    }

    #[test]
    fn raw_test() {
        if let Some(TgSpan::Raw(res)) = test_parse(RAW).body.get(0) {
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
        if let [TgSpan::Raw(ref res), TgSpan::Raw(ref ws), TgSpan::Raw(ref res2)] =
            test_parse(ESCAPE).body.as_slice()[0..]
        {
            let mut r = String::new();
            r.push_str(&res);
            r.push_str(&ws);
            r.push_str(&res2);
            assert_eq!(r, ESCAPE.replace("\\", "").as_str());
        } else {
            panic!("failed to parse");
        }
    }

    #[test]
    fn into_utf16_chars() {
        let v = "im so many sads";
        for (v, n) in v.chars().zip(v.into_utf16_chars()) {
            assert_eq!(v, n);
        }
    }

    #[test]
    fn parse_bold() {
        let t = "*bold*";
        let md = RoseMdParser::new(t);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "bold");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");
    }

    #[test]
    fn parse_link() {
        let t = "[thing](https://example.com)";
        let md = RoseMdParser::new(t);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "thing");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "text_link");
    }

    #[test]
    fn decompile_link() {
        let t = "[thing](https://example.com)";
        let md = RoseMdParser::new(t);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "thing");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "text_link");

        let v = Vec::new();
        let mut decompiler = RoseMdDecompiler::new(&text, &entities, &v);
        let out = decompiler.decompile();

        assert_eq!(out, t);
    }

    #[test]
    fn parse_many() {
        let t = "*bold* ~strike~ *||boldspoiler||*";
        let md = RoseMdParser::new(t);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "bold strike boldspoiler");
        assert_eq!(entities.len(), 4);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");
    }

    #[test]
    fn decompile_bold() {
        let t = "*bold*";
        let md = RoseMdParser::new(t);
        let (text, entities, buttons) = md.parse();

        assert_eq!(text, "bold");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");
        let v = Vec::new();
        let mut decompiler = RoseMdDecompiler::new(&text, &entities, &v);
        let out = decompiler.decompile();

        assert_eq!(out, t);
    }

    #[test]
    fn decompile_many() {
        let t = "*bold* ~strike~ *||boldspoiler||*";
        let t_rev = "*bold* ~strike~ ||*boldspoiler*||";
        let md = RoseMdParser::new(t);
        let (text, entities, buttons) = md.parse();
        println!("got entities {:?}", entities);
        assert_eq!(text, "bold strike boldspoiler");
        assert_eq!(entities.len(), 4);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");

        let v = Vec::new();
        let mut decompiler = RoseMdDecompiler::new(&text, &entities, &v);
        let out = decompiler.decompile();

        assert!(out == t || out == t_rev);
    }
}
