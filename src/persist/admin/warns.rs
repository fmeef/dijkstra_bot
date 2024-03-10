use chrono::Utc;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel, Eq, Hash)]
#[sea_orm(table_name = "warns")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
    pub user_id: i64,
    pub chat_id: i64,
    pub expires: Option<chrono::DateTime<Utc>>,
    pub reason: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    // #[sea_orm(
    //     belongs_to = "crate::persist::core::entity::Entity",
    //     from = "Column::EntityId",
    //     to = "crate::persist::core::entity::Column::Id"
    // )]
    // Entities,
}

// impl Related<crate::persist::core::entity::Entity> for Entity {
//     fn to() -> RelationDef {
//         Relation::Entities.def()
//     }
// }

// impl Related<Entity> for crate::persist::core::entity::Entity {
//     fn to() -> RelationDef {
//         Relation::Entities.def().rev()
//     }
// }

// #[derive(FromQueryResult)]
// struct FiltersWithEntities {
//     // warns fields
//     pub id: Option<i64>,
//     pub warn_user_id: Option<i64>,
//     pub chat_id: Option<i64>,
//     pub expires: Option<chrono::DateTime<Utc>>,
//     pub reason: Option<String>,
//     pub entity_id: Option<i64>,

//     // button fields
//     pub button_text: Option<String>,
//     pub callback_data: Option<String>,
//     pub button_url: Option<String>,
//     pub owner_id: Option<i64>,
//     pub pos_x: Option<i32>,
//     pub pos_y: Option<i32>,
//     pub b_owner_id: Option<i64>,
//     pub raw_text: Option<String>,

//     // entity fields
//     pub tg_type: Option<DbMarkupType>,
//     pub offset: Option<i64>,
//     pub length: Option<i64>,
//     pub url: Option<String>,
//     pub user: Option<i64>,
//     pub language: Option<String>,
//     pub emoji_id: Option<String>,

//     // user fields
//     pub user_id: Option<i64>,
//     pub first_name: Option<String>,
//     pub last_name: Option<String>,
//     pub username: Option<String>,
//     pub is_bot: Option<bool>,
// }

// impl FiltersWithEntities {
//     fn get(self) -> (Option<Model>, Option<button::Model>, Option<EntityWithUser>) {
//         let button = if let (Some(button_text), Some(owner_id), Some(pos_x), Some(pos_y)) =
//             (self.button_text, self.b_owner_id, self.pos_x, self.pos_y)
//         {
//             Some(button::Model {
//                 button_text,
//                 owner_id: Some(owner_id),
//                 callback_data: self.callback_data,
//                 button_url: self.button_url,
//                 pos_x,
//                 pos_y,
//                 raw_text: self.raw_text,
//             })
//         } else {
//             None
//         };

//         let filter = if let (Some(id), Some(warn_user_id), Some(chat_id)) =
//             (self.id, self.warn_user_id, self.chat_id)
//         {
//             Some(Model {
//                 id,
//                 user_id: warn_user_id,
//                 chat_id,
//                 expires: self.expires,
//                 reason: self.reason,
//                 entity_id: self.entity_id,
//             })
//         } else {
//             None
//         };

//         let entity = if let (Some(tg_type), Some(offset), Some(length), Some(owner_id)) =
//             (self.tg_type, self.offset, self.length, self.owner_id)
//         {
//             Some(EntityWithUser {
//                 tg_type,
//                 offset,
//                 length,
//                 url: self.url,
//                 language: self.language,
//                 emoji_id: self.emoji_id,
//                 user: self.user,
//                 owner_id,
//                 user_id: self.user_id,
//                 first_name: self.first_name,
//                 last_name: self.last_name,
//                 username: self.username,
//                 is_bot: self.is_bot,
//             })
//         } else {
//             None
//         };

//         (filter, button, entity)
//     }
// }

// pub type FiltersMap = HashMap<Model, (Vec<EntityWithUser>, Vec<button::Model>)>;

// pub async fn get_filters_join<F>(filter: F) -> crate::util::error::Result<FiltersMap>
// where
//     F: IntoCondition,
// {
//     let res = Entity::find()
//         .select_only()
//         .columns([
//             Column::Id,
//             Column::ChatId,
//             Column::Expires,
//             Column::Reason,
//             Column::EntityId,
//         ])
//         .column_as(Column::UserId, "warn_user_id")
//         .columns([
//             messageentity::Column::TgType,
//             messageentity::Column::Offset,
//             messageentity::Column::Length,
//             messageentity::Column::Url,
//             messageentity::Column::User,
//             messageentity::Column::Language,
//             messageentity::Column::EmojiId,
//             messageentity::Column::OwnerId,
//         ])
//         .columns([
//             button::Column::ButtonText,
//             button::Column::CallbackData,
//             button::Column::ButtonUrl,
//             button::Column::PosX,
//             button::Column::PosY,
//             button::Column::RawText,
//         ])
//         .column_as(button::Column::OwnerId, "b_owner_id")
//         .columns([
//             users::Column::UserId,
//             users::Column::FirstName,
//             users::Column::LastName,
//             users::Column::Username,
//             users::Column::IsBot,
//         ])
//         .join(JoinType::LeftJoin, Relation::Entities.def())
//         .join(JoinType::LeftJoin, entity::Relation::EntitiesRev.def())
//         .join(JoinType::LeftJoin, entity::Relation::ButtonsRev.def())
//         .join(JoinType::LeftJoin, messageentity::Relation::Users.def())
//         .filter(filter)
//         .order_by_asc(button::Column::PosX)
//         .order_by_asc(button::Column::PosY)
//         .into_model::<FiltersWithEntities>()
//         .all(*DB)
//         .await?;

//     let res = res.into_iter().map(|v| v.get()).fold(
//         FiltersMap::new(),
//         |mut acc, (filter, button, entity)| {
//             if let Some(filter) = filter {
//                 let (entitylist, buttonlist) = acc
//                     .entry(filter)
//                     .or_insert_with(|| (Vec::new(), Vec::new()));

//                 if let Some(button) = button {
//                     buttonlist.push(button);
//                 }

//                 if let Some(entity) = entity {
//                     entitylist.push(entity);
//                 }
//             }
//             acc
//         },
//     );

//     //            log::info!("got {:?} filters from db", res);
//     Ok(res)
// }
impl ActiveModelBehavior for ActiveModel {}
