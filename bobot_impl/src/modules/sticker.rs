use sea_schema::migration::MigrationTrait;

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}
