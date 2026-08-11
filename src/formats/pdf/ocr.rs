//! Pluggable OCR backend for scanned / image-only PDF pages.
//!
//! Anydoc's PDF parser ([pdf-inspector]) identifies pages that have no text
//! layer and would produce no output. Rather than failing, this module lets
//! the caller supply an [`OcrEngine`] that renders those pages to images and
//! extracts text from them.
//!
//! ## When OCR runs
//!
//! OCR is **only** invoked for pages pdf-inspector flags as needing it, plus
//! pages whose Thai text decoded through a broken font mapping. Pages with a
//! trustworthy text layer are never touched, so the fast path stays fast.
//! This keeps the zero-overhead promise: if you don't pass an engine, the
//! behavior is identical to anydoc without OCR.
//!
//! ## Rendering
//!
//! [`render_page`] rasterizes a page with `pdftoppm` (poppler-utils) and is
//! available in every build — a custom or cloud backend needs no anydoc
//! feature flag, only `pdftoppm` on `PATH`.
//!
//! ## Implementing a custom backend
//!
//! ```rust,ignore
//! use anydoc::{OcrEngine, OcrError};
//!
//! struct MyOcr;
//!
//! impl OcrEngine for MyOcr {
//!     fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError> {
//!         // image is a PNG byte slice; page is 1-based
//!         todo!("call your OCR library here")
//!     }
//! }
//! ```
//!
//! ## Built-in backends
//!
//! Anydoc provides optional feature-flagged backends so you don't have to
//! roll your own:
//!
//! - **`ocr-tesseract`** — shells out to the [Tesseract] command-line binary
//!   (`brew install tesseract` / `apt install tesseract-ocr`). Low resource,
//!   good quality, 100+ languages.
//!
//! [pdf-inspector]: https://github.com/firecrawl/pdf-inspector
//! [Tesseract]: https://github.com/tesseract-ocr/tesseract

use std::fmt;

/// Error returned by an [`OcrEngine`].
#[derive(Debug)]
pub enum OcrError {
    /// No OCR engine is installed or configured. The page is skipped.
    NotConfigured,
    /// The backend ran but failed on this page. The page is skipped; the
    /// message is logged.
    Backend(String),
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OcrError::NotConfigured => write!(f, "no OCR engine configured"),
            OcrError::Backend(msg) => write!(f, "OCR backend error: {msg}"),
        }
    }
}

impl std::error::Error for OcrError {}

/// Pluggable OCR backend.
///
/// Implement this trait and pass a reference to
/// [`crate::to_markdown_with_ocr`] to handle PDF pages that pdf-inspector
/// flags as having no text layer.
///
/// The `recognize` method receives a **PNG** image byte slice and the 1-based
/// page number. It returns the extracted text, or an [`OcrError`] explaining
/// why the page was skipped.
pub trait OcrEngine: Send + Sync {
    /// Run OCR on a rendered page image (PNG bytes) and return the text.
    fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError>;
}

// ── Page rendering ──────────────────────────────────────────────────────────

/// Render a single PDF page to a PNG image at 300 DPI.
///
/// pdf-inspector extracts text but does not rasterize pages, so this shells
/// out to the `pdftoppm` binary (poppler-utils). `page_num` is 1-based.
///
/// Available in every build, feature flags included: rendering is what feeds
/// an [`OcrEngine`], so gating it would leave a custom or cloud backend with
/// nothing to recognize. [`OcrError::NotConfigured`] when `pdftoppm` is not
/// on `PATH`.
pub fn render_page(pdf_bytes: &[u8], page_num: usize) -> Result<Vec<u8>, OcrError> {
    render_page_via_pdftoppm(pdf_bytes, page_num)
}

/// Render a page by writing a temp PDF into a private temp directory and
/// invoking `pdftoppm -png -r 300`.
fn render_page_via_pdftoppm(pdf_bytes: &[u8], page_num: usize) -> Result<Vec<u8>, OcrError> {
    use std::process::Command;

    // Removed with everything in it on drop, error paths included.
    let dir = super::tempdir::TempDir::new("anydoc-render")
        .map_err(|e| OcrError::Backend(format!("temp dir create failed: {e}")))?;

    // Write the full PDF; pdftoppm can extract single pages via -f / -l.
    let pdf_path = dir.path().join("input.pdf");
    std::fs::write(&pdf_path, pdf_bytes)
        .map_err(|e| OcrError::Backend(format!("temp file write failed: {e}")))?;

    let pdf_str = pdf_path
        .to_str()
        .ok_or_else(|| OcrError::Backend("temp PDF path is not valid UTF-8".into()))?;
    let png_prefix = dir.path().join("page").to_string_lossy().into_owned();

    let output = Command::new("pdftoppm")
        .args([
            "-png",
            "-r",
            "300",
            "-f",
            &page_num.to_string(),
            "-l",
            &page_num.to_string(),
            pdf_str,
            &png_prefix,
        ])
        .output()
        .map_err(|_e| OcrError::NotConfigured)?;

    // Check exit status before searching for output — gives a clearer error.
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OcrError::Backend(format!(
            "pdftoppm exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    // pdftoppm names output files <prefix>-NN.png (zero-padded page number).
    // We scan our private temp dir, so any .png found is ours.
    let entries = std::fs::read_dir(dir.path())
        .map_err(|e| OcrError::Backend(format!("temp dir read failed: {e}")))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("png") {
            let png = std::fs::read(&path)
                .map_err(|e| OcrError::Backend(format!("png read failed: {e}")))?;
            // TempDir drop cleans up everything, including the PDF and PNG.
            return Ok(png);
        }
    }

    Err(OcrError::Backend(format!(
        "pdftoppm succeeded but produced no PNG output (stderr: {})",
        String::from_utf8_lossy(&output.stderr)
    )))
    // `dir` is dropped here, cleaning up all temp files.
}
