//! A catalog model's two formats, as Settings → Models offers them: one GGUF file for the
//! llama.cpp provider, or a folder for MLX on Apple-silicon Macs, downloaded or deleted one at
//! a time, as `carrel models download --format` and `carrel models remove --format` do. Each is
//! the owner's confirmed action.

use std::sync::Arc;

use carrel_core::models::{CATALOG, ModelError, mlx_dir, remove, remove_mlx};

use crate::{ArchiveHandle, CarrelError, ProgressListener, text};

/// A model's file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ModelFormat {
    /// One GGUF file, for the llama.cpp provider.
    Gguf,
    /// A folder, for the MLX provider on Apple-silicon Macs.
    Mlx,
}

/// The formats a catalog model comes in, and which are here.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelFormats {
    /// The model's short name.
    pub name: String,
    /// Whether its GGUF file is in the models folder.
    pub gguf_here: bool,
    /// The size of its MLX build in bytes, if it has one.
    pub mlx_size: Option<u64>,
    /// Whether its MLX build is in the models folder.
    pub mlx_here: bool,
}

#[uniffi::export]
impl ArchiveHandle {
    /// The formats of every catalog model that can be downloaded, and which are here.
    #[must_use]
    pub fn model_formats(&self) -> Vec<ModelFormats> {
        let dir = self.models_dir();
        CATALOG
            .iter()
            .filter(|m| m.comes_with.is_none() && m.is_pinned())
            .map(|m| ModelFormats {
                name: m.name.to_owned(),
                gguf_here: dir
                    .as_ref()
                    .is_some_and(|d| d.join(m.file_name()).is_file()),
                mlx_size: m.mlx.map(|b| b.size()),
                mlx_here: dir
                    .as_ref()
                    .is_some_and(|d| mlx_dir(d).join(m.name).is_dir()),
            })
            .collect()
    }

    /// Downloads the catalog model `name` in `format` into this device's models folder, as
    /// [`download_model`](Self::download_model) does. Call it only once the owner has
    /// confirmed, having seen the size. Returns where it was saved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a model that isn't in the catalog or has no build in
    /// that format, or [`CarrelError::Failed`] if the helper is missing or the download
    /// fails.
    pub fn download_model_in(
        &self,
        name: String,
        format: ModelFormat,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<String, CarrelError> {
        let dir = self
            .models_dir()
            .ok_or_else(|| CarrelError::failed("no models folder: set CARREL_MODELS_DIR"))?;
        match format {
            ModelFormat::Gguf => crate::models::download(&name, &dir, progress),
            ModelFormat::Mlx => download_mlx(&name, &dir, progress),
        }
    }

    /// Deletes the downloaded model `name` in `format` from this device's models folder,
    /// leaving the other format. Call it only once the owner has confirmed. Returns what was
    /// deleted.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if it isn't downloaded in that format, or
    /// [`CarrelError::Failed`] if it can't be deleted.
    pub fn remove_model_in(
        &self,
        name: String,
        format: ModelFormat,
    ) -> Result<String, CarrelError> {
        let dir = self
            .models_dir()
            .ok_or_else(|| CarrelError::failed("no models folder: set CARREL_MODELS_DIR"))?;
        let removed = match format {
            ModelFormat::Gguf => remove(&name, &dir),
            ModelFormat::Mlx => remove_mlx(&name, &dir),
        };
        match removed {
            Ok(path) => Ok(text(&path)),
            Err(e @ ModelError::NotDownloaded(_)) => Err(CarrelError::invalid(e)),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
fn download_mlx(
    name: &str,
    dir: &std::path::Path,
    progress: Option<Arc<dyn ProgressListener>>,
) -> Result<String, CarrelError> {
    use carrel_core::models::{Helper, find, network_log};
    let model = find(name)
        .filter(|m| m.comes_with.is_none() && m.is_pinned())
        .ok_or_else(|| CarrelError::invalid(format!("no model called {name} to download")))?;
    if model.mlx.is_none() {
        return Err(CarrelError::invalid(format!(
            "{name} has no MLX build; download it as GGUF instead"
        )));
    }
    let helper = Helper::find()?;
    let log = network_log();
    let path = carrel_core::models::download_mlx(
        model,
        dir,
        &helper,
        log.as_deref(),
        &mut |done, total| {
            if let Some(progress) = &progress {
                progress.update(done, total, Some(name.to_owned()));
            }
        },
    )?;
    Ok(text(&path))
}

#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
fn download_mlx(
    _name: &str,
    _dir: &std::path::Path,
    _progress: Option<Arc<dyn ProgressListener>>,
) -> Result<String, CarrelError> {
    Err(CarrelError::failed("models are downloaded on a Mac"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_format_is_listed_and_deleted_on_its_own() {
        // With CARREL_MODELS_DIR set, the models folder is the real one: leave it alone.
        if std::env::var_os("CARREL_MODELS_DIR").is_some() {
            return;
        }
        let sample = crate::tests::Sample::new();
        let dir = sample.handle.models_dir().unwrap();
        let model = CATALOG
            .iter()
            .find(|m| m.comes_with.is_none() && m.is_pinned() && m.mlx.is_some())
            .unwrap();
        std::fs::create_dir_all(mlx_dir(&dir).join(model.name)).unwrap();
        std::fs::write(dir.join(model.file_name()), "weights").unwrap();
        let formats = sample.handle.model_formats();
        let shown = formats.iter().find(|f| f.name == model.name).unwrap();
        assert!(shown.gguf_here && shown.mlx_here && shown.mlx_size.is_some());

        sample
            .handle
            .remove_model_in(model.name.to_owned(), ModelFormat::Mlx)
            .unwrap();
        let formats = sample.handle.model_formats();
        let shown = formats.iter().find(|f| f.name == model.name).unwrap();
        assert!(shown.gguf_here && !shown.mlx_here, "the GGUF file stays");
        assert!(matches!(
            sample
                .handle
                .remove_model_in(model.name.to_owned(), ModelFormat::Mlx),
            Err(CarrelError::Invalid { .. })
        ));
    }
}
