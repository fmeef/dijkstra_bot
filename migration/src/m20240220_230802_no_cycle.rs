use dijkstra::{
    persist::admin::federations,
    sea_orm::{DatabaseBackend, Statement},
};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
        Ok(())
    }
}
