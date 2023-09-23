use std::{collections::HashMap, ops::Deref};

use crate::{persist::core::media::*, statics::DB};
use sea_orm::{entity::prelude::*, FromQueryResult, QueryOrder, QuerySelect};
use sea_query::{IntoCondition, JoinType};
use serde::{Deserialize, Serialize};

use super::{
    button, entity,
    messageentity::{self, DbMarkupType, EntityWithUser},
    users,
};
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, Hash, Eq)]
#[sea_orm(table_name = "welcome")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub chat: i64,
    #[sea_orm(column_type = "Text")]
    pub text: Option<String>,
    pub media_id: Option<String>,
    pub media_type: Option<MediaType>,
    #[sea_orm(column_type = "Text")]
    pub goodbye_text: Option<String>,
    pub goodbye_media_id: Option<String>,
    pub goodbye_media_type: Option<MediaType>,
    #[sea_orm(default = false)]
    pub enabled: bool,
    pub welcome_entity_id: Option<i64>,
    pub goodbye_entity_id: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::persist::core::entity::Entity",
        from = "Column::WelcomeEntityId",
        to = "crate::persist::core::entity::Column::Id"
    )]
    WelcomeEntities,

    #[sea_orm(
        belongs_to = "crate::persist::core::entity::Entity",
        from = "Column::GoodbyeEntityId",
        to = "crate::persist::core::entity::Column::Id"
    )]
    GoodbyeEntities,
}

impl Related<crate::persist::core::entity::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WelcomeEntities.def()
    }
}

impl Related<Entity> for crate::persist::core::entity::Entity {
    fn to() -> RelationDef {
        Relation::WelcomeEntities.def().rev()
    }
}

impl ActiveModelBehavior for ActiveModel {}
#[derive(FromQueryResult)]
pub struct WelcomesWithEntities {
    /// Welcome fields    
    pub chat: Option<i64>,
    pub text: Option<String>,
    pub media_id: Option<String>,
    pub media_type: Option<MediaType>,
    pub goodbye_text: Option<String>,
    pub goodbye_media_id: Option<String>,
    pub goodbye_media_type: Option<MediaType>,
    pub enabled: Option<bool>,
    pub welcome_entity_id: Option<i64>,
    pub goodbye_entity_id: Option<i64>,

    //button fields
    pub button_text: Option<String>,
    pub callback_data: Option<String>,
    pub button_url: Option<String>,
    pub owner_id: Option<i64>,
    pub pos_x: Option<i32>,
    pub pos_y: Option<i32>,
    pub raw_text: Option<String>,

    //goodbye button fields
    pub goodbye_button_text: Option<String>,
    pub goodbye_callback_data: Option<String>,
    pub goodbye_button_url: Option<String>,
    pub goodbye_pos_x: Option<i32>,
    pub goodbye_pos_y: Option<i32>,
    pub goodbye_raw_text: Option<String>,

    // entity fields
    pub tg_type: Option<DbMarkupType>,
    pub offset: Option<i64>,
    pub length: Option<i64>,
    pub url: Option<String>,
    pub user: Option<i64>,
    pub language: Option<String>,
    pub emoji_id: Option<String>,

    // goodbye entity fields
    pub goodbye_tg_type: Option<DbMarkupType>,
    pub goodbye_offset: Option<i64>,
    pub goodbye_length: Option<i64>,
    pub goodbye_url: Option<String>,
    pub goodbye_user: Option<i64>,
    pub goodbye_language: Option<String>,
    pub goodbye_emoji_id: Option<String>,
    pub goodbye_owner_id: Option<i64>,

    // goodbye user fields
    pub goodbye_user_id: Option<i64>,
    pub goodbye_first_name: Option<String>,
    pub goodbye_last_name: Option<String>,
    pub goodbye_username: Option<String>,
    pub goodbye_is_bot: Option<bool>,

    // user fields
    pub user_id: Option<i64>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub is_bot: Option<bool>,
}

impl WelcomesWithEntities {
    pub fn get(
        self,
    ) -> (
        Option<Model>,
        Option<button::Model>,
        Option<button::Model>,
        Option<EntityWithUser>,
        Option<EntityWithUser>,
    ) {
        let button = if let (Some(button_text), Some(owner_id), Some(pos_x), Some(pos_y)) =
            (self.button_text, self.owner_id, self.pos_x, self.pos_y)
        {
            Some(button::Model {
                button_text,
                owner_id: Some(owner_id),
                callback_data: self.callback_data,
                button_url: self.button_url,
                pos_x,
                pos_y,
                raw_text: self.raw_text,
            })
        } else {
            None
        };

        let goodbye_button = if let (Some(button_text), Some(owner_id), Some(pos_x), Some(pos_y)) = (
            self.goodbye_button_text,
            self.goodbye_owner_id,
            self.goodbye_pos_x,
            self.goodbye_pos_y,
        ) {
            Some(button::Model {
                button_text,
                owner_id: Some(owner_id),
                callback_data: self.goodbye_callback_data,
                button_url: self.goodbye_button_url,
                pos_x,
                pos_y,
                raw_text: self.goodbye_raw_text,
            })
        } else {
            None
        };

        let filter = if let (Some(chat), Some(enabled)) = (self.chat, self.enabled) {
            Some(Model {
                chat,
                text: self.text,
                media_id: self.media_id,
                media_type: self.media_type,
                goodbye_text: self.goodbye_text,
                goodbye_media_id: self.goodbye_media_id,
                goodbye_media_type: self.goodbye_media_type,
                enabled,
                welcome_entity_id: self.welcome_entity_id,
                goodbye_entity_id: self.goodbye_entity_id,
            })
        } else {
            None
        };

        let welcome_entity = if let (Some(tg_type), Some(offset), Some(length), Some(owner_id)) =
            (self.tg_type, self.offset, self.length, self.owner_id)
        {
            Some(EntityWithUser {
                tg_type,
                offset,
                length,
                url: self.url,
                language: self.language,
                emoji_id: self.emoji_id,
                user: self.user,
                owner_id,
                user_id: self.user_id,
                first_name: self.first_name,
                last_name: self.last_name,
                username: self.username,
                is_bot: self.is_bot,
            })
        } else {
            None
        };

        let goodbye_entity = if let (Some(tg_type), Some(offset), Some(length), Some(owner_id)) = (
            self.goodbye_tg_type,
            self.goodbye_offset,
            self.goodbye_length,
            self.goodbye_owner_id,
        ) {
            Some(EntityWithUser {
                tg_type,
                offset,
                length,
                url: self.goodbye_url,
                language: self.goodbye_language,
                emoji_id: self.goodbye_emoji_id,
                user: self.goodbye_user,
                owner_id,
                user_id: self.goodbye_user_id,
                first_name: self.goodbye_first_name,
                last_name: self.goodbye_last_name,
                username: self.goodbye_username,
                is_bot: self.goodbye_is_bot,
            })
        } else {
            None
        };

        (
            filter,
            button,
            goodbye_button,
            welcome_entity,
            goodbye_entity,
        )
    }
}
#[derive(Iden)]
struct EntityAlias;

#[derive(Iden)]
struct ListAlias;

#[derive(Iden)]
struct UserAlias;

#[derive(Iden)]
struct ButtonAlias;

pub type FiltersMap = HashMap<
    Model,
    (
        Vec<EntityWithUser>,
        Vec<EntityWithUser>,
        Vec<button::Model>,
        Vec<button::Model>,
    ),
>;

pub async fn get_filters_join<F>(filter: F) -> crate::util::error::Result<FiltersMap>
where
    F: IntoCondition,
{
    let res = Entity::find()
        .select_only()
        .columns([
            Column::Chat,
            Column::Text,
            Column::MediaId,
            Column::MediaType,
            Column::GoodbyeText,
            Column::GoodbyeMediaId,
            Column::GoodbyeMediaType,
            Column::Enabled,
            Column::WelcomeEntityId,
            Column::GoodbyeEntityId,
        ])
        .columns([
            messageentity::Column::TgType,
            messageentity::Column::Offset,
            messageentity::Column::Length,
            messageentity::Column::Url,
            messageentity::Column::User,
            messageentity::Column::Language,
            messageentity::Column::EmojiId,
            messageentity::Column::OwnerId,
        ])
        .columns([
            button::Column::ButtonText,
            button::Column::CallbackData,
            button::Column::ButtonUrl,
            button::Column::PosX,
            button::Column::PosY,
            button::Column::RawText,
        ])
        .column_as(
            Expr::col((ButtonAlias, button::Column::ButtonText)),
            "goodbye_button_text",
        )
        .column_as(
            Expr::col((ButtonAlias, button::Column::CallbackData)),
            "goodbye_callback_data",
        )
        .column_as(
            Expr::col((ButtonAlias, button::Column::ButtonUrl)),
            "goodbye_button_url",
        )
        .column_as(
            Expr::col((ButtonAlias, button::Column::PosX)),
            "goodbye_pos_x",
        )
        .column_as(
            Expr::col((ButtonAlias, button::Column::PosY)),
            "goodbye_pos_y",
        )
        .column_as(
            Expr::col((ButtonAlias, button::Column::RawText)),
            "goodbye_raw_text",
        )
        .columns([
            users::Column::UserId,
            users::Column::FirstName,
            users::Column::LastName,
            users::Column::Username,
            users::Column::IsBot,
        ])
        .column_as(
            Expr::col((UserAlias, users::Column::UserId)),
            "goodbye_user_id",
        )
        .column_as(
            Expr::col((UserAlias, users::Column::FirstName)),
            "goodbye_first_name",
        )
        .column_as(
            Expr::col((UserAlias, users::Column::LastName)),
            "goodbye_last_name",
        )
        .column_as(
            Expr::col((UserAlias, users::Column::Username)),
            "goodbye_username",
        )
        .column_as(
            Expr::col((UserAlias, users::Column::IsBot)),
            "goodbye_is_bot",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::TgType)),
            "goodbye_tg_type",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::Offset)),
            "goodbye_offset",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::Length)),
            "goodbye_length",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::Url)),
            "goodbye_url",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::User)),
            "goodbye_user",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::Language)),
            "goodbye_language",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::EmojiId)),
            "goodbye_emoji_id",
        )
        .column_as(
            Expr::col((EntityAlias, messageentity::Column::OwnerId)),
            "goodbye_owner_id",
        )
        .join(JoinType::LeftJoin, Relation::WelcomeEntities.def())
        .join_as(
            JoinType::LeftJoin,
            Relation::GoodbyeEntities.def(),
            ListAlias,
        )
        .join_as(
            JoinType::LeftJoin,
            entity::Relation::ButtonsRev
                .def()
                .on_condition(|_left, _right| {
                    Expr::col((ButtonAlias, button::Column::OwnerId))
                        .eq(Expr::col((ListAlias, entity::Column::Id)))
                        .into_condition()
                }),
            ButtonAlias,
        )
        .join_as(
            JoinType::LeftJoin,
            entity::Relation::EntitiesRev
                .def()
                .on_condition(|_left, _right| {
                    Expr::col((EntityAlias, messageentity::Column::OwnerId))
                        .eq(Expr::col((ListAlias, entity::Column::Id)))
                        .into_condition()
                }),
            EntityAlias,
        )
        .join(JoinType::LeftJoin, entity::Relation::EntitiesRev.def())
        .join_as(
            JoinType::LeftJoin,
            messageentity::Relation::Users.def(),
            UserAlias,
        )
        .join(JoinType::LeftJoin, entity::Relation::ButtonsRev.def())
        .join(JoinType::LeftJoin, messageentity::Relation::Users.def())
        .filter(filter)
        .order_by_asc(button::Column::PosX)
        .order_by_asc(button::Column::PosY)
        .into_model::<WelcomesWithEntities>()
        .all(DB.deref())
        .await?;

    let res = res.into_iter().map(|v| v.get()).fold(
        FiltersMap::new(),
        |mut acc, (filter, button, gb_button, entity, goodbye)| {
            //        log::info!("got entity {:?} goodbye {:?}", entity, goodbye);
            if let Some(filter) = filter {
                let (entitylist, goodbyelist, buttonlist, gb_buttonlist) = acc
                    .entry(filter)
                    .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new(), Vec::new()));

                if let Some(button) = button {
                    buttonlist.push(button);
                }

                if let Some(entity) = entity {
                    entitylist.push(entity);
                }

                if let Some(goodbye) = goodbye {
                    goodbyelist.push(goodbye);
                }

                if let Some(gb) = gb_button {
                    gb_buttonlist.push(gb);
                }
            }
            acc
        },
    );

    Ok(res)
}
