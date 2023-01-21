use botapi::gen_types::{MessageEntity, MessageEntityBuilder, User};
use markdown::{Block, ListItem, Span};

use crate::util::error::Result;
use lazy_static::lazy_static;
use pomelo::pomelo;
use regex::Regex;
use std::fmt::Display;
use std::{iter::Peekable, str::Chars};
use thiserror::Error;
pub struct MarkupBuilder {
    entities: Vec<MessageEntity>,
    offset: i64,
    text: String,
}

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

pub enum TgSpan {
    Code(String),
    Italic(Vec<TgSpan>),
    Bold(Vec<TgSpan>),
    Strikethrough(Vec<TgSpan>),
    Underline(Vec<TgSpan>),
    Spoiler(Vec<TgSpan>),
    Link(Vec<TgSpan>, String),
    Raw(String),
}

lazy_static! {
    static ref RAWSTR: Regex = Regex::new(r#"([^\s"]+|")"#).unwrap();
}

pomelo! {
    %error super::DefaultParseErr;
    %type input Vec<super::TgSpan>;
    %type words Vec<super::TgSpan>;
    %type main Vec<super::TgSpan>;
    %type word super::TgSpan;
    %type wordraw (super::TgSpan, super::TgSpan);
    %type RawChar char;
    %type raw String;

    input     ::= main(A) { A }

    main     ::= words?(A) { A.unwrap_or_else(Vec::new) }

    wordraw  ::= word(W) raw(R) { (W, super::TgSpan::Raw(R)) }

    words    ::=  words(mut L) word(C) { L.push(C); L }
    words    ::= words(mut L) wordraw(W) { L.push(W.0); L.push(W.1); L }
    words    ::= wordraw(W) { vec![W.0, W.1] }
    words    ::= word(C) { vec![C] }
    words    ::= raw(R) { vec![super::TgSpan::Raw(R)]}

    raw       ::= RawChar(C) { C.into() }
    raw       ::= raw(mut R) RawChar(C) { R.push(C); R }

    word      ::= LSBracket Tick raw(W) RSBracket { super::TgSpan::Code(W) }
    word      ::= LSBracket Star main(S) RSBracket { super::TgSpan::Bold(S) }
    word      ::= LSBracket main(H) RSBracket LParen raw(L) RParen { super::TgSpan::Link(H, L) }
    word      ::= LSBracket Tilde words(R) RSBracket { super::TgSpan::Strikethrough(R) }
    word      ::= LSBracket Underscore main(R) RSBracket { super::TgSpan::Italic(R) }
    word      ::= LSBracket DoubleUnderscore main(R) RSBracket { super::TgSpan::Underline(R) }
    word      ::= LSBracket DoubleBar main(R) RSBracket { super::TgSpan::Spoiler(R) }
}

use parser::{Parser, Token};

struct Lexer<'a>(Peekable<Chars<'a>>);

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        let chars = input.chars().peekable();
        Self(chars)
    }

    fn next_token(&mut self) -> Option<Token> {
        if let Some(char) = self.0.next() {
            match char {
                '\\' => self.0.next().map(|char| Token::RawChar(char)),
                '_' => {
                    if let Some('_') = self.0.peek() {
                        self.0.next();
                        Some(Token::DoubleUnderscore)
                    } else {
                        Some(Token::Underscore)
                    }
                }
                '|' => {
                    if let Some('|') = self.0.peek() {
                        self.0.next();
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
                _ => Some(Token::RawChar(char)),
            }
        } else {
            None
        }
    }
}

#[allow(dead_code)]
impl MarkupBuilder {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            offset: 0,
            text: String::new(),
        }
    }

    fn parse_tgspan(&mut self, span: Vec<TgSpan>) -> (i64, i64) {
        let mut size = 0;
        for span in span {
            match span {
                TgSpan::Code(code) => {
                    self.code(&code);
                }
                TgSpan::Italic(s) => {
                    let (s, e) = self.parse_tgspan(s);
                    size += e;
                    self.manual("italic", s, e);
                }
                TgSpan::Bold(s) => {
                    let (s, e) = self.parse_tgspan(s);
                    size += e;
                    self.manual("bold", s, e);
                }
                TgSpan::Strikethrough(s) => {
                    let (s, e) = self.parse_tgspan(s);
                    size += e;
                    self.manual("strikethrough", s, e);
                }
                TgSpan::Underline(s) => {
                    let (s, e) = self.parse_tgspan(s);
                    size += e;
                    self.manual("underline", s, e);
                }
                TgSpan::Spoiler(s) => {
                    let (s, e) = self.parse_tgspan(s);
                    size += e;
                    self.manual("spoiler", s, e);
                }
                TgSpan::Link(hint, link) => {
                    let (s, e) = self.parse_tgspan(hint);
                    size += e;
                    let entity = MessageEntityBuilder::new(s, e)
                        .set_type("text_link".to_owned())
                        .set_url(link)
                        .build();
                    self.entities.push(entity);
                }

                TgSpan::Raw(s) => {
                    size += s.encode_utf16().count() as i64;
                    self.text(s);
                }
            };
        }
        let offset = self.offset - size;
        (offset, size)
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

    pub fn from_markdown<T: AsRef<str>>(text: T) -> Self {
        let text = text.as_ref();
        let mut s = Self::new();
        markdown::tokenize(text).into_iter().for_each(|v| {
            s.parse_block(v);
        });
        s
    }

    pub fn from_murkdown<T: AsRef<str>>(text: T) -> Result<Self> {
        let text = text.as_ref();
        let mut s = Self::new();
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(text);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token)?;
        }
        let res = parser.end_of_input()?;
        s.parse_tgspan(res);
        Ok(s)
    }

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

    fn regular<'a, T: AsRef<str>>(
        &'a mut self,
        text: T,
        entity_type: &str,
        advance: Option<i64>,
    ) -> &'a mut Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type(entity_type.to_owned())
            .build();
        self.offset += advance.unwrap_or(n);
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

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

    pub fn strikethrough<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "strikethrough", None)
    }

    pub fn hashtag<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "hashtag", None)
    }

    pub fn cashtag<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "cashtag", None)
    }

    pub fn bot_command<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "bot_command", None)
    }

    pub fn email<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "email", None)
    }

    pub fn phone_number<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "phone_number", None)
    }

    pub fn bold<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "bold", None)
    }

    pub fn italic<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "italic", None)
    }

    pub fn underline<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "underline", None)
    }

    pub fn spoiler<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "spoiler", None)
    }

    pub fn code<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "code", None)
    }

    pub fn mention<'a, T: AsRef<str>>(&'a mut self, text: T) -> &'a mut Self {
        self.regular(text, "mention", None)
    }

    pub fn s<'a>(&'a mut self) -> &'a mut Self {
        let t = " ";
        let count = t.encode_utf16().count() as i64;
        self.offset += count;
        self.text.push_str(t);
        self
    }

    pub fn build<'a>(&'a self) -> (&'a str, &'a Vec<MessageEntity>) {
        (&self.text, &self.entities)
    }

    pub fn build_owned(self) -> (String, Vec<MessageEntity>) {
        (self.text, self.entities)
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

    fn test_parse(markdown: &str) -> Vec<TgSpan> {
        let mut parser = Parser::new();
        let mut tokenizer = Lexer::new(markdown);
        while let Some(token) = tokenizer.next_token() {
            parser.parse(token).unwrap();
        }

        parser.end_of_input().unwrap()
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
        b.parse_tgspan(p);
        assert_eq!(b.entities.len(), 2);
        println!("{}", b.text);
    }
}
