use botapi::gen_types::MessageEntity;
use futures::{stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use sea_orm::{entity::prelude::*, ActiveValue, IntoActiveModel};
use sea_query::OnConflict;
use serde::{Deserialize, Serialize};

use crate::tg::button::InlineKeyboardBuilder;

use super::{button, messageentity};
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "entitylist")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "crate::persist::core::messageentity::Entity")]
    Entities,
    #[sea_orm(has_many = "crate::persist::core::button::Entity")]
    Buttons,
    #[sea_orm(
        belongs_to = "crate::persist::core::button::Entity",
        from = "Column::Id",
        to = "crate::persist::core::button::Column::OwnerId"
    )]
    ButtonsRev,
    #[sea_orm(
        belongs_to = "crate::persist::core::messageentity::Entity",
        from = "Column::Id",
        to = "crate::persist::core::messageentity::Column::OwnerId"
    )]
    EntitiesRev,
}

impl Related<crate::persist::core::messageentity::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Entities.def()
    }
}

impl Related<Entity> for crate::persist::core::messageentity::Entity {
    fn to() -> RelationDef {
        Relation::EntitiesRev.def().rev()
    }
}

impl Related<crate::persist::core::button::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Buttons.def()
    }
}

impl Related<Entity> for crate::persist::core::button::Entity {
    fn to() -> RelationDef {
        Relation::ButtonsRev.def().rev()
    }
}

pub async fn insert<T>(
    conn: &T,
    entities: &Vec<MessageEntity>,
    buttons: InlineKeyboardBuilder,
) -> crate::util::error::Result<Option<i64>>
where
    T: ConnectionTrait,
{
    log::info!("inserting {} entities", entities.len());

    if entities.len() > 0 || buttons.get().iter().map(|v| v.len()).sum::<usize>() > 0 {
        let entity_id = Entity::insert(ActiveModel {
            id: ActiveValue::NotSet,
        })
        .exec_with_returning(conn)
        .await?
        .id;

       let entities: Vec<messageentity::Model> = stream::iter(entities)
            .then(|v| async move { messageentity::Model::from_entity(v, entity_id).await })
            .try_collect()
            .await?;

        if entities.len() > 0 {
            messageentity::Entity::insert_many(
                entities
                    .into_iter()
                    .map(|v| v.into_active_model())
                    .collect::<Vec<messageentity::ActiveModel>>(),
            )
            .on_conflict(
                OnConflict::columns([
                    messageentity::Column::TgType,
                    messageentity::Column::Offset,
                    messageentity::Column::Length,
                    messageentity::Column::OwnerId,
                ])
                .update_columns([
                    messageentity::Column::Url,
                    messageentity::Column::User,
                    messageentity::Column::Language,
                    messageentity::Column::EmojiId,
                ])
                .to_owned(),
            )
            .exec_with_returning(conn)
            .await?;
        }

        let buttons = buttons
            .into_inner()
            .into_iter()
            .flat_map(|list| list.into_iter())
            .map(|mut v| {
                v.owner_id = Some(entity_id);
                v
            })
            .map(|v| v.into_active_model())
            .collect_vec();
        if buttons.len() > 0 {
            button::Entity::insert_many(buttons)
                .on_conflict(
                    OnConflict::columns([
                        button::Column::OwnerId,
                        button::Column::PosX,
                        button::Column::PosY,
                    ])
                    .update_columns([
                        button::Column::ButtonText,
                        button::Column::CallbackData,
                        button::Column::ButtonUrl,
                        button::Column::RawText,
                    ])
                    .to_owned(),
                )
                .exec(conn)
                .await?;
        }
        Ok(Some(entity_id))
    } else {
        Ok(None)
    }
}

impl ActiveModelBehavior for ActiveModel {}
