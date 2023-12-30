use dijkstra::persist::{
    admin::{fbans, fedadmin, federations, gbans},
    core::{chat_members, dialogs, users},
    migrate::ManagerHelper,
};
use sea_orm_migration::{
    prelude::*,
    sea_orm::{DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts

        manager
            .create_table(
                Table::create()
                    .table(federations::Entity)
                    .col(
                        ColumnDef::new(federations::Column::FedId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(federations::Column::Subscribed).uuid())
                    .col(
                        ColumnDef::new(federations::Column::Owner)
                            .big_integer()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(federations::Column::FedName)
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .query_one(crate::prevent_cycle(
                "prevent_cycle",
                &federations::Column::FedId.to_string(),
            ))
            .await?;

        manager
            .get_connection()
            .query_one(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "
                    CREATE TRIGGER prevent_cycle_trigger
                    AFTER INSERT OR UPDATE OF {col} ON {table}
                    FOR EACH ROW
                    EXECUTE PROCEDURE prevent_cycle('{table}', '{col}');
                    ",
                    col = federations::Column::Subscribed.to_string(),
                    table = federations::Entity.to_string(),
                ),
            ))
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(fedadmin::Entity)
                    .col(
                        ColumnDef::new(fedadmin::Column::Federation)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(fedadmin::Column::User)
                            .big_integer()
                            .unique_key()
                            .not_null(),
                    )
                    .primary_key(
                        IndexCreateStatement::new()
                            .table(fedadmin::Entity)
                            .col(fedadmin::Column::Federation)
                            .col(fedadmin::Column::User)
                            .primary(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(fbans::Entity)
                    .col(ColumnDef::new(fbans::Column::FbanId).uuid().primary_key())
                    .col(ColumnDef::new(fbans::Column::Federation).uuid().not_null())
                    .col(ColumnDef::new(fbans::Column::UserName).text())
                    .col(ColumnDef::new(fbans::Column::Reason).text())
                    .col(
                        ColumnDef::new(fbans::Column::User)
                            .big_integer()
                            .not_null()
                            .unique_key(),
                    )
                    .index(
                        Index::create()
                            .col(fbans::Column::User)
                            .col(fbans::Column::Federation)
                            .unique(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(gbans::Entity)
                    .col(ColumnDef::new(gbans::Column::Id).uuid().not_null())
                    .col(ColumnDef::new(gbans::Column::Reason).text())
                    .col(
                        ColumnDef::new(gbans::Column::User)
                            .big_integer()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(federations::Entity, federations::Column::Subscribed)
                    .to(federations::Entity, federations::Column::FedId)
                    .name("fk_subscriptions")
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(fedadmin::Entity, fedadmin::Column::Federation)
                    .to(federations::Entity, federations::Column::FedId)
                    .name("fk_fedadmin")
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(fbans::Entity, fbans::Column::Federation)
                    .to(federations::Entity, federations::Column::FedId)
                    .name("fk_fban")
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(fbans::Entity, fbans::Column::User)
                    .to(users::Entity, users::Column::UserId)
                    .name("fk_fban_user")
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(gbans::Entity, gbans::Column::User)
                    .to(users::Entity, users::Column::UserId)
                    .name("fk_gban_user")
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(fedadmin::Entity, fedadmin::Column::User)
                    .to(users::Entity, users::Column::UserId)
                    .name("fk_fedadmin_user")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(dialogs::Entity)
                    .add_column(ColumnDef::new(dialogs::Column::Federation).uuid())
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(dialogs::Entity, dialogs::Column::Federation)
                    .to(federations::Entity, federations::Column::FedId)
                    .name("fk_fed_dialog")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(chat_members::Entity)
                    .add_column(
                        ColumnDef::new(chat_members::Column::BannedByMe)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .name("fk_subscriptions")
                    .table(federations::Entity)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(fedadmin::Entity)
                    .name("fk_fedadmin")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(fbans::Entity)
                    .name("fk_fban")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(fbans::Entity)
                    .name("fk_fban_user")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(gbans::Entity)
                    .name("fk_gban_user")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(fedadmin::Entity)
                    .name("fk_fedadmin_user")
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(dialogs::Entity)
                    .name("fk_fed_dialog")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(dialogs::Entity)
                    .drop_column(dialogs::Column::Federation)
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .query_one(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "DROP TRIGGER prevent_cycle_trigger ON {};",
                    federations::Entity.to_string()
                ),
            ))
            .await?;

        manager
            .get_connection()
            .query_one(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "DROP FUNCTION prevent_cycle;",
            ))
            .await?;
        manager.drop_table_auto(federations::Entity).await?;
        manager.drop_table_auto(fedadmin::Entity).await?;
        manager.drop_table_auto(fbans::Entity).await?;
        manager.drop_table_auto(gbans::Entity).await?;
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(chat_members::Entity)
                    .drop_column(chat_members::Column::BannedByMe)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
