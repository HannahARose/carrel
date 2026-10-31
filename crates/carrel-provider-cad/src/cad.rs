//! The provider: a STEP or IGES file's header and parts (read as `carrel-provider-extract`
//! reads them), its size, solids and volume from `OpenCascade`, and four views of it as a
//! picture for an image reader to describe.

use std::fmt::Write as _;
use std::io::Write as _;

use carrel_core::providers::{
    Capability, Extraction, ExtractionRequest, Offer, Picture, Provider, ProviderError, priority,
};

use crate::{occt, png, render};

/// The offer for drawing CAD models.
pub const CAD_MODEL: &str = "cad-model";

/// Draws STEP and IGES models.
pub struct Cad {
    offers: Vec<Offer>,
}

impl Cad {
    /// The provider, available when it was built with `OpenCascade`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offers: vec![Offer {
                capability: Capability::ExtractText,
                id: CAD_MODEL.into(),
                label: Some("CAD models (OpenCascade)".into()),
                priority: priority::PORTABLE,
                available: occt::AVAILABLE,
                reason: (!occt::AVAILABLE).then(|| occt::INSTALL.to_owned()),
                media_types: vec!["model/step".into(), "model/iges".into()],
                extra: serde_json::Map::new(),
            }],
        }
    }

    fn read(request: &ExtractionRequest) -> Result<Extraction, ProviderError> {
        let iges = match request.media_type.as_str() {
            "model/step" => false,
            "model/iges" => true,
            other => {
                return Err(ProviderError::Unsupported(format!(
                    "{CAD_MODEL} doesn't read {other}"
                )));
            }
        };
        let header = carrel_provider_extract::models::read(&request.path, &request.media_type);
        let mesh = occt::mesh(&request.path, iges)
            .map_err(|e| ProviderError::Failed(format!("{}: {e}", request.path.display())))?;
        let Some((low, high)) = render::bounds(&mesh.positions) else {
            return Err(ProviderError::Failed(format!(
                "{} has no geometry to draw",
                request.path.display()
            )));
        };
        let size = [0, 1, 2]
            .map(|i| number(f64::from(high[i] - low[i])))
            .join(" × ");
        let (mut text, created) = match header {
            Ok(header) => (header.text, header.created),
            Err(_) => (
                format!("CAD model ({})", if iges { "IGES" } else { "STEP" }),
                None,
            ),
        };
        write!(text, "\nSize: {size} mm").unwrap();
        if mesh.solids > 0 {
            write!(text, "\nSolids: {}", mesh.solids).unwrap();
            write!(text, "\nVolume: {} cm³", number(mesh.volume / 1000.0)).unwrap();
        }
        let pictures = render::views(&mesh.positions, &mesh.triangles)
            .and_then(|image| write_picture(&image))
            .map(|path| Picture {
                path,
                context: format!(
                    "four views of a CAD model: angled (top left), front (top right), top \
                     (bottom left) and side (bottom right); it measures {size} mm"
                ),
            })
            .into_iter()
            .collect();
        Ok(Extraction {
            text,
            method: "geometry".into(),
            pages: None,
            created,
            located: false,
            pictures,
        })
    }
}

impl Default for Cad {
    fn default() -> Self {
        Self::new()
    }
}

/// `image` as a PNG in the temporary folder, where Carrel reads and deletes it.
fn write_picture(image: &render::Image) -> Option<std::path::PathBuf> {
    let mut file = tempfile::Builder::new()
        .prefix("carrel-picture-")
        .suffix(".png")
        .tempfile()
        .ok()?;
    file.write_all(&png::encode(image)).ok()?;
    file.into_temp_path().keep().ok()
}

/// Three significant figures, more for whole numbers: `86`, `25.5`, `0.0912`, `1234`.
fn number(value: f64) -> String {
    if value <= 0.0 || !value.is_finite() {
        return "0".to_owned();
    }
    #[allow(clippy::cast_possible_truncation)] // a small exponent
    let magnitude = value.log10().floor() as i32;
    let decimals = usize::try_from((2 - magnitude).clamp(0, 6)).unwrap_or(0);
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

impl Provider for Cad {
    fn name(&self) -> &'static str {
        "cad"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn extract(
        &self,
        offer: &str,
        request: &ExtractionRequest,
    ) -> Result<Extraction, ProviderError> {
        if offer != CAD_MODEL {
            return Err(ProviderError::Unsupported(format!(
                "cad has no offer {offer}"
            )));
        }
        if !occt::AVAILABLE {
            return Err(ProviderError::Unavailable(occt::INSTALL.into()));
        }
        Self::read(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_three_figures() {
        assert_eq!(number(86.0), "86");
        assert_eq!(number(16.93), "16.9");
        assert_eq!(number(0.0912), "0.0912");
        assert_eq!(number(1234.4), "1234");
        assert_eq!(number(0.0), "0");
    }

    #[test]
    fn the_offer_says_whether_opencascade_is_here() {
        let cad = Cad::new();
        let offer = &cad.offers()[0];
        assert_eq!(offer.available, occt::AVAILABLE);
        assert_eq!(offer.priority, priority::PORTABLE);
        if !occt::AVAILABLE {
            assert!(offer.reason.as_deref().unwrap().contains("opencascade"));
        }
    }

    #[test]
    fn other_types_are_unsupported() {
        let request = ExtractionRequest {
            path: "/x.stl".into(),
            media_type: "model/stl".into(),
            context: None,
        };
        assert!(matches!(
            Cad::read(&request),
            Err(ProviderError::Unsupported(_))
        ));
    }
}
