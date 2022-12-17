use async_trait::async_trait;
pub use sea_orm_migration::*;

mod m20220101_000001_create_table;
mod m20221217_150626_create_user;

pub struct Migrator;

#[async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut module_migrations = bobot_impl::modules::get_migrations();
        let mut core_migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20220101_000001_create_table::Migration),
            Box::new(m20221217_150626_create_user::Migration),
        ];
        core_migrations.append(&mut module_migrations);
        core_migrations
    }
}
