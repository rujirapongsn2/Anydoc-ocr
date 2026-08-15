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
//! engine, as are pages whose text layer decoded through a broken Thai font
//! mapping (see [`encoding`]). Recognized text is inserted at the page
//! boundary so the output reads as one continuous document.
//!
//! [pdf-inspector]: https://github.com/firecrawl/pdf-inspector

mod encoding;
mod tempdir;

pub mod items;
pub mod ocr;

#[cfg(feature = "ocr-tesseract")]
pub mod tesseract;

#[cfg(feature = "ocr-tesseract")]
pub use tesseract::TesseractOcr;

#[cfg(feature = "ocr-mistral")]
pub mod mistral;

#[cfg(feature = "ocr-mistral")]
pub use mistral::MistralOcr;

#[cfg(feature = "ocr-softnix")]
pub mod softnix;

#[cfg(feature = "ocr-softnix")]
pub use softnix::SoftnixOcr;

pub mod fallback;

pub use fallback::FallbackOcr;

use crate::error::ConvertError;
use pdf_inspector::markdown::MarkdownOptions;
use pdf_inspector::types::ItemType;
use pdf_inspector::{PdfError, PdfOptions, PdfType};
use std::collections::{BTreeMap, BTreeSet};

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
///
/// Supplying an engine can only add text, never remove it. Recognized text
/// takes the place of a page's own text layer in one case: the layer was
/// flagged as untrustworthy *and* the recognized text accounts for what it
/// held (see [`covers`]). A backend that returns little or nothing leaves
/// the document as it found it.
pub fn to_markdown_with_ocr(
    bytes: &[u8],
    ocr_engine: Option<&dyn ocr::OcrEngine>,
) -> Result<String, ConvertError> {
    // Page markers are what let recognized text land at the page it came
    // from, out of the same pass that produced the Markdown — no second
    // extraction, so no loss of the table and header/footer handling that
    // only the whole-document pass does. Without an engine there is nothing
    // to place, so they are not emitted and the output is byte-identical to
    // [`to_markdown`].
    let options = PdfOptions::new().markdown(MarkdownOptions {
        include_page_numbers: ocr_engine.is_some(),
        ..MarkdownOptions::default()
    });
    let result = pdf_inspector::process_pdf_mem_with_options(bytes, options).map_err(map_error)?;

    if !result.pages_needing_ocr.is_empty() {
        log::warn!("{} of {} pages need OCR", result.pages_needing_ocr.len(), result.page_count);
    }
    if result.has_encoding_issues {
        log::warn!("broken font encodings detected; extracted text may be garbled");
    }

    let pdf_type = result.pdf_type;
    let page_count = result.page_count;

    // No engine: report a broken font mapping if the whole-document Markdown
    // shows one — the raw recheck below would cost an extraction pass with no
    // way to act on what it found — and hand back what pdf-inspector produced.
    let Some(engine) = ocr_engine else {
        if result.markdown.as_deref().is_some_and(encoding::thai_text_is_garbled) {
            log::warn!("Thai text decoded through a broken font mapping; pages need OCR");
        }
        return finish_markdown(result.markdown, pdf_type, page_count);
    };

    // pdf-inspector reports the pages it knows are unreadable (scanned,
    // image-only, no text layer). It cannot report a broken Thai font
    // mapping, because every glyph did decode to *something* — so add the
    // pages whose raw text shows that signature. Checking the raw items
    // rather than `result.markdown` sees the repeated header/footer lines
    // that `MarkdownOptions::default` strips out, and attributes each hit to
    // the page it came from rather than to the document as a whole.
    let garbled: BTreeSet<u32> = raw_page_text(bytes)
        .into_iter()
        .filter(|(_, text)| encoding::thai_text_is_garbled(text))
        .map(|(page, _)| page)
        .collect();
    if !garbled.is_empty() {
        log::warn!(
            "Thai text decoded through a broken font mapping on {} page(s); they need OCR",
            garbled.len()
        );
    }

    let plan = OcrPlan { flagged: result.pages_needing_ocr.iter().copied().collect(), garbled };
    let markdown = result.markdown.unwrap_or_default();
    assemble_pages(&markdown, page_count, &plan, |page_no| {
        render_and_recognize(bytes, page_no as usize, engine)
    })
    .ok_or_else(|| no_text_error(pdf_type, page_count, !plan.is_empty()))
}

/// pdf-inspector's raw, unstripped text items, joined per 1-based page.
///
/// Empty on extraction failure: `process_pdf_mem` already succeeded, so this
/// is a best-effort second view of the same document, not load-bearing.
///
/// Items are separated by a newline. Running them together instead would
/// manufacture [`encoding`]'s three-character window out of thin air at
/// every run boundary — a label ending in a tone mark followed by a run
/// starting with `(`, `/` or `:` reads as a mis-mapped glyph — and clean
/// Thai pages hit the threshold that way within a few lines.
fn raw_page_text(bytes: &[u8]) -> BTreeMap<u32, String> {
    let Ok(items) = pdf_inspector::extract_text_with_positions_mem(bytes) else {
        return BTreeMap::new();
    };
    let mut pages: BTreeMap<u32, String> = BTreeMap::new();
    for item in items {
        if item.text.trim().is_empty() || matches!(item.item_type, ItemType::Image) {
            continue;
        }
        let page = pages.entry(item.page).or_default();
        if !page.is_empty() {
            page.push('\n');
        }
        page.push_str(&item.text);
    }
    pages
}

/// Which 1-based pages to recognize, and why.
struct OcrPlan {
    /// Pages pdf-inspector reported as unreadable: scanned, image-only, no
    /// text layer.
    flagged: BTreeSet<u32>,
    /// Pages whose Thai text decoded through a broken font mapping. Invisible
    /// to pdf-inspector — every glyph decoded to *something* — so the garbled
    /// text itself is the only signal (see [`encoding`]).
    garbled: BTreeSet<u32>,
}

impl OcrPlan {
    fn needs_ocr(&self, page: u32) -> bool {
        self.flagged.contains(&page) || self.garbled.contains(&page)
    }

    fn is_empty(&self) -> bool {
        self.flagged.is_empty() && self.garbled.is_empty()
    }

    /// What to append to a log line about `page`, naming the broken font
    /// mapping when that is the only reason the page was picked.
    fn reason_note(&self, page: u32) -> &'static str {
        if self.flagged.contains(&page) {
            ""
        } else {
            " (Thai text decoded through a broken font mapping)"
        }
    }
}

fn normalize(markdown: Option<String>) -> Option<String> {
    let mut markdown = markdown?;
    if markdown.trim().is_empty() {
        return None;
    }
    if !markdown.ends_with('\n') {
        markdown.push('\n');
    }
    Some(markdown)
}

fn finish_markdown(
    markdown: Option<String>,
    pdf_type: PdfType,
    page_count: u32,
) -> Result<String, ConvertError> {
    normalize(markdown).ok_or_else(|| no_text_error(pdf_type, page_count, false))
}

/// The 1-based page a `<!-- Page N -->` marker introduces.
fn page_marker(line: &str) -> Option<u32> {
    line.trim().strip_prefix("<!-- Page ")?.strip_suffix("-->")?.trim().parse().ok()
}

/// Cut Markdown carrying `<!-- Page N -->` markers into per-page sections,
/// dropping the markers themselves.
///
/// Anything before the first marker keys to page 0, which no real page can
/// claim, so a producer that puts a preamble there cannot lose it.
fn split_pages(markdown: &str) -> BTreeMap<u32, String> {
    let mut sections: BTreeMap<u32, String> = BTreeMap::new();
    let mut current = 0;

    for line in markdown.lines() {
        if let Some(page) = page_marker(line) {
            current = page;
            sections.entry(page).or_default();
            continue;
        }
        let section = sections.entry(current).or_default();
        section.push_str(line);
        section.push('\n');
    }

    sections
}

/// How much of the text it stands in for a replacement has to account for.
const MIN_REPLACEMENT_RATIO: f64 = 0.5;

/// Whether `replacement` carries enough text to supersede `existing`.
///
/// An engine that comes back with a fraction of what the page held has
/// failed to read it, whatever its exit status said. Dropping the text layer
/// for that turns a 9,000-character page into a 30-character one, so a short
/// result is kept *alongside* the text layer instead of replacing it.
fn covers(replacement: &str, existing: &str) -> bool {
    let existing_len = existing.trim().chars().count();
    existing_len == 0
        || replacement.trim().chars().count() as f64 >= existing_len as f64 * MIN_REPLACEMENT_RATIO
}

/// Walk the document page by page, inserting recognized text at the page it
/// came from. Returns `None` when neither the text layer nor OCR yielded
/// anything.
///
/// `recognize` takes the 1-based page number, keeping this assembly free of
/// page rendering so it can be tested without an OCR backend installed.
fn assemble_pages<F>(
    markdown: &str,
    page_count: u32,
    plan: &OcrPlan,
    mut recognize: F,
) -> Option<String>
where
    F: FnMut(u32) -> Result<String, ocr::OcrError>,
{
    let sections = split_pages(markdown);
    let mut out = String::new();
    let mut any_content = false;

    let mut push = |section: &str| {
        if !section.is_empty() {
            push_section(&mut out, section);
            any_content = true;
        }
    };

    if let Some(preamble) = sections.get(&0) {
        push(preamble.trim());
    }

    // A page with no text layer at all contributes no marker, so iterate the
    // page count rather than the sections: otherwise the scanned pages —
    // exactly the ones OCR exists for — would never come up.
    for page_no in 1..=page_count {
        let own = sections.get(&page_no).map_or("", |section| section.trim());
        let recognized = plan
            .needs_ocr(page_no)
            .then(|| recognize_page(page_no, plan, &mut recognize))
            .flatten();

        if let Some(text) = &recognized {
            push(&format!("<!-- OCR: page {page_no} -->"));
            push(text);
        }
        if !recognized.as_deref().is_some_and(|text| covers(text, own)) {
            push(own);
        }
    }

    if !any_content {
        return None;
    }
    out.push('\n');
    Some(out)
}

/// Recognize one page, logging why nothing came back rather than failing the
/// conversion: the text layer is still there to fall back on.
fn recognize_page<F>(page_no: u32, plan: &OcrPlan, recognize: &mut F) -> Option<String>
where
    F: FnMut(u32) -> Result<String, ocr::OcrError>,
{
    let note = plan.reason_note(page_no);
    match recognize(page_no) {
        Ok(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Ok(_) => {
            log::warn!("page {page_no} OCR returned empty text{note}");
            None
        }
        Err(ocr::OcrError::NotConfigured) => {
            log::warn!("page {page_no} skipped: OCR engine not configured{note}");
            None
        }
        Err(ocr::OcrError::Backend(msg)) => {
            log::warn!("page {page_no} OCR failed: {msg}{note}");
            None
        }
    }
}

fn push_section(markdown: &mut String, section: &str) {
    if !markdown.is_empty() {
        markdown.push_str("\n\n");
    }
    markdown.push_str(section);
}

fn no_text_error(pdf_type: PdfType, page_count: u32, ocr_ran: bool) -> ConvertError {
    let tail = if ocr_ran { "OCR ran but extracted nothing" } else { "OCR is required" };
    ConvertError::Unsupported(format!(
        "PDF has no extractable text ({pdf_type:?}, {page_count} pages): {tail}"
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(flagged: &[u32], garbled: &[u32]) -> OcrPlan {
        OcrPlan {
            flagged: flagged.iter().copied().collect(),
            garbled: garbled.iter().copied().collect(),
        }
    }

    #[test]
    fn recognized_text_lands_between_the_pages_around_it() {
        let markdown =
            "<!-- Page 1 -->\n\nFirst page.\n\n<!-- Page 2 -->\n\n<!-- Page 3 -->\n\nThird page.\n";

        let assembled = assemble_pages(markdown, 3, &plan(&[2], &[]), |page_no| {
            Ok(format!("Scanned page {page_no}."))
        })
        .expect("pages carry content");

        assert_eq!(
            assembled,
            "First page.\n\n<!-- OCR: page 2 -->\n\nScanned page 2.\n\nThird page.\n"
        );
    }

    /// A flagged page still has a text layer, it is just untrustworthy, so
    /// recognized text that accounts for it replaces it rather than joining
    /// it.
    #[test]
    fn recognized_text_supersedes_an_unreliable_text_layer() {
        let markdown = "<!-- Page 1 -->\n\ngarbled \u{fffd}\u{fffd}\n";

        let assembled =
            assemble_pages(markdown, 1, &plan(&[1], &[]), |_| Ok("Legible text.".into())).unwrap();

        assert_eq!(assembled, "<!-- OCR: page 1 -->\n\nLegible text.\n");
    }

    /// The engine reports success but hands back a line where the page held
    /// a paragraph. Letting that replace the text layer is how supplying an
    /// engine ends up deleting most of a document.
    #[test]
    fn a_short_recognition_is_kept_alongside_the_text_layer_not_instead_of_it() {
        let markdown =
            "<!-- Page 1 -->\n\nA full page of text that the engine did not manage to read.\n";

        let assembled =
            assemble_pages(markdown, 1, &plan(&[1], &[]), |_| Ok("noise".into())).unwrap();

        assert_eq!(
            assembled,
            "<!-- OCR: page 1 -->\n\nnoise\n\nA full page of text that the engine did not manage to read.\n"
        );
    }

    /// Without this, supplying an engine would delete text rather than add
    /// it whenever the backend was unreachable.
    #[test]
    fn a_failed_page_keeps_its_text_layer() {
        let markdown = "<!-- Page 1 -->\n\nPartial text.\n";

        let assembled = assemble_pages(markdown, 1, &plan(&[1], &[]), |_| {
            Err(ocr::OcrError::Backend("tesseract died".into()))
        })
        .unwrap();

        assert_eq!(assembled, "Partial text.\n");
    }

    #[test]
    fn empty_recognition_of_an_empty_page_yields_nothing() {
        assert!(assemble_pages("", 1, &plan(&[1], &[]), |_| Ok(String::new())).is_none());
    }

    /// A scanned page has no text lines, so the extraction emits no marker
    /// for it at all. Its recognized text still has to land in page order.
    #[test]
    fn a_page_with_no_marker_still_gets_its_recognized_text_in_order() {
        let markdown = "<!-- Page 1 -->\n\nFirst page.\n\n<!-- Page 3 -->\n\nThird page.\n";

        let assembled =
            assemble_pages(markdown, 3, &plan(&[2], &[]), |_| Ok("Scanned page 2.".into()))
                .unwrap();

        assert_eq!(
            assembled,
            "First page.\n\n<!-- OCR: page 2 -->\n\nScanned page 2.\n\nThird page.\n"
        );
    }

    /// Only flagged pages are rendered: OCR costs hundreds of milliseconds a
    /// page, so touching a page with a good text layer is a real regression.
    #[test]
    fn pages_with_a_good_text_layer_are_never_recognized() {
        let markdown = "<!-- Page 1 -->\n\nText.\n\n<!-- Page 2 -->\n\nMore text.\n";
        let mut recognized = Vec::new();

        let assembled = assemble_pages(markdown, 2, &plan(&[], &[]), |page_no| {
            recognized.push(page_no);
            Ok("should not happen".into())
        })
        .unwrap();

        assert!(recognized.is_empty());
        assert_eq!(assembled, "Text.\n\nMore text.\n");
    }

    /// A scanned page can carry a stray text stamp — a Bates number, a
    /// "CONFIDENTIAL" overlay — so pdf-inspector's document-level page list
    /// is the only signal that the page is an image.
    #[test]
    fn a_page_flagged_only_at_the_document_level_is_still_recognized() {
        let markdown = "<!-- Page 1 -->\n\nPage 1 of 1\n";

        let assembled = assemble_pages(markdown, 1, &plan(&[1], &[]), |_| {
            Ok("Scanned contents, at length.".into())
        })
        .unwrap();

        assert_eq!(assembled, "<!-- OCR: page 1 -->\n\nScanned contents, at length.\n");
    }

    /// Markers are the only thing dropped from a document that needs no OCR:
    /// requesting them must not otherwise change the output.
    #[test]
    fn markers_are_stripped_from_a_document_that_needs_no_ocr() {
        let markdown = "<!-- Page 1 -->\n\n# Title\n\nBody.\n\n<!-- Page 2 -->\n\nMore.\n";

        let assembled = assemble_pages(markdown, 2, &plan(&[], &[]), |_| unreachable!()).unwrap();

        assert_eq!(assembled, "# Title\n\nBody.\n\nMore.\n");
    }

    #[test]
    fn text_before_the_first_marker_is_kept() {
        let markdown = "Preamble.\n\n<!-- Page 1 -->\n\nBody.\n";

        let assembled = assemble_pages(markdown, 1, &plan(&[], &[]), |_| unreachable!()).unwrap();

        assert_eq!(assembled, "Preamble.\n\nBody.\n");
    }

    #[test]
    fn a_garbled_page_is_recognized_and_named_as_such_in_logs() {
        let plan = plan(&[2], &[1]);

        assert!(plan.needs_ocr(1));
        assert_eq!(plan.reason_note(1), " (Thai text decoded through a broken font mapping)");
        assert_eq!(plan.reason_note(2), "");
    }
}
