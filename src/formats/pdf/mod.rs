//! PDF via [pdf-inspector]: classification plus direct Markdown extraction.
//!
//! Unlike the other frontends, pdf-inspector emits Markdown itself, so PDFs
//! bypass the document model and the shared GFM writer. Scanned and
//! image-only PDFs need OCR, which anydoc does not do; they error as
//! unsupported. Pages flagged for OCR in an otherwise text-based document
//! degrade with a log, consistent with the crate-wide recovery policy.
//!
//! When an [`OcrEngine`](ocr::OcrEngine) is provided, pages flagged by
//! pdf-inspector as needing OCR are rendered to images and run through the
//! engine. Recognized text is inserted at the page boundary so the output
//! reads as one continuous document.
//!
//! [pdf-inspector]: https://github.com/firecrawl/pdf-inspector

pub mod ocr;

#[cfg(feature = "ocr-tesseract")]
pub mod tesseract;

#[cfg(feature = "ocr-tesseract")]
pub use tesseract::TesseractOcr;

use crate::error::ConvertError;
use pdf_inspector::PdfError;

/// Convert PDF bytes to Markdown (no OCR fallback).
pub fn to_markdown(bytes: &[u8]) -> Result<String, ConvertError> {
    to_markdown_with_ocr(bytes, None)
}

/// Convert PDF bytes to Markdown with optional OCR fallback.
///
/// When `ocr_engine` is `None`, behavior is identical to [`to_markdown`]:
/// pages needing OCR are logged and skipped. When an engine is supplied,
/// those pages are rendered and recognized; recognized text is inserted into
/// the Markdown at the page boundary.
pub fn to_markdown_with_ocr(
    bytes: &[u8],
    ocr_engine: Option<&dyn ocr::OcrEngine>,
) -> Result<String, ConvertError> {
    let result = pdf_inspector::process_pdf_mem(bytes).map_err(map_error)?;

    if !result.pages_needing_ocr.is_empty() {
        log::warn!(
            "{} of {} pages need OCR and were not extracted",
            result.pages_needing_ocr.len(),
            result.page_count
        );
    }
    if result.has_encoding_issues {
        log::warn!("broken font encodings detected; extracted text may be garbled");
    }

    // Fast path: no pages need OCR, or there is no engine anyway.
    let needs_ocr_work = !result.pages_needing_ocr.is_empty() && ocr_engine.is_some();

    if !needs_ocr_work {
        return match result.markdown {
            Some(mut markdown) if !markdown.trim().is_empty() => {
                if !markdown.ends_with('\n') {
                    markdown.push('\n');
                }
                Ok(markdown)
            }
            _ => Err(ConvertError::Unsupported(format!(
                "PDF has no extractable text ({:?}, {} pages): OCR is required",
                result.pdf_type, result.page_count
            ))),
        };
    }

    // Slow path: run OCR on flagged pages.
    let engine = ocr_engine.unwrap();
    let mut markdown = result.markdown.unwrap_or_default();
    let mut recognized_any = false;

    for &page_num in &result.pages_needing_ocr {
        let page_num = page_num as usize;  // pdf-inspector uses u32; cast to usize
        match render_and_recognize(bytes, page_num, engine) {
            Ok(text) if !text.trim().is_empty() => {
                if !markdown.is_empty() && !markdown.ends_with('\n') {
                    markdown.push('\n');
                }
                markdown.push_str(&format!("\n<!-- OCR: page {page_num} -->\n\n"));
                markdown.push_str(&text);
                markdown.push('\n');
                recognized_any = true;
            }
            Ok(_) => log::warn!("page {page_num} OCR returned empty text"),
            Err(ocr::OcrError::NotConfigured) => {
                log::warn!("page {page_num} skipped: OCR engine not configured");
            }
            Err(ocr::OcrError::Backend(msg)) => {
                log::warn!("page {page_num} OCR failed: {msg}");
            }
        }
    }

    if !recognized_any && markdown.trim().is_empty() {
        return Err(ConvertError::Unsupported(format!(
            "PDF has no extractable text ({:?}, {} pages): OCR ran but extracted nothing",
            result.pdf_type, result.page_count
        )));
    }

    if !markdown.ends_with('\n') {
        markdown.push('\n');
    }
    Ok(markdown)
}

/// Render a single page to an image and run it through the OCR engine.
fn render_and_recognize(
    bytes: &[u8],
    page_num: usize,
    engine: &dyn ocr::OcrEngine,
) -> Result<String, ocr::OcrError> {
    let image = ocr::render_page(bytes, page_num)?;
    engine.recognize(&image, page_num)
}

fn map_error(e: PdfError) -> ConvertError {
    match e {
        PdfError::Encrypted => ConvertError::Encrypted,
        PdfError::Io(e) => ConvertError::Io(e),
        PdfError::NotAPdf(detail) => ConvertError::malformed(format!("not a PDF: {detail}")),
        PdfError::InvalidStructure => ConvertError::malformed("invalid PDF structure"),
        PdfError::Parse(detail) => ConvertError::malformed(detail),
    }
}
