use std::fmt::Display;

use lazy_static::lazy_static;
use pomelo::pomelo;
use regex::Regex;
use std::collections::VecDeque;
use thiserror::Error;

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

pomelo! {
    %error super::DefaultParseErr;
    %type input std::collections::VecDeque<crate::tg::command::Arg>;
    %type words std::collections::VecDeque<crate::tg::command::Arg>;
    %type quoteinner Vec<String>;
    %type Word String;
    %type quote Vec<String>;
    %type commond String;

    input   ::= commond(C) { std::collections::VecDeque::from([crate::tg::command::Arg::Command(C)]) }
    input   ::= commond(C) words(mut L) { L.push_back(crate::tg::command::Arg::Command(C)); L }
    commond ::= Exclaimation Word(W) { W }
    commond ::= Slash Word(W) { W }
    words   ::= Word(W) { std::collections::VecDeque::from([crate::tg::command::Arg::Arg(W)]) }
    words   ::= words(mut L)  Word(W) { L.push_back(crate::tg::command::Arg::Arg(W)); L }
    words   ::= words(mut L)  quote(Q) { L.push_back(crate::tg::command::Arg::Quote(Q)); L }
    words   ::= quote(Q) { std::collections::VecDeque::from([crate::tg::command::Arg::Quote(Q)]) }
    quoteinner ::= Word(W) { vec![W] }
    quoteinner ::= Word(W) quoteinner(mut L) { L.push(W); L }
    quote ::= QuoteMark quoteinner(Q) QuoteMark { Q }

}

lazy_static! {
    static ref TOKENS: Regex = Regex::new(r#"([^\s"!/]+|"|^!|^/)"#).unwrap();
}

pub enum Arg {
    Arg(String),
    Quote(Vec<String>),
    Command(String),
}

use parser::{Parser, Token};

use crate::persist::Result;

pub struct DefaultTokenizer(String);

impl DefaultTokenizer {
    pub fn new(val: String) -> Self {
        Self(val)
    }

    pub fn next_tokens<'a>(&'a self) -> impl Iterator<Item = Token> + 'a {
        TOKENS.find_iter(&self.0).map(|m| match m.as_str() {
            r#"""# => Token::QuoteMark,
            "!" => Token::Exclaimation,
            "/" => Token::Slash,
            _ => Token::Word(m.as_str().to_owned()),
        })
    }
}

fn parse_cmd_r<R: ToString>(cmd: R) -> Result<(Arg, VecDeque<Arg>)> {
    let tokenizer = DefaultTokenizer::new(cmd.to_string());

    let mut parser = Parser::new();
    tokenizer.next_tokens().try_for_each(|t| parser.parse(t))?;

    let mut res = parser.end_of_input()?;
    Ok((
        res.pop_front().ok_or_else(|| anyhow::anyhow!("empty"))?,
        res,
    ))
}

pub fn parse_cmd<R: ToString>(cmd: R) -> Option<(Arg, VecDeque<Arg>)> {
    parse_cmd_r(cmd).ok()
}

pub fn parse_cmd_iter<R: ToString>(cmd: R) -> Option<impl Iterator<Item = Arg>> {
    parse_cmd_iter_r(cmd).ok()
}

fn parse_cmd_iter_r<R: ToString>(cmd: R) -> Result<impl Iterator<Item = Arg>> {
    let iter = parse_cmd_r(cmd)?.1.into_iter();
    Ok(iter)
}
