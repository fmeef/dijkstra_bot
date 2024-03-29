//! Context-free grammar for parsing "filters" style commands.
//! Currently this is used by filters and blocklists, but other modules may use it as well
//!
//! Example: (key1, key2, key3) multi fw body {footer}
//! Example: key1 multi fw body
//! Example: "multi fw key" multi fw body

use lazy_static::lazy_static;

use pomelo::pomelo;
use regex::Regex;

/// Data for filter's header
pub enum Header {
    List(Vec<String>),
    Arg(String),
}

/// Complete parsed filter with header, body, and footer
pub struct FilterCommond {
    pub header: Header,
    pub body: Option<String>,
    pub footer: Option<String>,
}

pomelo! {
    %include {
             use super::{FilterCommond, Header};
             use crate::tg::command::TextArg;
        }
    %error crate::tg::markdown::DefaultParseErr;
    %parser pub struct Parser<'e>{};
    %type input FilterCommond;
    %token #[derive(Debug)] pub enum Token<'e>{};
    %type quote String;
    %type fw TextArg<'e>;
    %type Whitespace &'e str;
    %type multi Vec<TextArg<'e>>;
    %type list Vec<TextArg<'e>>;
    %type Str &'e str;
    %type footer String;
    %type fws String;
    %type ign TextArg<'e>;
    %type header Header;

    input    ::= header(A) {
        FilterCommond {
            header: A,
            body: None,
            footer: None
        }
    }
    input    ::= header(A) Whitespace(_) fws(W) {
        FilterCommond {
            header: A,
            body: Some(W),
            footer: None
        }
    }
    input    ::= header(A) Whitespace(_) footer(F) {
        FilterCommond {
            header: A,
            body: None,
            footer: Some(F)
        }
    }

    input    ::= header(A) Whitespace(_) fws(W) Whitespace(_) footer(F) {
        FilterCommond {
            header: A,
            body: Some(W),
            footer: Some(F)
        }
    }
    footer   ::= LBrace fws(A) Rbrace { A }
    header   ::= multi(V)  { Header::List(V.into_iter().map(|v| v.get_text().to_owned()).collect()) }
    header   ::= fw(S) { Header::Arg(S.get_text().to_owned()) }
    header   ::= quote(S) { Header::Arg(S) }
    fw     ::= Str(A) { TextArg::Arg(A) }
    ign      ::= fw(W) { W }
    ign      ::= fw(W) Whitespace(_) { W }
    ign      ::= Whitespace(_) fw(W) { W }
    ign      ::= Whitespace(_) fw(W) Whitespace(_) { W }
    fws    ::= fw(W) { W.get_text().to_owned() }
    fws    ::= fws(mut L) Whitespace(S) fw(W) {
        L.push_str(&S);
        L.push_str(W.get_text());
        L
    }

    quote    ::= Quote fws(A) Quote { A }
    multi    ::= LParen list(A) RParen {A }
    list     ::= ign(A) { vec![A] }
    list     ::= list(mut L) Comma ign(A) { L.push(A); L }

}

pub use parser::{Parser, Token};

lazy_static! {
    static ref TOKENS: Regex = Regex::new(r#"((\s+)|[\{\}\(\),"]|[^\{\}\(\),"\s]+)"#).unwrap();
}

/// Tokenizer for filters
pub struct Lexer<'a>(&'a str);

impl<'a> Lexer<'a> {
    pub fn new(s: &'a str) -> Self {
        Self(s)
    }
    pub fn all_tokens(&'a self) -> impl Iterator<Item = Token<'a>> {
        TOKENS.find_iter(self.0).map(|t| match t.as_str() {
            "(" => Token::LParen,
            ")" => Token::RParen,
            "{" => Token::LBrace,
            "}" => Token::Rbrace,
            "," => Token::Comma,
            "\"" => Token::Quote,
            s if t.as_str().trim().is_empty() => Token::Whitespace(s),
            s => Token::Str(s),
        })
    }
}
