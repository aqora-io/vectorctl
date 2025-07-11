use std::path::Path;

use derive_builder::Builder;
use handlebars::{RenderError, RenderErrorReason};
use serde::Serialize;

use crate::registry::REGISTRY;

#[derive(Builder, Serialize, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct MigratorTemplate {
    #[builder(setter(into), default)]
    imports: Vec<String>,
}

impl MigratorTemplate {
    pub fn builder() -> MigratorTemplateBuilder {
        MigratorTemplateBuilder::default()
    }

    pub fn render(&self, out: impl AsRef<Path>) -> Result<(), RenderError> {
        REGISTRY.render_all("migrator", self, out)
    }
}

impl MigratorTemplateBuilder {
    fn validate(&self) -> Result<(), String> {
        if let Some(imports) = self.imports.as_ref() {
            if imports.is_empty() {
                return Err("You should at least define one migration.".into());
            }
        } else {
            return Err("imports is required".into());
        }

        Ok(())
    }

    pub fn render(&self, out: impl AsRef<Path>) -> Result<(), RenderError> {
        self.build()
            .map_err(|err| RenderErrorReason::Other(err.to_string()))?
            .render(out)
    }
}
