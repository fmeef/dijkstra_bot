use std::collections::VecDeque;

use lazy_static::lazy_static;

use regex::Regex;

lazy_static! {
    static ref COMMOND: Regex = Regex::new(r#"^(!|/)\w+(@\w)?\s+.*"#).unwrap();
    static ref COMMOND_HEAD: Regex = Regex::new(r#"^(!|/)\w+(@\w+)?"#).unwrap();
    static ref TOKENS: Regex = Regex::new(r#"([^\s"!/]+|"|^!|^/)"#).unwrap();
    static ref ARGS: Regex = Regex::new(r#"(".*"|[^"\s]+)"#).unwrap();
    static ref QUOTE: Regex = Regex::new(r#"".*""#).unwrap();
}

pub enum Arg<'a> {
    Arg(&'a str),
    Command(&'a str),
    Quote(&'a str),
}

pub fn parse_cmd<'a>(cmd: &'a str) -> Option<(&'a str, VecDeque<Arg<'a>>)> {
    if let Some(head) = COMMOND_HEAD.find(cmd) {
        Some((
            &head.as_str()[1..head.end()],
            ARGS.find_iter(&cmd[head.end()..])
                .map(|v| {
                    if QUOTE.is_match(v.as_str()) {
                        Arg::Quote(v.as_str())
                    } else {
                        Arg::Arg(v.as_str())
                    }
                })
                .collect(),
        ))
    } else {
        None
    }
}
