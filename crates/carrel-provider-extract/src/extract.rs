//! The extractors: a PDF's text layer, read in Rust, and OCR through the `tesseract` program
//! when it's installed (with `pdftoppm` for scanned PDFs).

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;

use carrel_core::providers::process::{default_folders, find_program};
use carrel_core::providers::{
    Capability, Extraction, ExtractionRequest, Offer, Provider, ProviderError, priority,
};

/// The offer for PDF text layers.
pub const PDF_TEXT: &str = "pdf-text";

/// The offer for OCR.
pub const TESSERACT: &str = "tesseract";

/// Below this many letters and digits a page, a PDF's text layer is treated as missing, as in
/// a scan, so OCR is tried instead.
const MIN_CHARS_PER_PAGE: usize = 10;

/// The most pages of a scanned PDF that are recognized, so one long scan can't take longer
/// than a request may.
pub const MAX_OCR_PAGES: u32 = 30;

/// The resolution scanned pages are rendered at for OCR, in dots per inch.
const OCR_DPI: &str = "300";

/// The image types Tesseract reads.
const IMAGES: [&str; 6] = [
    "image/png",
    "image/jpeg",
    "image/tiff",
    "image/bmp",
    "image/gif",
    "image/webp",
];

/// What to tell someone without Tesseract.
const INSTALL_TESSERACT: &str = "Tesseract isn't installed: brew install tesseract, apt install tesseract-ocr, or winget install UB-Mannheim.TesseractOCR";

/// Portable text extraction.
pub struct Extractor {
    offers: Vec<Offer>,
    tesseract: Option<PathBuf>,
    pdftoppm: Option<PathBuf>,
    languages: String,
}

impl Extractor {
    /// The extractors this device can use, finding `tesseract` and `pdftoppm` beside the
    /// provider or on `PATH`. OCR reads the languages in `CARREL_OCR_LANGUAGES` (Tesseract's
    /// form, such as `eng+deu`), English by default.
    #[must_use]
    pub fn new() -> Self {
        let folders = default_folders();
        Self::with(
            find_program("tesseract", &folders),
            find_program("pdftoppm", &folders),
            std::env::var("CARREL_OCR_LANGUAGES")
                .ok()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| "eng".to_owned()),
        )
    }

    /// The extractors, with these programs.
    #[must_use]
    pub fn with(tesseract: Option<PathBuf>, pdftoppm: Option<PathBuf>, languages: String) -> Self {
        let mut ocr_types: Vec<String> = IMAGES.map(str::to_owned).to_vec();
        if pdftoppm.is_some() {
            ocr_types.push("application/pdf".to_owned());
        }
        let offers = vec![
            // Listed first: a PDF's own text is exact, so it's tried before OCR.
            Offer {
                capability: Capability::ExtractText,
                id: PDF_TEXT.into(),
                label: Some("PDF text".into()),
                priority: priority::PORTABLE,
                available: true,
                reason: None,
                media_types: vec!["application/pdf".into()],
                extra: serde_json::Map::new(),
            },
            Offer {
                capability: Capability::ExtractText,
                id: TESSERACT.into(),
                label: Some("Tesseract OCR".into()),
                priority: priority::PORTABLE,
                available: tesseract.is_some(),
                reason: tesseract.is_none().then(|| INSTALL_TESSERACT.to_owned()),
                media_types: ocr_types,
                extra: serde_json::Map::new(),
            },
        ];
        Self {
            offers,
            tesseract,
            pdftoppm,
            languages,
        }
    }

    fn pdf_text(path: &Path) -> Result<Extraction, ProviderError> {
        // The PDF parser can panic on unusual files; that fails this file, not the provider.
        let pages = std::panic::catch_unwind(AssertUnwindSafe(|| {
            pdf_extract::extract_text_by_pages(path)
        }))
        .map_err(|_| ProviderError::Failed(format!("{}: couldn't be parsed", path.display())))?
        .map_err(|e| ProviderError::Failed(format!("{}: {e}", path.display())))?;
        let count = u32::try_from(pages.len()).unwrap_or(u32::MAX);
        let text = pages.join("\n\n");
        let meaningful = text.chars().filter(|c| c.is_alphanumeric()).count();
        if pages.is_empty() || meaningful < MIN_CHARS_PER_PAGE * pages.len() {
            return Err(ProviderError::Unsupported(format!(
                "{} has no text layer to speak of; it needs OCR",
                path.display()
            )));
        }
        Ok(Extraction {
            text,
            method: "embedded".into(),
            pages: Some(count),
            created: None,
            located: false,
        })
    }

    fn ocr(&self, request: &ExtractionRequest) -> Result<Extraction, ProviderError> {
        let tesseract = self
            .tesseract
            .as_deref()
            .ok_or_else(|| ProviderError::Unavailable(INSTALL_TESSERACT.into()))?;
        if request.media_type != "application/pdf" {
            return Ok(Extraction {
                text: self.recognize(tesseract, &request.path)?,
                method: "ocr".into(),
                pages: None,
                created: None,
                located: false,
            });
        }
        let pdftoppm = self.pdftoppm.as_deref().ok_or_else(|| {
            ProviderError::Unsupported("OCR of PDFs needs pdftoppm (from Poppler)".into())
        })?;
        let work = tempfile::tempdir()
            .map_err(|e| ProviderError::Failed(format!("no temporary folder: {e}")))?;
        let prefix = work.path().join("page");
        run(Command::new(pdftoppm)
            .args(["-r", OCR_DPI, "-png", "-l", &MAX_OCR_PAGES.to_string()])
            .arg(&request.path)
            .arg(&prefix))?;
        let mut images: Vec<PathBuf> = std::fs::read_dir(work.path())
            .map_err(|e| ProviderError::Failed(e.to_string()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "png"))
            .collect();
        // pdftoppm pads page numbers to the same width, so names sort in page order.
        images.sort();
        if images.is_empty() {
            return Err(ProviderError::Failed(format!(
                "{}: no pages could be rendered",
                request.path.display()
            )));
        }
        let mut pages = Vec::new();
        for image in &images {
            pages.push(self.recognize(tesseract, image)?);
        }
        Ok(Extraction {
            text: pages.join("\n\n"),
            method: "ocr".into(),
            pages: Some(u32::try_from(pages.len()).unwrap_or(u32::MAX)),
            created: None,
            located: false,
        })
    }

    fn recognize(&self, tesseract: &Path, image: &Path) -> Result<String, ProviderError> {
        run(Command::new(tesseract)
            .arg(image)
            .arg("stdout")
            .args(["-l", &self.languages]))
    }
}

impl Default for Extractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs `command`, giving its standard output, or its standard error as the failure.
fn run(command: &mut Command) -> Result<String, ProviderError> {
    let output = command
        .output()
        .map_err(|e| ProviderError::Failed(format!("{}: {e}", command.get_program().display())))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(ProviderError::Failed(format!(
            "{} failed: {}",
            Path::new(command.get_program()).display(),
            message.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Provider for Extractor {
    fn name(&self) -> &'static str {
        "extract"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn extract(
        &self,
        offer: &str,
        request: &ExtractionRequest,
    ) -> Result<Extraction, ProviderError> {
        match offer {
            PDF_TEXT if request.media_type == "application/pdf" => Self::pdf_text(&request.path),
            TESSERACT => self.ocr(request),
            _ => Err(ProviderError::Unsupported(format!(
                "{offer} doesn't read {}",
                request.media_type
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &Path, media_type: &str) -> ExtractionRequest {
        ExtractionRequest {
            path: path.to_owned(),
            media_type: media_type.into(),
        }
    }

    #[test]
    fn without_tesseract_ocr_is_offered_as_unavailable_with_how_to_install() {
        let extractor = Extractor::with(None, None, "eng".into());
        let ocr = &extractor.offers()[1];
        assert_eq!(ocr.id, TESSERACT);
        assert!(!ocr.available);
        assert!(
            ocr.reason
                .as_deref()
                .unwrap()
                .contains("brew install tesseract")
        );
        assert!(!ocr.media_types.contains(&"application/pdf".to_owned()));
        assert!(extractor.offers()[0].available, "PDF text needs nothing");
        let error = extractor
            .extract(TESSERACT, &request(Path::new("/x.png"), "image/png"))
            .unwrap_err();
        assert!(matches!(error, ProviderError::Unavailable(_)));
    }

    #[test]
    fn scanned_pdfs_are_offered_to_ocr_only_with_pdftoppm() {
        let with = Extractor::with(
            Some("tesseract".into()),
            Some("pdftoppm".into()),
            "eng".into(),
        );
        assert!(
            with.offers()[1]
                .media_types
                .contains(&"application/pdf".to_owned())
        );
        assert!(with.offers()[1].available);
    }

    #[test]
    fn a_file_that_isnt_a_pdf_fails_without_stopping_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.pdf");
        std::fs::write(&path, b"%PDF-1.4 not really").unwrap();
        let extractor = Extractor::with(None, None, "eng".into());
        let error = extractor
            .extract(PDF_TEXT, &request(&path, "application/pdf"))
            .unwrap_err();
        assert!(matches!(error, ProviderError::Failed(_)), "{error:?}");
    }

    #[test]
    fn other_types_are_unsupported() {
        let extractor = Extractor::with(None, None, "eng".into());
        let error = extractor
            .extract(PDF_TEXT, &request(Path::new("/a.txt"), "text/plain"))
            .unwrap_err();
        assert!(matches!(error, ProviderError::Unsupported(_)));
    }
}
