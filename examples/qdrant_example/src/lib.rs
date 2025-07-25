mod version_20250522_145910_init_migration;

pub struct Migrator;

#[async_trait::async_trait]
impl vectorctl::MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn vectorctl::MigrationTrait>> {
        vec![Box::new(version_20250522_145910_init_migration::Migration)]
    }
}
