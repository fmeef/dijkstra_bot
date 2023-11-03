use std::collections::{BTreeMap, HashMap};

use botapi::gen_types::{
    InlineKeyboardButton, InlineKeyboardButtonBuilder, MessageEntity, MessageEntityBuilder,
};

use super::button::InlineKeyboardBuilder;

pub trait IntoUtf16Chars {
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
    let mut link_end;
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
    }

    if !found_text_end {
        return None;
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

pub struct RoseMdParser {
    prefixes: HashMap<String, String>,
    same_line_suffix: String,
    chars: Vec<char>,
    enable_buttons: bool,
}

pub struct RoseMdDecompiler<'a> {
    out: &'a str,
    entities: BTreeMap<i64, Vec<&'a MessageEntity>>,
    buttons: &'a Vec<Vec<InlineKeyboardButton>>,
    current: BTreeMap<i64, Vec<&'a MessageEntity>>,
}

impl<'a> RoseMdDecompiler<'a> {
    pub fn new(
        out: &'a str,
        entities: &'a Vec<MessageEntity>,
        buttons: &'a Vec<Vec<InlineKeyboardButton>>,
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

    pub fn decompile(mut self) -> String {
        let mut out = String::new();
        for (offset, ch) in self.out.into_utf16_chars().into_iter().enumerate() {
            if let Some(entity) = self.entities.remove(&(offset as i64)) {
                for entity in entity.into_iter().rev() {
                    // println!(
                    //     "match entity: {} {}",
                    //     entity.get_tg_type_ref(),
                    //     entity.get_offset()
                    // );
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

        for buttons in self.buttons {
            for (x, button) in buttons.iter().enumerate() {
                let same = if x < self.buttons.len() - 1 {
                    ":same"
                } else {
                    ""
                };
                let url = button.get_url_ref().unwrap_or(button.get_text_ref());
                out.push_str(&format!(
                    "[{}](buttonurl://{}{})",
                    button.get_text(),
                    url,
                    same
                ));
            }
        }
        out
    }
}

impl RoseMdParser {
    pub fn new(chars: &str, enable_buttons: bool) -> Self {
        let mut prefixes = HashMap::with_capacity(1);
        prefixes.insert("url".to_owned(), "buttonurl:".to_owned());
        Self {
            prefixes,
            same_line_suffix: "same".to_owned(),
            chars: chars.into_utf16_chars(),
            enable_buttons,
        }
    }

    pub fn parse(&self) -> (String, Vec<MessageEntity>, InlineKeyboardBuilder) {
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
                            // println!(
                            //     "parsed nested {} {} {}",
                            //     entity.get_tg_type_ref(),
                            //     x,
                            //     offset
                            // );

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

                        if self.enable_buttons {
                            for (_, prefix) in self.prefixes.iter() {
                                if !content.starts_with(prefix) {
                                    continue;
                                }

                                let content = content.trim_start_matches(prefix);
                                let content = content.trim_start_matches("/");

                                let sameline = content.ends_with(&self.same_line_suffix);
                                if sameline
                                    || builder.get().len() == 1
                                        && builder.get().first().unwrap().len() == 0
                                {
                                    let content = content.trim_end_matches(&self.same_line_suffix);
                                    let bt = if content.starts_with("#") {
                                        InlineKeyboardButtonBuilder::new(nested_text.to_owned())
                                            .build()
                                    } else {
                                        InlineKeyboardButtonBuilder::new(nested_text.to_owned())
                                            .set_url(content.to_owned())
                                            .build()
                                    };

                                    builder.button(bt);
                                } else {
                                    let bt = if content.starts_with("#") {
                                        InlineKeyboardButtonBuilder::new(nested_text.to_owned())
                                            .build()
                                    } else {
                                        InlineKeyboardButtonBuilder::new(nested_text.to_owned())
                                            .set_url(content.to_owned())
                                            .build()
                                    };

                                    builder.newline().button(bt);
                                }
                                builder.merge(nested_buttons);
                                builder.merge(follow_buttons);
                                return (text, res, builder);
                            }
                        }

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

#[allow(unused_imports)]
mod test {
    use super::*;
    #[test]
    fn parse_bold() {
        let t = "*bold*";
        let md = RoseMdParser::new(t, true);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "bold");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");
    }

    #[test]
    fn parse_link() {
        let t = "[thing](https://example.com)";
        let md = RoseMdParser::new(t, true);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "thing");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "text_link");
    }

    #[test]
    fn parse_button() {
        let t = "thing [thing](buttonurl://https://example.com)";
        let md = RoseMdParser::new(t, true);
        let (text, _, buttons) = md.parse();
        println!("{:?}", buttons);
        assert_eq!(text.trim(), "thing");
        assert_eq!(buttons.get().len(), 1);
    }

    #[test]
    fn decompile_button() {
        let t = "thing [thing](buttonurl://https://example.com)";
        let md = RoseMdParser::new(t, true);
        let (text, entities, buttons) = md.parse();
        println!("{:?}", buttons);
        assert_eq!(text.trim(), "thing");
        assert_eq!(buttons.get().len(), 1);
        let b = buttons.build();
        let b = b.get_inline_keyboard_ref();
        let decompiler = RoseMdDecompiler::new(&text, &entities, &b);
        let out = decompiler.decompile();

        println!("{}", out);
        assert_eq!(out, t);
    }

    #[test]
    fn decompile_link() {
        let t = "[thing](https://example.com)";
        let md = RoseMdParser::new(t, true);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "thing");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "text_link");

        let v = Vec::new();
        let decompiler = RoseMdDecompiler::new(&text, &entities, &v);
        let out = decompiler.decompile();

        assert_eq!(out, t);
    }

    #[test]
    fn parse_many() {
        let t = "*bold* ~strike~ *||boldspoiler||*";
        let md = RoseMdParser::new(t, true);
        let (text, entities, _) = md.parse();

        assert_eq!(text, "bold strike boldspoiler");
        assert_eq!(entities.len(), 4);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");
    }

    #[test]
    fn decompile_bold() {
        let t = "*bold*";
        let md = RoseMdParser::new(t, true);
        let (text, entities, buttons) = md.parse();

        assert_eq!(text, "bold");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");

        let b = buttons.build();
        let b = b.get_inline_keyboard_ref();
        let decompiler = RoseMdDecompiler::new(&text, &entities, &b);
        let out = decompiler.decompile();

        assert_eq!(out, t);
    }

    #[test]
    fn into_utf16_chars() {
        let v = "im so many sads";
        for (v, n) in v.chars().zip(v.into_utf16_chars()) {
            assert_eq!(v, n);
        }
    }

    #[test]
    fn decompile_many() {
        let t = "*bold* ~strike~ *||boldspoiler||*";
        let t_rev = "*bold* ~strike~ ||*boldspoiler*||";
        let md = RoseMdParser::new(t, true);
        let (text, entities, buttons) = md.parse();
        println!("got entities {:?}", entities);
        assert_eq!(text, "bold strike boldspoiler");
        assert_eq!(entities.len(), 4);
        assert_eq!(entities[0].get_tg_type_ref(), "bold");

        let b = buttons.build();
        let b = b.get_inline_keyboard_ref();
        let decompiler = RoseMdDecompiler::new(&text, &entities, &b);
        let out = decompiler.decompile();

        assert!(out == t || out == t_rev);
    }
}
