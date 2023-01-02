use botapi::gen_types::{MessageEntity, MessageEntityBuilder, User};

pub(crate) struct MarkupBuilder {
    entities: Vec<MessageEntity>,
    offset: i64,
    text: String,
}

#[allow(dead_code)]
impl MarkupBuilder {
    pub(crate) fn new() -> Self {
        Self {
            entities: Vec::new(),
            offset: 0,
            text: String::new(),
        }
    }

    pub(crate) fn text<T: AsRef<str>>(mut self, text: T) -> Self {
        self.text.push_str(text.as_ref());
        self.offset += text.as_ref().encode_utf16().count() as i64;
        self
    }

    fn regular<T: AsRef<str>>(mut self, text: T, entity_type: &str) -> Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type(entity_type.to_owned())
            .build();
        self.offset += n;
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    pub(crate) fn text_link<T: AsRef<str>>(mut self, text: T, link: String) -> Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("text_link".to_owned())
            .set_url(link)
            .build();
        self.offset += n;
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    pub(crate) fn text_mention<T: AsRef<str>>(mut self, text: T, mention: User) -> Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("text_mention".to_owned())
            .set_user(mention)
            .build();
        self.offset += n;
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    pub(crate) fn pre<T: AsRef<str>>(mut self, text: T, language: String) -> Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("pre".to_owned())
            .set_language(language)
            .build();
        self.offset += n;
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    pub(crate) fn custom_emoji<T: AsRef<str>>(mut self, text: T, emoji_id: String) -> Self {
        let text = text.as_ref();
        let n = text.encode_utf16().count() as i64;
        let entity = MessageEntityBuilder::new(self.offset, n)
            .set_type("custom_emoji".to_owned())
            .set_custom_emoji_id(emoji_id)
            .build();
        self.offset += n;
        self.entities.push(entity);
        self.text.push_str(text);
        self
    }

    pub(crate) fn strikethrough<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "strikethrough")
    }

    pub(crate) fn hashtag<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "hashtag")
    }

    pub(crate) fn cashtag<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "cashtag")
    }

    pub(crate) fn bot_command<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "bot_command")
    }

    pub(crate) fn email<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "email")
    }

    pub(crate) fn phone_number<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "phone_number")
    }

    pub(crate) fn bold<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "bold")
    }

    pub(crate) fn italic<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "italic")
    }

    pub(crate) fn underline<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "underline")
    }

    pub(crate) fn spoiler<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "spoiler")
    }

    pub(crate) fn code<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "code")
    }

    pub(crate) fn mention<T: AsRef<str>>(self, text: T) -> Self {
        self.regular(text, "mention")
    }

    pub(crate) fn s(mut self) -> Self {
        let t = " ";
        let count = t.encode_utf16().count() as i64;
        self.offset += count;
        self.text.push_str(t);
        self
    }

    pub(crate) fn build(self) -> (String, Vec<MessageEntity>) {
        (self.text, self.entities)
    }
}
