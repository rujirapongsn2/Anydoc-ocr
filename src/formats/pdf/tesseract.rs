//! Tesseract OCR backend.
//!
//! Wraps the Tesseract OCR engine via the `tesseract-rs` crate.
//! Requires the Tesseract system binary to be installed:
//!
//! - **macOS**: `brew install tesseract`
//! - **Linux**: `apt install tesseract-ocr` / `dnf install tesseract`
//! - **Windows**: download from [UB-Mannheim/tesseract](https://github.com/UB-Mannheim/tesseract)
//!
//! ## Example
//!
//! ```rust,ignore
//! use anydoc::formats::pdf::ocr::TesseractOcr;
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
        Self {
            lang: lang.to_string(),
        }
    }

    /// Convenience: English-only engine.
    pub fn english() -> Self {
        Self::new("eng")
    }
}

impl OcrEngine for TesseractOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        use std::process::Command;

        // Write the PNG to a unique temp file via tempfile crate.
        // NamedTempFile provides RAII cleanup — the file is removed on drop
        // even if an error occurs, and the name is unpredictable (mitigates
        // symlink attacks and thread collisions).
        let mut tmp_file = tempfile::NamedTempFile::new()
            .map_err(|e| OcrError::Backend(format!("temp file create failed: {e}")))?;

        use std::io::Write;
        tmp_file
            .write_all(image)
            .map_err(|e| OcrError::Backend(format!("temp file write failed: {e}")))?;

        let img_path = tmp_file.path().to_str().ok_or_else(|| {
            OcrError::Backend("temp image path is not valid UTF-8".into())
        })?;

        // Invoke: tesseract <image> stdout -l <lang>
        let output = Command::new("tesseract")
            .args([
                img_path,
                "stdout", // output to stdout
                "-l",
                &self.lang,
            ])
            .output()
            .map_err(|_e| {
                OcrError::NotConfigured
            })?;

        // NamedTempFile is dropped here, cleaning up the temp image.

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
