use std::fmt::Display;

use lazy_static::lazy_static;
use pomelo::pomelo;
use regex::Regex;
use thiserror::Error;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_test() {
        let s = r#"word word2 "quoted words many" word3"#;
        let parsed = parse_cmd(s).unwrap();
        let mut quotes = 0;
        let mut words = 0;
        for p in parsed {
            match p {
                Arg::Arg(ref r) => words = words + 1,
                Arg::Quote(ref r) => quotes = quotes + 1,
            }
        }
        println!("quotes {}", quotes);
        println!("words {}", words);
        assert!(quotes == 1);
        assert!(words == 3);
    }
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
    %type input Vec<crate::tg::command::Arg>;
    %type words Vec<crate::tg::command::Arg>;
    %type quoteinner Vec<String>;
    %type Word String;
    %type Quote Vec<String>;
    %type quote Vec<String>;
    %type quotemark ();

    input ::= words?(A) { A.unwrap_or_else(Vec::new) }
    words ::= Word(W) { vec![crate::tg::command::Arg::Arg(W)] }
    words ::= words(mut L)  Word(W) { L.push(crate::tg::command::Arg::Arg(W)); L }
    words ::= words(mut L)  quote(Q) { L.push(crate::tg::command::Arg::Quote(Q)); L }
    words ::= quote(Q) { vec![crate::tg::command::Arg::Quote(Q)] }
    quoteinner ::= Word(W) { vec![W] }
    quoteinner ::= Word(W) quoteinner(mut L) { L.push(W); L }
    quote ::= QuoteMark quoteinner(Q) QuoteMark { Q }

}

lazy_static! {
    static ref TOKENS: Regex = Regex::new(r#"([^\W"]+|")"#).unwrap();
}

pub enum Arg {
    Arg(String),
    Quote(Vec<String>),
}

use parser::{Parser, Token};

use crate::persist::Result;

pub(crate) struct DefaultTokenizer {
    val: String,
    pos: usize,
}

impl DefaultTokenizer {
    pub fn new(val: String) -> Self {
        Self { val, pos: 0 }
    }

    pub fn next_tokens<'a>(&'a self) -> impl Iterator<Item = Token> + 'a {
        TOKENS.find_iter(&self.val).map(|m| {
            if m.as_str() == r#"""# {
                Token::QuoteMark
            } else {
                Token::Word(m.as_str().to_owned())
            }
        })
    }
}

pub(crate) fn parse_cmd<R: ToString>(cmd: R) -> Result<Vec<Arg>> {
    let tokenizer = DefaultTokenizer::new(cmd.to_string());

    let mut parser = Parser::new();
    tokenizer.next_tokens().try_for_each(|t| parser.parse(t))?;
    let res = parser.end_of_input()?;
    Ok(res)
}

pub(crate) fn parse_cmd_iter<R: ToString>(cmd: R) -> Result<impl Iterator<Item = Arg>> {
    let iter = parse_cmd(cmd)?.into_iter();
    Ok(iter)
}
