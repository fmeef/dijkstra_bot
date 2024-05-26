
[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, Debug)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum ActionType {
    #[sea_orm(num_value = 1)]
    Mute,
    #[sea_orm(num_value = 2)]
    Ban,
    #[sea_orm(num_value = 3)]
    Shame,
    #[sea_orm(num_value = 4)]
    Warn,
    #[sea_orm(num_value = 5)]
    Delete,
}
