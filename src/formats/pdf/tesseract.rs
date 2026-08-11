//! Tesseract OCR backend.
//!
//! Shells out to the `tesseract` command-line binary, which must be installed:
//!
//! - **macOS**: `brew install tesseract`
//! - **Linux**: `apt install tesseract-ocr` / `dnf install tesseract`
//! - **Windows**: download from [UB-Mannheim/tesseract](https://github.com/UB-Mannheim/tesseract)
//!
//! ## Example
//!
//! ```rust,ignore
//! use anydoc::TesseractOcr;
//!
//! let ocr = TesseractOcr::new("eng");  // language code
//! let markdown = anydoc::to_markdown_with_ocr(
//!     bytes,
//!     None,
//!     Some(&ocr),
//! )?;
//! ```

use super::ocr::{OcrEngine, OcrError};

/// Tesseract OCR engine wrapper.
///
/// Construct with [`TesseractOcr::new`] passing a Tesseract language code
/// (e.g. `"eng"`, `"tha"`, `"eng+tha"` for multi-language).
pub struct TesseractOcr {
    /// Tesseract language code string (e.g. "eng", "tha", "eng+tha").
    lang: String,
    /// Page segmentation mode, or `None` for Tesseract's own default.
    psm: Option<u8>,
}

impl TesseractOcr {
    /// Create a new Tesseract OCR engine.
    ///
    /// `lang` is a Tesseract language code. Common values:
    /// - `"eng"` — English
    /// - `"tha"` — Thai
    /// - `"eng+tha"` — English + Thai
    /// - `"chi_sim"` — Simplified Chinese
    /// - `"jpn"` — Japanese
    pub fn new(lang: &str) -> Self {
        Self { lang: lang.to_string(), psm: None }
    }

    /// Convenience: English-only engine.
    pub fn english() -> Self {
        Self::new("eng")
    }

    /// Set Tesseract's page segmentation mode (`--psm`).
    ///
    /// The default auto mode can drop trailing lines on single-column scans;
    /// `6` (uniform block of text) is the usual fix for forms and invoices.
    /// Leave unset for multi-column documents.
    pub fn with_psm(mut self, psm: u8) -> Self {
        self.psm = Some(psm);
        self
    }
}

impl OcrEngine for TesseractOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        use std::process::Command;

        // Removed with everything in it on drop, error paths included.
        let dir = super::tempdir::TempDir::new("anydoc-tesseract")
            .map_err(|e| OcrError::Backend(format!("temp dir create failed: {e}")))?;

        let img_path = dir.path().join("page.png");
        std::fs::write(&img_path, image)
            .map_err(|e| OcrError::Backend(format!("temp file write failed: {e}")))?;

        let img_path = img_path
            .to_str()
            .ok_or_else(|| OcrError::Backend("temp image path is not valid UTF-8".into()))?;

        // Invoke: tesseract <image> stdout -l <lang> [--psm <n>]
        let mut command = Command::new("tesseract");
        command.args([img_path, "stdout", "-l", &self.lang]);
        if let Some(psm) = self.psm {
            command.args(["--psm", &psm.to_string()]);
        }
        let output = command.output().map_err(|_e| OcrError::NotConfigured)?;

        // `dir` is dropped at the end of the function, taking the PNG with it.

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OcrError::Backend(format!(
                "tesseract exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(text)
    }
}
