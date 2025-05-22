use std::{env, path::Path};

use derive_builder::Builder;
use handlebars::{RenderError, RenderErrorReason};
use lazy_regex::{Lazy, Regex, lazy_regex};
use serde::Serialize;

use crate::registry::REGISTRY;

static SEMVER_REGEX: Lazy<Regex> = lazy_regex!(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$"
);
static STRING_REGEX: Lazy<Regex> = lazy_regex!(r"^[a-zA-Z0-9_]+$");

const DEFAULT_RUST_EDITION: &str = "2021";
const DEFAULT_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PACKAGE_NAME: &str = "migration";

#[derive(Builder, Serialize, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct MigrationTemplate {
    #[builder(setter(into), default = "DEFAULT_RUST_EDITION.to_string()")]
    rust_edition: String,
    #[builder(setter(into), default = "DEFAULT_VERSION.to_string()")]
    version: String,
    #[builder(setter(into), default = "DEFAULT_PACKAGE_NAME.to_string()")]
    package_name: String,
}

impl MigrationTemplate {
    pub fn builder() -> MigrationTemplateBuilder {
        MigrationTemplateBuilder::default()
    }

    pub fn render(&self, out: impl AsRef<Path>) -> Result<(), RenderError> {
        REGISTRY.render_all("migration", self, out)
    }
}

impl MigrationTemplateBuilder {
    fn validate(&self) -> Result<(), String> {
        if let Some(version) = self.version.as_ref() {
            if !SEMVER_REGEX.is_match(version) {
                return Err(format!("Invalid version: {}", version));
            }
        }

        if let Some(package_name) = self.package_name.as_ref() {
            if !STRING_REGEX.is_match(package_name) {
                return Err(format!("Invalid package_name: {}", package_name));
            }
        }

        if let Some(rust_edition) = self.rust_edition.as_ref() {
            if !STRING_REGEX.is_match(rust_edition) {
                return Err(format!("Invalid rust edition: {}", rust_edition));
            }
        }

        Ok(())
    }

    pub fn render(&self, out: impl AsRef<Path>) -> Result<(), RenderError> {
        self.build()
            .map_err(|err| RenderErrorReason::Other(err.to_string()))?
            .render(out)
    }
}
