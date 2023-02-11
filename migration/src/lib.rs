use async_trait::async_trait;
pub use sea_orm_migration::*;

mod m20220101_000001_create_table;
mod m20221217_150626_create_user;
mod m20230113_005856_addlang;
mod m20230118_045027_adminactions;
mod m20230211_202851_expires;

pub struct Migrator;

#[async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut module_migrations = bobot_impl::modules::get_migrations();
        let mut core_migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20220101_000001_create_table::Migration),
            Box::new(m20221217_150626_create_user::Migration),
            Box::new(m20230113_005856_addlang::Migration),
            Box::new(m20230118_045027_adminactions::Migration),
            Box::new(m20230211_202851_expires::Migration),
        ];
        core_migrations.append(&mut module_migrations);
        core_migrations
    }
}
