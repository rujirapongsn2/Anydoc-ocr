//! Geometric plane: per-item text positions on native PDF pages, for
//! template-bbox field mapping against a fixed-layout form.
//!
//! pdf-inspector's own Markdown extraction ([`super::to_markdown`]) discards
//! position information once it joins items into lines and paragraphs. This
//! module keeps it, so a consumer that already knows where a value sits on
//! the page (a government form with fixed coordinates) can read it directly
//! instead of parsing prose.
//!
//! ## Coordinate space
//!
//! Positions are in points (1/72 inch), PDF's own space: origin at the
//! page's **bottom-left**, y increasing upward. Anydoc does not flip this to
//! a top-left origin, since doing so needs each page's height and
//! pdf-inspector does not expose that publicly. Keep this in mind when
//! comparing against boxes from another source (an image renderer or OCR
//! engine typically use a top-left, pixel-space origin instead).
//!
//! Pages needing OCR have no native text layer and so contribute nothing
//! here; see [`super::to_markdown_with_ocr`] for recovering their text.

use crate::error::ConvertError;
use pdf_inspector::TextItem;
use pdf_inspector::types::ItemType;
use std::collections::BTreeMap;

/// Native text-item positions, grouped by page.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct Layout {
    /// Pages that produced at least one recognized item, in document order.
    /// A page absent from this list is either blank or needs OCR.
    pub pages: Vec<PageLayout>,
}

/// One page's positioned text items.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "json", derive(serde::Serialize), serde(rename_all = "camelCase"))]
pub struct PageLayout {
    /// 1-based page number.
    pub page: u32,
    /// Items in extraction order (approximately reading order).
    pub items: Vec<PositionedText>,
}

/// One text item and its position on the page. See the [module docs](self)
/// for the coordinate space.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "json", derive(serde::Serialize), serde(rename_all = "camelCase"))]
pub struct PositionedText {
    /// The text content.
    pub text: String,
    /// Left edge, in points from the page's left edge.
    pub x: f32,
    /// Bottom edge, in points from the page's bottom edge.
    pub y: f32,
    /// Width in points.
    pub width: f32,
    /// Height in points, approximated from font size.
    pub height: f32,
}

/// Extract every native text item's position, grouped by page.
///
/// Returns the same items [`super::to_markdown`] would join into prose, just
/// unjoined and with their coordinates kept. Whitespace-only items are
/// dropped since they carry no field value, as are image placeholders, which
/// carry a caption rather than page text.
pub fn extract_layout(bytes: &[u8]) -> Result<Layout, ConvertError> {
    let items = pdf_inspector::extract_text_with_positions_mem(bytes).map_err(super::map_error)?;
    Ok(Layout { pages: group_by_page(items) })
}

/// Group items into one entry per page, in page order.
///
/// Not written as a running "same page as the last item?" check: form-field
/// values arrive after every page's body text, so a filled AcroForm hands us
/// page 1's items again once page 3's are already in. That grouping would
/// emit two entries for page 1, and a `find`-by-page lookup would only ever
/// see the first.
fn group_by_page(items: Vec<TextItem>) -> Vec<PageLayout> {
    let mut by_page: BTreeMap<u32, Vec<PositionedText>> = BTreeMap::new();
    for item in items {
        if item.text.trim().is_empty() || matches!(item.item_type, ItemType::Image) {
            continue;
        }
        by_page.entry(item.page).or_default().push(from_text_item(item));
    }
    by_page.into_iter().map(|(page, items)| PageLayout { page, items }).collect()
}

fn from_text_item(item: TextItem) -> PositionedText {
    PositionedText { text: item.text, x: item.x, y: item.y, width: item.width, height: item.height }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str, page: u32) -> TextItem {
        typed_item(text, page, ItemType::Text)
    }

    fn typed_item(text: &str, page: u32, item_type: ItemType) -> TextItem {
        TextItem {
            text: text.to_string(),
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 12.0,
            font: "Helvetica".to_string(),
            font_size: 12.0,
            page,
            is_bold: false,
            is_italic: false,
            is_underline: false,
            is_strikeout: false,
            item_type,
            mcid: None,
        }
    }

    fn texts(page: &PageLayout) -> Vec<&str> {
        page.items.iter().map(|i| i.text.as_str()).collect()
    }

    #[test]
    fn items_on_the_same_page_share_one_entry() {
        let pages = group_by_page(vec![item("First", 1), item("Second", 1), item("Third", 2)]);

        assert_eq!(pages.len(), 2);
        assert_eq!(texts(&pages[0]), ["First", "Second"]);
        assert_eq!(texts(&pages[1]), ["Third"]);
    }

    /// A filled AcroForm hands back every page's field values after all of
    /// the body text, so page 1 reappears once page 2 is already in. Both
    /// runs have to land in the same entry or a lookup by page finds only
    /// the body text and misses the filled-in values.
    #[test]
    fn a_page_revisited_out_of_order_keeps_one_entry() {
        let pages = group_by_page(vec![
            item("Body page 1", 1),
            item("Body page 2", 2),
            item("Field value page 1", 1),
        ]);

        assert_eq!(pages.len(), 2);
        assert_eq!(texts(&pages[0]), ["Body page 1", "Field value page 1"]);
        assert_eq!(texts(&pages[1]), ["Body page 2"]);
    }

    #[test]
    fn pages_come_back_in_page_order() {
        let pages = group_by_page(vec![item("Third", 3), item("First", 1), item("Second", 2)]);

        assert_eq!(pages.iter().map(|p| p.page).collect::<Vec<_>>(), [1, 2, 3]);
    }

    /// Image placeholders carry a caption, not page text, so a bbox drawn
    /// over a photo would otherwise resolve to `[Image: ...]`.
    #[test]
    fn image_placeholders_are_dropped() {
        let pages = group_by_page(vec![
            typed_item("[Image: logo.png]", 1, ItemType::Image),
            item("Real text", 1),
        ]);

        assert_eq!(texts(&pages[0]), ["Real text"]);
    }

    #[test]
    fn whitespace_only_items_are_dropped() {
        let pages = group_by_page(vec![item("   ", 1), item("Real text", 1)]);

        assert_eq!(texts(&pages[0]), ["Real text"]);
    }

    /// A page whose every item was dropped contributes no entry at all,
    /// matching [`Layout::pages`]'s "blank or needs OCR" documentation.
    #[test]
    fn a_page_with_nothing_left_produces_no_entry() {
        let pages = group_by_page(vec![typed_item("[Image: scan.png]", 1, ItemType::Image)]);

        assert!(pages.is_empty());
    }
}
