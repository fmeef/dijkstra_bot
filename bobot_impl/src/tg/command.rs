use std::fmt::Display;

use lazy_static::lazy_static;
use pomelo::pomelo;
use regex::Regex;
use std::collections::VecDeque;
use thiserror::Error;

#[cfg(test)]
mod test {
    use super::*;
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

pomelo! {
    %error super::DefaultParseErr;
    %type input std::collections::VecDeque<crate::tg::command::Arg>;
    %type words std::collections::VecDeque<crate::tg::command::Arg>;
    %type quoteinner Vec<String>;
    %type Word String;
    %type quote Vec<String>;

    input ::= words?(A) { A.unwrap_or_else(std::collections::VecDeque::new) }
    words ::= Word(W) { std::collections::VecDeque::from([crate::tg::command::Arg::Arg(W)]) }
    words ::= words(mut L)  Word(W) { L.push_back(crate::tg::command::Arg::Arg(W)); L }
    words ::= words(mut L)  quote(Q) { L.push_back(crate::tg::command::Arg::Quote(Q)); L }
    words ::= quote(Q) { std::collections::VecDeque::from([crate::tg::command::Arg::Quote(Q)]) }
    quoteinner ::= Word(W) { vec![W] }
    quoteinner ::= Word(W) quoteinner(mut L) { L.push(W); L }
    quote ::= QuoteMark quoteinner(Q) QuoteMark { Q }

}

lazy_static! {
    static ref TOKENS: Regex = Regex::new(r#"([^\s"]+|")"#).unwrap();
}

pub enum Arg {
    Arg(String),
    Quote(Vec<String>),
}

use parser::{Parser, Token};

use crate::persist::Result;

pub(crate) struct DefaultTokenizer(String);

impl DefaultTokenizer {
    pub fn new(val: String) -> Self {
        Self(val)
    }

    pub fn next_tokens<'a>(&'a self) -> impl Iterator<Item = Token> + 'a {
        TOKENS.find_iter(&self.0).map(|m| {
            if m.as_str() == r#"""# {
                Token::QuoteMark
            } else {
                Token::Word(m.as_str().to_owned())
            }
        })
    }
}

#[allow(dead_code)]
pub(crate) fn parse_cmd<R: ToString>(cmd: R) -> Result<(Arg, VecDeque<Arg>)> {
    let tokenizer = DefaultTokenizer::new(cmd.to_string());

    let mut parser = Parser::new();
    tokenizer.next_tokens().try_for_each(|t| parser.parse(t))?;

    let mut res = parser.end_of_input()?;
    Ok((
        res.pop_front().ok_or_else(|| anyhow::anyhow!("empty"))?,
        res,
    ))
}

#[allow(dead_code)]
pub(crate) fn parse_cmd_iter<R: ToString>(cmd: R) -> Result<impl Iterator<Item = Arg>> {
    let iter = parse_cmd(cmd)?.1.into_iter();
    Ok(iter)
}
