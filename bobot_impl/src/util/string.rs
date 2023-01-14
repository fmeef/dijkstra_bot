use crate::statics::DB;
use anyhow::Result;

use lazy_static::__Deref;
use macros::get_langs;

get_langs!();

pub use langs::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{prelude::ChronoDateTimeWithTimeZone, EntityTrait, IntoActiveModel};

use crate::persist::core::dialogs;

pub(crate) async fn get_chat_lang(chat: i64) -> Result<Lang> {
    Ok(dialogs::Entity::find_by_id(chat)
        .one(DB.deref().deref())
        .await?
        .map(|v| v.language)
        .unwrap_or_else(|| Lang::En))
}

pub(crate) async fn set_chat_lang(chat: i64, lang: Lang) -> Result<()> {
    let c = dialogs::Model {
        chat_id: chat,
        last_activity: ChronoDateTimeWithTimeZone::default(),
        language: lang,
    };
    dialogs::Entity::insert(c.into_active_model())
        .on_conflict(
            OnConflict::column(dialogs::Column::Language)
                .update_column(dialogs::Column::Language)
                .to_owned(),
        )
        .exec(DB.deref().deref())
        .await?;

    Ok(())
}
