use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(Some(1))")]
pub enum ChatType {
    #[sea_orm(string_value = "Private")]
    Private,
    #[sea_orm(string_value = "Group")]
    Group,
    #[sea_orm(string_value = "Supergroup")]
    Supergroup,
}
