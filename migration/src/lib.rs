use crate::sea_orm::Statement;
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
mod m20231117_045213_taint;
mod m20240220_230802_no_cycle;

pub struct Migrator;

pub fn prevent_cycle(name: &str, col: &str) -> Statement {
    Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        format!(
            "
            CREATE FUNCTION {name}()
              RETURNS TRIGGER AS $$
            DECLARE
              rc INTEGER;
            BEGIN
              EXECUTE format(
                'WITH RECURSIVE search_graph(%2$I, path, cycle) AS (' ||
                  'SELECT t.%2$I, ARRAY[t.{fed}, t.%2$I], (t.{fed} = t.%2$I) ' ||
                    'FROM %1$I t ' ||
                    'WHERE t.{fed} = $1 ' ||
                  'UNION ALL ' ||
                  'SELECT t.%2$I, sg.path || t.%2$I, t.%2$I = ANY(sg.path) ' ||
                    'FROM search_graph sg ' ||
                    'JOIN %1$I t on t.{fed} = sg.%2$I ' ||
                    'WHERE NOT sg.cycle' ||
                  ') SELECT 1 FROM search_graph WHERE cycle LIMIT 1;',
                TG_ARGV[0], TG_ARGV[1]) USING NEW.{fed};
              GET DIAGNOSTICS rc = ROW_COUNT;
              IF rc > 0 THEN
                RAISE EXCEPTION 'Self-referential foreign key cycle detected';
              ELSE
                RETURN NEW;
              END IF;
            END
            $$ LANGUAGE plpgsql;    
            ",
            fed = col,
            name = name
        ),
    )
}

#[async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut module_migrations = dijkstra::modules::get_migrations();
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
            Box::new(m20231117_045213_taint::Migration),
        ];
        core_migrations.append(&mut module_migrations);
        core_migrations.append(&mut vec![Box::new(m20230629_231657_tags_idx::Migration)]);
        core_migrations.append(&mut vec![
            Box::new(m20231029_015614_notes::Migration),
            Box::new(m20231029_032907_notes_entity::Migration),
            Box::new(m20240220_230802_no_cycle::Migration),
        ]);
        core_migrations
    }
}
