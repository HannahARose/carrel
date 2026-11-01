//! Creating an archive with a chosen embedding model, as `carrel init --embedding-model`
//! does, and the models to choose from before there's an archive to ask.

use std::path::Path;
use std::sync::Arc;

use carrel_core::settings::Key;

use crate::{ArchiveHandle, CarrelError, create_archive};

/// The embedding models a new archive can use, as `carrel init --embedding-model` takes them.
#[must_use]
#[uniffi::export]
pub fn embedding_model_choices() -> Vec<String> {
    Key::EmbeddingModel
        .choices()
        .map(|c| c.iter().map(|&c| c.to_owned()).collect())
        .unwrap_or_default()
}

/// The embedding model a new archive uses unless another is chosen.
#[must_use]
#[uniffi::export]
pub fn default_embedding_model() -> String {
    carrel_core::models::DEFAULT_EMBEDDING_MODEL.to_owned()
}

#[uniffi::export]
impl ArchiveHandle {
    /// Creates an archive named `name` in the empty folder `path`, as
    /// [`ArchiveHandle::create`] does, with `embedding_model` as its embedding model.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if `embedding_model` isn't one of
    /// [`embedding_model_choices`], or as [`ArchiveHandle::create`].
    #[uniffi::constructor]
    pub fn create_with_embedding_model(
        path: String,
        name: String,
        embedding_model: String,
    ) -> Result<Arc<Self>, CarrelError> {
        let choices = embedding_model_choices();
        if !choices.contains(&embedding_model) {
            return Err(CarrelError::invalid(format!(
                "no embedding model {embedding_model}; choose one of {}",
                choices.join(", ")
            )));
        }
        create_archive(
            Path::new(&path),
            &name,
            Some(&embedding_model),
            &carrel_core::local::config_folder()?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_archive_uses_the_embedding_model_chosen() {
        let choices = embedding_model_choices();
        assert!(choices.contains(&default_embedding_model()));
        let other = choices
            .iter()
            .find(|c| **c != default_embedding_model())
            .expect("a second embedding model to choose")
            .clone();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        let config = dir.path().join("config");
        let handle = create_archive(&root, "Letters", Some(&other), &config).unwrap();
        assert_eq!(
            handle.archive.settings().embedding_model(),
            Some(other.as_str())
        );
        assert!(
            ArchiveHandle::create_with_embedding_model(
                crate::text(&dir.path().join("another")),
                "Another".into(),
                "no-such-model".into()
            )
            .is_err()
        );
    }
}
