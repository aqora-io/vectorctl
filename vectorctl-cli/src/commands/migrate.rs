use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use rand::{TryRngCore, rand_core::OsError, rngs::OsRng};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use tokio_stream::{StreamExt, wrappers::ReadDirStream};
use vectorctl_template::{
    MigrationTemplate, RenderError, migrator::MigratorTemplate, revision::RevisionTemplate,
};

pub const REVISION_PREFIX: &str = "version";
const DATE_FMT: &str = "%Y-%m-%dT%H:%M:%S";
const DATE_FILE_FMT: &str = "%Y%m%d_%H%M%S";
const MIGRATOR_FILENAME: &str = "lib.rs";

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Syntax parsing failed: {0}")]
    Parser(#[from] syn::Error),
    #[error("Secure RNG failed: {0}")]
    OsRng(#[from] OsError),
    #[error("Template rendering failed: {0}")]
    Render(#[from] RenderError),
}

type Result<T> = std::result::Result<T, MigrateError>;

fn revision_id() -> Result<String> {
    let mut bytes = [0u8; 8];
    OsRng.try_fill_bytes(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD
        .encode(bytes)
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect())
}

fn filename(stem: &str) -> String {
    format!(
        "{}_{}_{}",
        REVISION_PREFIX,
        Utc::now().format(DATE_FILE_FMT),
        stem
    )
}

fn src_dir(base: impl AsRef<Path>) -> PathBuf {
    let base = base.as_ref();
    let candidate = base.join("src");
    if candidate.is_dir() {
        candidate
    } else {
        base.to_owned()
    }
}

async fn render_revision(
    dir: impl AsRef<Path>,
    name: &str,
    down_rev: Option<&str>,
    message: Option<&str>,
) -> Result<()> {
    let mut builder = RevisionTemplate::builder();
    builder
        .date(Utc::now().format(DATE_FMT).to_string())
        .revision_id(revision_id()?)
        .filename(filename(name));
    if let Some(down_rev) = down_rev {
        builder.down_revision_id(down_rev);
    }
    if let Some(message) = message {
        builder.message(message);
    }
    builder.render(src_dir(dir))?;
    Ok(())
}

async fn render_migrator(dir: impl AsRef<Path>) -> Result<()> {
    let source_dir = src_dir(dir);
    let stems: Vec<String> = ReadDirStream::new(fs::read_dir(&source_dir).await?)
        .filter_map(|item| {
            item.ok()?
                .path()
                .file_name()
                .and_then(|stem| stem.to_str())
                .filter(|name| name.starts_with(REVISION_PREFIX) && name.ends_with(".rs"))
                .map(|name| name.trim_end_matches(".rs").to_owned())
        })
        .collect()
        .await;

    MigratorTemplate::builder()
        .imports(stems)
        .render(source_dir)?;

    Ok(())
}

#[derive(Debug)]
struct Backup(PathBuf);

impl Backup {
    async fn new(p: impl AsRef<Path>) -> Result<Self> {
        let orig = p.as_ref().to_owned();
        let bak = orig.with_extension("rs.bak");
        fs::copy(&orig, &bak).await?;
        Ok(Self(bak))
    }
    async fn commit(self) -> Result<()> {
        fs::remove_file(self.0).await?;
        Ok(())
    }
}

pub async fn init(
    pkg: Option<&str>,
    edition: Option<&str>,
    migration_dir: impl AsRef<Path>,
) -> Result<()> {
    let mut builer = MigrationTemplate::builder();
    if let Some(package_name) = pkg {
        builer.package_name(package_name);
    }
    if let Some(rust_edition) = edition {
        builer.rust_edition(rust_edition);
    }
    builer.render(&migration_dir)?;
    render_revision(&migration_dir, "init_migration", None, None).await?;
    render_migrator(migration_dir).await
}

pub async fn create_new_revision(
    migration_dir: impl AsRef<Path>,
    name: &str,
    down_rev: &str,
    message: Option<&str>,
) -> Result<()> {
    let migrator = src_dir(&migration_dir).join(MIGRATOR_FILENAME);
    let backup = Backup::new(&migrator).await?;
    render_revision(&migration_dir, name, Some(down_rev), message).await?;
    render_migrator(migration_dir).await?;
    backup.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use regex::Regex;
    use std::collections::HashSet;
    use tempfile::tempdir;
    use tokio_stream::{StreamExt, wrappers::ReadDirStream};

    fn id_re() -> Regex {
        Regex::new(r"^[A-Za-z]{1,11}$").unwrap()
    }

    #[test]
    fn id_format() {
        let id = revision_id().unwrap();
        assert!(id_re().is_match(&id));
    }

    #[test]
    fn uniqueness() {
        let mut seen = HashSet::with_capacity(4096);
        for _ in 0..4096 {
            seen.insert(revision_id().unwrap());
        }
        assert_eq!(seen.len(), 4096);
    }

    #[tokio::test]
    async fn init_creates_scaffold_and_revision() {
        let tmp = tempdir().unwrap();
        init(Some("pkg"), Some("2021"), tmp.path()).await.unwrap();
        assert!(tmp.path().join("Cargo.toml").exists());
        let revs: Vec<_> =
            ReadDirStream::new(tokio::fs::read_dir(src_dir(tmp.path())).await.unwrap())
                .collect()
                .await;
        assert!(!revs.is_empty());
    }

    #[tokio::test]
    async fn backup_is_removed() {
        let tmp = tempdir().unwrap();
        init(None, None, tmp.path()).await.unwrap();
        create_new_revision(tmp.path(), "add_tbl", "prev", Some("msg"))
            .await
            .unwrap();
        assert!(
            !tmp.path()
                .join("src")
                .join(format!("{MIGRATOR_FILENAME}.bak"))
                .exists()
        );
    }

    proptest! {
        #[test]
        fn prop_base64(buf in any::<[u8;8]>()) {
            let enc = URL_SAFE_NO_PAD.encode(buf).chars().filter(|c| c.is_alphabetic()).collect::<String>();
            prop_assert!(id_re().is_match(&enc));
        }
    }
}
