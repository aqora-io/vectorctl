use migration::MigrationTrait;
pub use migration::migrator::MigratorTrait;
mod version_20250101_011111_init_migration;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(version_20250101_011111_init_migration::Migration)]
    }
}
