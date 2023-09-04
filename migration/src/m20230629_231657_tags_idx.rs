use sea_orm_migration::{
    prelude::*,
    sea_orm::{DbBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "CREATE EXTENSION IF NOT EXISTS pg_trgm;".to_owned(),
            ))
            .await?;
        manager
            .get_connection()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "create index idx_gin on tags using gin (tag gin_trgm_ops);".to_owned(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "DROP INDEX idx_gin".to_owned(),
            ))
            .await?;

        manager
            .get_connection()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "DROP EXTENSION IF EXISTS pg_trgm;".to_owned(),
            ))
            .await?;

        Ok(())
    }
}
