use std::path::Path;

use derive_builder::Builder;
use handlebars::{RenderError, RenderErrorReason};
use lazy_regex::{Lazy, Regex, lazy_regex};
use serde::Serialize;

use crate::registry::REGISTRY;

static FILENAME_REGEX: Lazy<Regex> = lazy_regex!(r"^[a-zA-Z][a-zA-Z0-9_]*$");
static ISO_DATE_REGEX: Lazy<Regex> = lazy_regex!(r"^\d{4}-\d{2}-\d{2}(?:[ T]\d{2}:\d{2}:\d{2})?$");
static REVISION_ID_REGEX: Lazy<Regex> = lazy_regex!(r"^[A-Za-z0-9_-]+$");

#[derive(Builder, Serialize, Debug)]
#[builder(build_fn(validate = "Self::validate"))]
pub struct RevisionTemplate {
    #[builder(setter(into))]
    filename: String,
    #[builder(setter(into, strip_option), default)]
    message: Option<String>,
    #[builder(setter(into))]
    date: String,
    #[builder(setter(into))]
    revision_id: String,
    #[builder(setter(into, strip_option), default)]
    down_revision_id: Option<String>,
}

impl RevisionTemplate {
    pub fn builder() -> RevisionTemplateBuilder {
        RevisionTemplateBuilder::default()
    }

    pub fn render(&self, out: impl AsRef<Path>) -> Result<(), RenderError> {
        REGISTRY.render_all_with_filename_templates("revision", self, out)
    }
}

impl RevisionTemplateBuilder {
    fn validate(&self) -> Result<(), String> {
        if let Some(filename) = self.filename.as_ref() {
            if !FILENAME_REGEX.is_match(filename) {
                return Err(format!(
                    "Invalid Rust filename `{}`. It must match `{}`",
                    filename,
                    FILENAME_REGEX.as_str()
                ));
            }
        } else {
            return Err("filename is required".into());
        }

        if let Some(date) = self.date.as_ref() {
            if !ISO_DATE_REGEX.is_match(date) {
                return Err(format!(
                    "date `{}` must be ISO-8601: YYYY-MM-DD, YYYY-MM-DD HH:MM:SS, or YYYY-MM-DDT HH:MM:SS",
                    date
                ));
            }
        } else {
            return Err("date is required".into());
        }

        if let Some(revision_id) = self.revision_id.as_ref() {
            if !REVISION_ID_REGEX.is_match(revision_id) {
                return Err(format!(
                    "revision_id `{}` may only contain letters, numbers, dashes or underscores",
                    revision_id
                ));
            }
        } else {
            return Err("revision_id is required".into());
        }

        if let Some(Some(down_revision_id)) = self.down_revision_id.as_ref() {
            if !REVISION_ID_REGEX.is_match(down_revision_id) {
                return Err(format!(
                    "down_revision_id `{}` may only contain letters, numbers, dashes or underscores",
                    down_revision_id
                ));
            }
        }

        if let Some(Some(message)) = self.message.as_ref() {
            if message.trim().is_empty() {
                return Err("message cannot be empty".into());
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
