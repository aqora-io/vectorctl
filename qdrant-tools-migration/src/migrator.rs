use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::SystemTime,
};

use qdrant_client::Qdrant;
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::{create_dir_all, read_to_string, write};

use crate::MigrationTrait;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QdrantMigrationHistory {
    name: String,
    applied_at: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QdrantMigrationFile {
    version: Version,
    history: Vec<QdrantMigrationHistory>,
}

#[derive(Debug, Error)]
pub enum FileError {
    #[error("{0} is empty. Run a migration first")]
    Empty(String),
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error("Migration {0} already exist")]
    AlreadyExist(String),
    #[error("Migration {0}  do not exist")]
    DoNotExist(String),
    #[error("{0}")]
    Missing(String),
    #[error(transparent)]
    File(#[from] FileError),
}

pub async fn add_migration_history(
    path: impl AsRef<Path>,
    migration_name: &str,
    version: Version,
) -> Result<(), MigrationError> {
    let path = path.as_ref();
    let file_opt: Option<QdrantMigrationFile> = if path.exists() {
        Some(serde_json::from_str(&read_to_string(path).await?)?)
    } else {
        None
    };

    let file_version = file_opt
        .as_ref()
        .map(|f| f.version.clone())
        .unwrap_or(version);

    let history = file_opt.map(|f| f.history).unwrap_or_default();

    if history.iter().any(|h| h.name == migration_name) {
        return Err(MigrationError::AlreadyExist(migration_name.to_owned()));
    }

    let new_file = QdrantMigrationFile {
        version: file_version,
        history: history
            .into_iter()
            .chain(std::iter::once(QdrantMigrationHistory {
                name: migration_name.to_owned(),
                applied_at: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("SystemTime before UNIX EPOCH!")
                    .as_secs(),
            }))
            .collect(),
    };

    if let Some(dir) = path.parent() {
        create_dir_all(dir).await?;
    }

    write(path, serde_json::to_string_pretty(&new_file)?).await?;
    Ok(())
}

pub async fn delete_migration_history(
    path: impl AsRef<Path>,
    migration_name: &str,
    version: Version,
) -> Result<(), MigrationError> {
    let path = path.as_ref();
    let file_opt: Option<QdrantMigrationFile> = if path.exists() {
        Some(serde_json::from_str(&read_to_string(path).await?)?)
    } else {
        None
    };

    let file_version = file_opt
        .as_ref()
        .map(|f| f.version.clone())
        .unwrap_or(version);

    let history = file_opt.map(|f| f.history).unwrap_or_default();

    if !history.iter().any(|h| h.name == migration_name) {
        return Err(MigrationError::DoNotExist(migration_name.to_owned()));
    }

    let new_file = QdrantMigrationFile {
        version: file_version,
        history: history
            .into_iter()
            .filter(|h| h.name != migration_name)
            .collect(),
    };

    if let Some(dir) = path.parent() {
        create_dir_all(dir).await?;
    }

    write(path, serde_json::to_string_pretty(&new_file)?).await?;
    Ok(())
}

async fn get_migrations(
    path: impl AsRef<Path>,
) -> Result<Vec<QdrantMigrationHistory>, MigrationError> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(MigrationError::File(FileError::Empty(
            path.to_string_lossy().to_string(),
        )));
    }
    Ok(serde_json::from_str::<QdrantMigrationFile>(&read_to_string(path).await?)?.history)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MigrationStatus {
    Pending,
    Applied,
}

impl std::fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Pending => "Pending",
                Self::Applied => "Applied",
            }
        )
    }
}

pub struct Migration<Db: Send + Sync + 'static> {
    migration: Box<dyn MigrationTrait<Db = Db>>,
    status: MigrationStatus,
}

#[async_trait::async_trait]
pub trait MigratorTrait: Send {
    type Db: Send + Sync + 'static;

    fn migrations() -> Vec<Box<dyn MigrationTrait<Db = Self::Db>>>;
    fn history_path() -> PathBuf;

    fn get_migration_files() -> Vec<Migration<Self::Db>> {
        Self::migrations()
            .into_iter()
            .map(|migration| Migration {
                migration,
                status: MigrationStatus::Pending,
            })
            .collect()
    }

    async fn get_migration_json_file() -> Result<HashSet<String>, MigrationError> {
        Ok(get_migrations(Self::history_path())
            .await?
            .into_iter()
            .map(|h| h.name)
            .collect())
    }

    async fn get_migration_with_status() -> Result<Vec<Migration<Self::Db>>, MigrationError> {
        let migration_files = Self::get_migration_files();
        let migration_in_db = Self::get_migration_json_file().await?;
        let migration_in_fs: HashSet<_> = migration_files
            .iter()
            .map(|f| f.migration.name().to_string())
            .collect();

        let missing_migrations_in_fs = &migration_in_db - &migration_in_fs;

        let migrations = migration_files
            .into_iter()
            .map(|file| {
                let status = if migration_in_db.contains(file.migration.name()) {
                    MigrationStatus::Applied
                } else {
                    MigrationStatus::Pending
                };
                Migration {
                    migration: file.migration,
                    status,
                }
            })
            .collect::<Vec<_>>();

        if missing_migrations_in_fs.is_empty() {
            Ok(migrations)
        } else {
            Err(MigrationError::Missing(
                missing_migrations_in_fs
                    .into_iter()
                    .map(|migration| format!(
                        "Migration file of version '{migration}' is missing, this migration has been applied but its file is missing"
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
    }

    async fn get_filtered_migrations_by_status(
        status: MigrationStatus,
    ) -> Result<Vec<Migration<Self::Db>>, MigrationError> {
        Ok(Self::get_migration_with_status()
            .await?
            .into_iter()
            .filter(|file| file.status == status)
            .collect())
    }

    async fn status() -> Result<(), MigrationError> {
        Self::get_migration_with_status()
            .await?
            .into_iter()
            .for_each(|migration| {
                println!(
                    "Migration `{}`, status : `{}`",
                    migration.migration.name(),
                    migration.status
                )
            });
        Ok(())
    }

    async fn refresh(qdrant: &Qdrant, db: &Self::Db) -> Result<(), MigrationError> {
        exec_down::<Self>(qdrant, db, Self::history_path()).await?;
        exec_up::<Self>(qdrant, db, Self::history_path()).await
    }

    async fn reset(qdrant: &Qdrant, db: &Self::Db) -> Result<(), MigrationError> {
        exec_down::<Self>(qdrant, db, Self::history_path()).await
    }

    async fn up(qdrant: &Qdrant, db: &Self::Db) -> Result<(), MigrationError> {
        exec_up::<Self>(qdrant, db, Self::history_path()).await
    }

    async fn down(qdrant: &Qdrant, db: &Self::Db) -> Result<(), MigrationError> {
        exec_down::<Self>(qdrant, db, Self::history_path()).await
    }
}

async fn exec_up<M>(
    qdrant: &Qdrant,
    db: &M::Db,
    path: impl AsRef<Path>,
) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    for migration in M::get_filtered_migrations_by_status(MigrationStatus::Pending).await? {
        migration.migration.up(qdrant, db).await?;
        println!(
            "applying {}, {}",
            migration.migration.name(),
            migration.migration.description()
        );
        add_migration_history(
            &path,
            migration.migration.name(),
            semver::Version::new(0, 0, 0),
        )
        .await?;
    }
    Ok(())
}

async fn exec_down<M>(
    qdrant: &Qdrant,
    db: &M::Db,
    path: impl AsRef<Path>,
) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    for migration in M::get_filtered_migrations_by_status(MigrationStatus::Applied)
        .await?
        .into_iter()
        .rev()
    {
        migration.migration.down(qdrant, db).await?;
        delete_migration_history(
            &path,
            migration.migration.name(),
            semver::Version::new(0, 0, 0),
        )
        .await?;
    }
    Ok(())
}
