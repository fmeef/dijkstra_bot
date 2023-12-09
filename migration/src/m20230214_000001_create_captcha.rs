use chrono::Duration;

use dijkstra::persist::{
    admin::{
        authorized,
        captchastate::{self, CaptchaType},
    },
    migrate::ManagerHelper,
};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(captchastate::Entity)
                    .col(
                        ColumnDef::new(captchastate::Column::Chat)
                            .big_integer()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(captchastate::Column::CaptchaType)
                            .integer()
                            .not_null()
                            .default(CaptchaType::Button),
                    )
                    .col(
                        ColumnDef::new(captchastate::Column::KickTime)
                            .big_integer()
                            .default(Duration::minutes(1).num_seconds()),
                    )
                    .col(ColumnDef::new(captchastate::Column::CaptchaText).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(authorized::Entity)
                    .col(
                        ColumnDef::new(authorized::Column::Chat)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(authorized::Column::User)
                            .big_integer()
                            .not_null(),
                    )
                    .primary_key(
                        IndexCreateStatement::new()
                            .col(authorized::Column::Chat)
                            .col(authorized::Column::User)
                            .primary(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        manager.drop_table_auto(captchastate::Entity).await?;
        manager.drop_table_auto(authorized::Entity).await?;
        Ok(())
    }
}
