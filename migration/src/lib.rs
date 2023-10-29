use async_trait::async_trait;
pub use sea_orm_migration::*;

mod m20220101_000001_create_table;
mod m20221217_150626_create_user;
mod m20230113_005856_addlang;
mod m20230118_045027_adminactions;
mod m20230211_202851_expires;
mod m20230214_000001_create_captcha;
mod m20230312_000001_create_welcomes;
mod m20230507_201523_dialog_permissions;
mod m20230509_133432_approvals;
mod m20230629_005040_rules;
mod m20230629_231657_tags_idx;
mod m20230712_063916_fbans;
mod m20230828_202520_user_names;
mod m20230910_204018_entity_in_db;
mod m20231029_015614_notes;
mod m20231029_032907_notes_entity;

pub struct Migrator;

#[async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut module_migrations = bot_impl::modules::get_migrations();
        let mut core_migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20220101_000001_create_table::Migration),
            Box::new(m20221217_150626_create_user::Migration),
            Box::new(m20230113_005856_addlang::Migration),
            Box::new(m20230118_045027_adminactions::Migration),
            Box::new(m20230211_202851_expires::Migration),
            Box::new(m20230507_201523_dialog_permissions::Migration),
            Box::new(m20230509_133432_approvals::Migration),
            Box::new(m20230629_005040_rules::Migration),
            Box::new(m20230712_063916_fbans::Migration),
            Box::new(m20230828_202520_user_names::Migration),
            Box::new(m20230312_000001_create_welcomes::Migration),
            Box::new(m20230214_000001_create_captcha::Migration),
            Box::new(m20230910_204018_entity_in_db::Migration),
        ];
        core_migrations.append(&mut module_migrations);
        core_migrations.append(&mut vec![Box::new(m20230629_231657_tags_idx::Migration)]);
        core_migrations.append(&mut vec![
            Box::new(m20231029_015614_notes::Migration),
            Box::new(m20231029_032907_notes_entity::Migration),
        ]);
        core_migrations
    }
}
