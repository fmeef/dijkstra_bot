//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple
use botapi::gen_types::{MessageEntity, MessageEntityBuilder};
use sea_orm::{entity::prelude::*, FromQueryResult};
use serde::{Deserialize, Serialize};

use crate::tg::admin_helpers::insert_user;

use super::users;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "message_entity")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub tg_type: DbMarkupType,
    #[sea_orm(primary_key)]
    pub offset: i64,
    #[sea_orm(primary_key)]
    pub length: i64,
    pub url: Option<String>,
    pub user: Option<i64>,
    pub language: Option<String>,
    pub emoji_id: Option<String>,
    pub owner_id: i64,
}

#[derive(FromQueryResult, Debug)]
pub struct EntityWithUser {
    // entity fields
    pub tg_type: DbMarkupType,
    pub offset: i64,
    pub length: i64,
    pub url: Option<String>,
    pub user: Option<i64>,
    pub language: Option<String>,
    pub emoji_id: Option<String>,
    pub owner_id: i64,

    // user fields
    pub user_id: Option<i64>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub is_bot: Option<bool>,
}

impl EntityWithUser {
    pub fn get(self) -> (Model, Option<users::Model>) {
        (
            Model {
                tg_type: self.tg_type,
                offset: self.offset,
                length: self.length,
                url: self.url,
                user: self.user,
                language: self.language,
                emoji_id: self.emoji_id,
                owner_id: self.owner_id,
            },
            if let (Some(user_id), Some(first_name), Some(is_bot)) =
                (self.user_id, self.first_name, self.is_bot)
            {
                Some(users::Model {
                    user_id,
                    first_name,
                    last_name: self.last_name,
                    username: self.username,
                    is_bot,
                })
            } else {
                None
            },
        )
    }
}

impl Model {
    pub async fn from_entity(
        messageentity: &MessageEntity,
        owner_id: i64,
    ) -> crate::util::error::Result<Self> {
        if let Some(user) = messageentity.get_user() {
            insert_user(&user).await?;
        }
        let tg_type = DbMarkupType::from_tg_type(messageentity.get_tg_type_ref())?;
        Ok(Self {
            tg_type,
            offset: messageentity.get_offset(),
            length: messageentity.get_length(),
            url: messageentity.get_url().map(|v| v.into_owned()),
            user: messageentity.get_user().map(|v| v.get_id()),
            language: messageentity.get_language().map(|v| v.into_owned()),
            emoji_id: messageentity.get_custom_emoji_id().map(|v| v.into_owned()),
            owner_id,
        })
    }

    pub fn to_entity(self, user: Option<users::Model>) -> MessageEntity {
        let tg_type = self.tg_type.get_tg_type();
        let mut res =
            MessageEntityBuilder::new(self.offset, self.length).set_type(tg_type.to_owned());

        if let Some(user) = user {
            res = res.set_user(user.into());
        }

        if let Some(url) = self.url {
            res = res.set_url(url);
        }

        if let Some(emoji) = self.emoji_id {
            res = res.set_custom_emoji_id(emoji);
        }

        if let Some(language) = self.language {
            res = res.set_language(language);
        }
        res.build()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::super::core::users::Entity",
        from = "Column::User",
        to = "super::super::core::users::Column::UserId"
    )]
    Users,
}

impl Related<super::super::core::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

#[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, PartialEq, Debug, Clone)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum DbMarkupType {
    #[sea_orm(num_value = 1)]
    StrikeThrough,
    #[sea_orm(num_value = 2)]
    HashTag,
    #[sea_orm(num_value = 3)]
    CashTag,
    #[sea_orm(num_value = 4)]
    BotCommand,
    #[sea_orm(num_value = 5)]
    Email,
    #[sea_orm(num_value = 6)]
    PhoneNumber,
    #[sea_orm(num_value = 7)]
    Bold,
    #[sea_orm(num_value = 8)]
    Italic,
    #[sea_orm(num_value = 9)]
    Underline,
    #[sea_orm(num_value = 10)]
    Spoiler,
    #[sea_orm(num_value = 11)]
    Code,
    #[sea_orm(num_value = 12)]
    Mention,
    #[sea_orm(num_value = 13)]
    TextLink,
    #[sea_orm(num_value = 14)]
    TextMention,
    #[sea_orm(num_value = 15)]
    Pre,
    #[sea_orm(num_value = 16)]
    CustomEmoji,
    #[sea_orm(num_value = 17)]
    Url,
}

impl DbMarkupType {
    pub fn from_tg_type(t: &str) -> crate::util::error::Result<Self> {
        match t {
            "text_mention" => Ok(Self::TextMention),
            "text_link" => Ok(Self::TextLink),
            "pre" => Ok(Self::Pre),
            "custom_emoji" => Ok(Self::CustomEmoji),
            "strikethrough" => Ok(Self::StrikeThrough),
            "hashtag" => Ok(Self::HashTag),
            "cashtag" => Ok(Self::CashTag),
            "bot_command" => Ok(Self::BotCommand),
            "email" => Ok(Self::Email),
            "phone_number" => Ok(Self::PhoneNumber),
            "bold" => Ok(Self::Bold),
            "italic" => Ok(Self::Italic),
            "underline" => Ok(Self::Underline),
            "spoiler" => Ok(Self::Spoiler),
            "code" => Ok(Self::Code),
            "mention" => Ok(Self::Mention),
            "url" => Ok(Self::Url),
            v => Err(crate::util::error::BotError::Generic(format!(
                "invalid tg_type {}",
                v
            ))),
        }
    }

    pub fn get_tg_type(&self) -> &str {
        match &self {
            Self::TextMention => "text_mention",
            Self::TextLink => "text_link",
            Self::Pre => "pre",
            Self::CustomEmoji => "custom_emoji",
            Self::StrikeThrough => "strikethrough",
            Self::HashTag => "hashtag",
            Self::CashTag => "cashtag",
            Self::BotCommand => "bot_command",
            Self::Email => "email",
            Self::PhoneNumber => "phone_number",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::Spoiler => "spoiler",
            Self::Code => "code",
            Self::Url => "url",
            Self::Mention => "mention",
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}
