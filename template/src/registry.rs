use handlebars::{
    Context, Handlebars, Helper, HelperResult, Output, RenderContext, RenderError,
    RenderErrorReason,
};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use rust_embed::RustEmbed;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml::Value as TomlValue;

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

#[derive(RustEmbed)]
#[folder = "assets"]
pub struct Assets;

fn json_value_to_toml_value(json: &JsonValue) -> Option<TomlValue> {
    Some(match json {
        JsonValue::Null => return None,
        JsonValue::Bool(b) => TomlValue::Boolean(*b),
        JsonValue::String(s) => TomlValue::String(s.clone()),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                TomlValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                TomlValue::Float(f)
            } else {
                return None;
            }
        }
        JsonValue::Array(a) => TomlValue::Array(
            a.iter()
                .map(json_value_to_toml_value)
                .collect::<Option<_>>()?,
        ),
        JsonValue::Object(m) => TomlValue::Table(
            m.iter()
                .map(|(s, v)| Some((s.clone(), json_value_to_toml_value(v)?)))
                .collect::<Option<_>>()?,
        ),
    })
}

fn toml_val(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: &mut dyn Output,
) -> HelperResult {
    let value = json_value_to_toml_value(
        h.param(0)
            .ok_or_else(|| RenderErrorReason::ParamNotFoundForIndex("toml_val", 0))?
            .value(),
    )
    .ok_or_else(|| RenderErrorReason::InvalidParamType("TOML value"))?;
    out.write(value.to_string().as_str())?;
    Ok(())
}

pub struct Registry {
    handlebars: Handlebars<'static>,
}

impl Registry {
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();
        #[cfg(debug_assertions)]
        handlebars.set_dev_mode(true);
        handlebars
            .register_embed_templates_with_extension::<Assets>(".hbs")
            .unwrap();
        handlebars.register_helper("toml_val", Box::new(toml_val));
        Self { handlebars }
    }

    fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        File::create(path)?.write_all(bytes)
    }

    fn strip_hbs(path: &Path) -> PathBuf {
        let s = path.to_string_lossy();
        match s.strip_suffix(".hbs") {
            Some(stripped) => PathBuf::from(stripped),
            None => path.to_path_buf(),
        }
    }

    fn static_assets(prefix: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        Assets::iter()
            .filter_map(|entry| {
                let p = Path::new(entry.as_ref());
                (p.strip_prefix(prefix).ok())
                    .filter(|rel| !rel.extension().map(|e| e == "hbs").unwrap_or(false))
                    .and_then(|rel| {
                        Assets::get(entry.as_ref()).map(|f| (rel.to_path_buf(), f.data.into()))
                    })
            })
            .collect()
    }

    fn template_paths<'a>(&'a self, prefix: &'a Path) -> Vec<&'a str> {
        self.handlebars
            .get_templates()
            .keys()
            .map(String::as_str)
            .filter(|p| Path::new(p).starts_with(prefix))
            .collect()
    }

    pub fn render_static<W: Write>(&self, path: &str, mut writer: W) -> Result<(), RenderError> {
        writer.write_all(
            &Assets::get(path)
                .ok_or_else(|| RenderErrorReason::TemplateNotFound(path.to_string()))?
                .data,
        )?;
        Ok(())
    }

    pub fn render_template<W: Write, D: Serialize>(
        &self,
        path: &str,
        data: &D,
        writer: W,
    ) -> Result<(), RenderError> {
        self.handlebars.render_to_write(path, data, writer)?;
        Ok(())
    }

    pub fn render_all<D: Serialize + Sync>(
        &self,
        prefix: &str,
        data: &D,
        out_dir: impl AsRef<Path>,
    ) -> Result<(), RenderError> {
        let prefix = Path::new(prefix);
        let out_dir = out_dir.as_ref();
        fs::create_dir_all(out_dir)?;

        let assets = Self::static_assets(prefix);

        assets.into_par_iter().try_for_each(|(rel, bytes)| {
            Self::write_bytes(&out_dir.join(rel), &bytes).map_err(RenderError::from)
        })?;

        let templates = self.template_paths(prefix);

        templates.into_par_iter().try_for_each(|tpl_path| {
            let rel = Path::new(tpl_path).strip_prefix(prefix).unwrap();
            let target = out_dir.join(Self::strip_hbs(rel));
            let mut buf = Vec::new();
            self.render_template(tpl_path, data, &mut buf)?;
            Self::write_bytes(&target, &buf).map_err(RenderError::from)
        })?;

        Ok(())
    }

    pub fn render_template_with_filename<D: Serialize>(
        &self,
        template_path: &str,
        filename_template: &str,
        data: &D,
        base_out: impl AsRef<Path>,
    ) -> Result<(), RenderError> {
        let dest_name = self.handlebars.render_template(filename_template, data)?;
        let dest_path = base_out.as_ref().join(dest_name);
        let mut buf = Vec::new();
        self.render_template(template_path, data, &mut buf)?;
        Self::write_bytes(&dest_path, &buf).map_err(RenderError::from)
    }

    pub fn render_all_with_filename_templates<D: Serialize + Sync>(
        &self,
        prefix: &str,
        data: &D,
        out_dir: impl AsRef<Path>,
    ) -> Result<(), RenderError> {
        let prefix = Path::new(prefix);
        let out_dir = out_dir.as_ref();
        fs::create_dir_all(out_dir)?;

        let assets = Self::static_assets(prefix);

        assets.into_par_iter().try_for_each(|(rel, bytes)| {
            Self::write_bytes(&out_dir.join(rel), &bytes).map_err(RenderError::from)
        })?;

        let templates = self.template_paths(prefix);

        templates.into_par_iter().try_for_each(|tpl_path| {
            let rel = Path::new(tpl_path).strip_prefix(prefix).unwrap();
            let filename_template = Self::strip_hbs(rel).to_string_lossy().into_owned();
            self.render_template_with_filename(tpl_path, &filename_template, data, out_dir)
        })?;
        Ok(())
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
