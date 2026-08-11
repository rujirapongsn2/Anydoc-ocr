//! Locators: where to find a field's raw text in a [`Layout`].

use crate::{Layout, PageLayout, PositionedText};
use std::cmp::Ordering;

/// Where to find a field's value.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "json", derive(serde::Deserialize), serde(rename_all = "camelCase"))]
pub enum Locator {
    /// A fixed rectangle on a specific page, in the PDF's own point space
    /// (see [`Layout`]'s coordinate-space docs). Matches every item whose
    /// center falls inside the box, joined top-to-bottom then left-to-right.
    /// Use this for forms with a fixed layout (ภ.ง.ด., ภ.พ.30).
    Bbox {
        /// 1-based page number.
        page: u32,
        /// Left edge, in points.
        x: f32,
        /// Bottom edge, in points.
        y: f32,
        /// Box width, in points.
        width: f32,
        /// Box height, in points.
        height: f32,
    },
    /// Text found near a label string, for documents that carry required
    /// data elements without a standard layout (ใบกำกับภาษี). Tries, in
    /// order: the same text item right after the label; another item to its
    /// right on the same line; the nearest line below it.
    Label {
        /// Matched case-insensitively (ASCII only — Thai has no case) as a
        /// substring of a text item.
        text: String,
        /// Restrict the search to one page; `None` searches every page in
        /// order and returns the first match.
        #[cfg_attr(feature = "json", serde(default))]
        page: Option<u32>,
    },
}

pub(super) fn resolve(locator: &Locator, layout: &Layout) -> Option<String> {
    match locator {
        Locator::Bbox { page, x, y, width, height } => {
            resolve_bbox(layout, *page, *x, *y, *width, *height)
        }
        Locator::Label { text, page } => resolve_label(layout, text, *page),
    }
}

fn resolve_bbox(
    layout: &Layout,
    page: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Option<String> {
    let page_layout = layout.pages.iter().find(|p| p.page == page)?;
    let x_max = x + width;
    let y_max = y + height;

    let matches: Vec<&PositionedText> = page_layout
        .items
        .iter()
        .filter(|item| {
            let center_x = item.x + item.width / 2.0;
            let center_y = item.y + item.height / 2.0;
            (x..=x_max).contains(&center_x) && (y..=y_max).contains(&center_y)
        })
        .collect();

    join_reading_order(matches)
}

/// Sorts top-to-bottom (descending y, since PDF y increases upward) then
/// left-to-right, and joins. `None` when nothing matched or everything
/// matched was blank.
fn join_reading_order(mut items: Vec<&PositionedText>) -> Option<String> {
    items.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(Ordering::Equal).then(cmp_x(a, b)));
    join_items(items)
}

fn cmp_x(a: &PositionedText, b: &PositionedText) -> Ordering {
    a.x.partial_cmp(&b.x).unwrap_or(Ordering::Equal)
}

/// How close two runs have to sit, in multiples of the earlier run's height,
/// to read as one uninterrupted word rather than two.
const FLUSH_GAP: f32 = 0.15;

/// Join runs in the order given, with a space between them — except where
/// two runs on one line sit flush against each other.
///
/// A font change splits `1,500.00` into `1,500` and `.00` at no visual gap.
/// Separating those with a space produces `1,500 .00`, which reads as two
/// numbers rather than one, and every downstream numeric parse rejects it.
fn join_items(items: Vec<&PositionedText>) -> Option<String> {
    let mut joined = String::new();
    let mut previous: Option<&PositionedText> = None;
    for item in items {
        let text = item.text.trim();
        if text.is_empty() {
            continue;
        }
        if !joined.is_empty() && !previous.is_some_and(|prev| sits_flush_after(prev, item)) {
            joined.push(' ');
        }
        joined.push_str(text);
        previous = Some(item);
    }
    (!joined.is_empty()).then_some(joined)
}

/// Whether `item` begins exactly where `previous` ends.
///
/// A zero width means the extractor did not measure the run — several PDF
/// producers report none — so its right edge is unknown and no gap can be
/// claimed. Backwards gaps are not flush either: reading order revisits the
/// left margin whenever two columns share a line to within rounding.
fn sits_flush_after(previous: &PositionedText, item: &PositionedText) -> bool {
    if previous.width <= 0.0 || !on_same_line(item, previous) {
        return false;
    }
    // Never splice digits onto digits. Two numeric columns that happen to sit
    // a point apart — a quantity beside its amount — would fuse into one
    // fabricated number (`2` + `50,000.00` reads as `250,000.00`), and no
    // later stage can tell that apart from a real value. A space there is
    // recoverable: the amount parser reports "more than one number".
    let joins_two_numbers = previous.text.trim_end().ends_with(|c: char| c.is_ascii_digit())
        && item.text.trim_start().starts_with(|c: char| c.is_ascii_digit());
    if joins_two_numbers {
        return false;
    }
    let gap = item.x - (previous.x + previous.width);
    (0.0..=previous.height * FLUSH_GAP).contains(&gap)
}

fn resolve_label(layout: &Layout, label: &str, page_filter: Option<u32>) -> Option<String> {
    let needle = label.to_ascii_lowercase();
    layout
        .pages
        .iter()
        .filter(|page| page_filter.is_none_or(|wanted| wanted == page.page))
        .find_map(|page| resolve_label_on_page(page, &needle))
}

fn resolve_label_on_page(page: &PageLayout, needle: &str) -> Option<String> {
    let label_item =
        page.items.iter().find(|item| item.text.to_ascii_lowercase().contains(needle))?;

    remainder_after(&label_item.text, needle)
        .or_else(|| same_line_to_the_right(page, label_item))
        .or_else(|| nearest_line_below(page, label_item))
}

/// The label and its value in one text run (`เลขที่ INV-2569-00042`):
/// whatever follows the matched substring, once the usual label/value
/// separators (`:`, `-`, whitespace) are stripped.
///
/// Relies on `haystack.to_ascii_lowercase()` being a byte-length-preserving
/// transform of `haystack` (ASCII case folding never changes a string's
/// byte length, and leaves multi-byte UTF-8 — Thai text — untouched), so a
/// byte offset found in the lowercased copy is safe to slice out of the
/// original.
fn remainder_after(haystack: &str, needle: &str) -> Option<String> {
    let lowered = haystack.to_ascii_lowercase();
    let start = lowered.find(needle)? + needle.len();
    let remainder = strip_separators(&haystack[start..]);
    (!remainder.is_empty()).then(|| remainder.to_string())
}

/// Drop the punctuation and whitespace a label uses to introduce its value.
///
/// `-` is one of those separators (`Status - Approved`) but is also a minus
/// sign, so it is only dropped when what follows is not a number: stripping
/// it from `ส่วนลด -250.00` would turn a credit into a charge. The dash in
/// front of a number is left for [`super::value::parse`] to read as a sign,
/// which is the reading it gives an attached and a detached dash alike.
fn strip_separators(raw: &str) -> &str {
    let mut rest = raw.trim();
    loop {
        let stripped = match rest.strip_prefix('-') {
            // The whitespace has to be looked past, not stopped at. A font
            // change at the digits puts the sign in its own text run, so the
            // sign arrives detached (`- 250.00`) — which is exactly what
            // `value::parse_amount` reads as negative. Stopping at the space
            // here would strip the sign it goes on to look for.
            Some(after) if !after.trim_start().starts_with(|c: char| c.is_ascii_digit()) => after,
            Some(_) => return rest,
            None => rest.trim_start_matches([':', '.', ' ', '\t']),
        };
        let stripped = stripped.trim_start();
        if stripped == rest {
            return rest;
        }
        rest = stripped;
    }
}

/// How far apart two runs can sit and still belong to the same value,
/// measured in multiples of the label's own height. A font change splits a
/// value into runs that all but touch; a separate column starts after a gap
/// wide enough to read as one.
const RUN_GAP: f32 = 1.5;

/// Take one unbroken run of items along a line, starting from `start`: each
/// item is kept while it sits within `max_gap` of the previous one's right
/// edge. Stops where the next column begins.
fn take_run(mut items: Vec<&PositionedText>, start: f32, max_gap: f32) -> Vec<&PositionedText> {
    items.sort_by(|a, b| cmp_x(a, b));
    let mut cursor = start;
    let mut run: Vec<&PositionedText> = Vec::new();
    for item in items {
        if item.x - cursor > max_gap {
            break;
        }
        cursor = cursor.max(item.x + item.width);
        run.push(item);
    }
    run
}

/// Another item on the same line (label and value split into separate text
/// runs by a font change), positioned to the label's right.
///
/// Stops at the first wide gap. A form line usually carries more than one
/// label/value pair (`เลขที่: 42    วันที่: 10/08/2569`), so taking every
/// item to the right would append the next pair to this one's value.
fn same_line_to_the_right(page: &PageLayout, label: &PositionedText) -> Option<String> {
    let right_edge = label.x + label.width;
    let to_the_right: Vec<&PositionedText> = page
        .items
        .iter()
        // Identity, not position: a label whose width the extractor did not
        // measure has its right edge at its own `x`, so a `>=` comparison
        // admits the label itself and every Label locator answers with its
        // own caption.
        .filter(|item| {
            !std::ptr::eq(*item, label) && item.x >= right_edge && on_same_line(item, label)
        })
        .collect();

    // The run is measured from the label's right edge — unless the extractor
    // never reported a width, in which case that edge is a fiction and every
    // real gap measured against it is wrong. Start at the first item instead:
    // the run still stops at the next column, it just cannot also check that
    // the label and its value sit close together.
    let start = if label.width > 0.0 {
        right_edge
    } else {
        to_the_right.iter().map(|item| item.x).fold(f32::INFINITY, f32::min)
    };

    join_items(take_run(to_the_right, start, label.height * RUN_GAP))
        .and_then(strip_joined_separators)
}

/// The label on its own line, with the value on the line under it (a form
/// caption above its answer, rather than beside it).
///
/// Only the run sitting under the label is taken. The line below spans the
/// full page width, so joining all of it hands a caption its neighbour's
/// answer too, and a totals block — where the amount sits in a right-hand
/// column too far away for [`same_line_to_the_right`] to reach — would
/// answer with the *next* row's number instead of its own.
fn nearest_line_below(page: &PageLayout, label: &PositionedText) -> Option<String> {
    let max_gap = label.height * RUN_GAP;
    let line_y = page
        .items
        .iter()
        .filter(|item| item.y < label.y - label.height / 2.0)
        .map(|item| item.y)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))?;

    let next_line = page
        .items
        .iter()
        .filter(|item| {
            (item.y - line_y).abs() <= label.height / 2.0
                // Reaching the label from the left is not sitting under it. A
                // wide run in a neighbouring column — an address spanning the
                // page — has its right edge past every label on the line
                // above, and would answer all of them.
                && item.x >= label.x - max_gap
                && item.x + item.width >= label.x
        })
        .collect();

    join_items(take_run(next_line, label.x, max_gap)).and_then(strip_joined_separators)
}

/// Strip the label/value separators from a value assembled out of several
/// text runs.
///
/// [`remainder_after`] does this for the single-run case; a value split at a
/// font change carries the same `:` or `-` into its own run (`Status` +
/// `: Approved`), and it has to come off there too or the two paths disagree
/// on what the value is. A run that was nothing but separators leaves
/// nothing behind, and `None` lets the next locator have its turn.
fn strip_joined_separators(joined: String) -> Option<String> {
    let stripped = strip_separators(&joined);
    (!stripped.is_empty()).then(|| stripped.to_string())
}

fn on_same_line(a: &PositionedText, b: &PositionedText) -> bool {
    (a.y - b.y).abs() <= b.height / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str, x: f32, y: f32, width: f32, height: f32) -> PositionedText {
        PositionedText { text: text.to_string(), x, y, width, height }
    }

    fn layout(pages: Vec<(u32, Vec<PositionedText>)>) -> Layout {
        Layout {
            pages: pages.into_iter().map(|(page, items)| PageLayout { page, items }).collect(),
        }
    }

    #[test]
    fn bbox_joins_items_inside_it_top_to_bottom() {
        let page = vec![
            text("Second", 10.0, 90.0, 40.0, 10.0),
            text("First", 10.0, 100.0, 40.0, 10.0),
            text("Outside", 500.0, 500.0, 40.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(
            &Locator::Bbox { page: 1, x: 0.0, y: 80.0, width: 60.0, height: 40.0 },
            &layout,
        );

        assert_eq!(value, Some("First Second".to_string()));
    }

    #[test]
    fn bbox_on_a_page_with_no_matches_is_none() {
        let layout = layout(vec![(1, vec![text("Elsewhere", 500.0, 500.0, 10.0, 10.0)])]);

        let value =
            resolve(&Locator::Bbox { page: 1, x: 0.0, y: 0.0, width: 10.0, height: 10.0 }, &layout);

        assert_eq!(value, None);
    }

    #[test]
    fn bbox_on_a_missing_page_is_none() {
        let layout = layout(vec![(1, vec![text("Only page one", 5.0, 5.0, 10.0, 10.0)])]);

        let value =
            resolve(&Locator::Bbox { page: 2, x: 0.0, y: 0.0, width: 10.0, height: 10.0 }, &layout);

        assert_eq!(value, None);
    }

    #[test]
    fn label_and_value_in_the_same_item_split_on_the_separator() {
        let layout = layout(vec![(1, vec![text("เลขที่ INV-2569-00042", 10.0, 100.0, 100.0, 10.0)])]);

        let value = resolve(&Locator::Label { text: "เลขที่".to_string(), page: None }, &layout);

        assert_eq!(value, Some("INV-2569-00042".to_string()));
    }

    #[test]
    fn label_matching_is_ascii_case_insensitive() {
        let layout = layout(vec![(1, vec![text("Invoice No: 12345", 10.0, 100.0, 100.0, 10.0)])]);

        let value =
            resolve(&Locator::Label { text: "invoice no".to_string(), page: None }, &layout);

        assert_eq!(value, Some("12345".to_string()));
    }

    #[test]
    fn a_value_split_into_its_own_item_on_the_same_line_is_found_to_the_right() {
        let page = vec![
            text("เลขที่:", 10.0, 100.0, 30.0, 10.0),
            text("INV-2569-00042", 50.0, 100.0, 80.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "เลขที่".to_string(), page: None }, &layout);

        assert_eq!(value, Some("INV-2569-00042".to_string()));
    }

    #[test]
    fn a_caption_label_finds_its_value_on_the_line_below() {
        let page = vec![
            text("ที่อยู่", 10.0, 100.0, 30.0, 12.0),
            text("123 ถนนสุขุมวิท", 10.0, 87.0, 90.0, 12.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "ที่อยู่".to_string(), page: None }, &layout);

        assert_eq!(value, Some("123 ถนนสุขุมวิท".to_string()));
    }

    /// `-` introduces a value (`ยอดรวม - 1,500.00`) and also negates one, so
    /// stripping it unconditionally turns a credit note into a charge.
    #[test]
    fn a_negative_amount_keeps_its_sign() {
        let layout = layout(vec![(1, vec![text("ส่วนลด: -250.00", 10.0, 100.0, 100.0, 10.0)])]);

        let value = resolve(&Locator::Label { text: "ส่วนลด".to_string(), page: None }, &layout);

        assert_eq!(value, Some("-250.00".to_string()));
    }

    #[test]
    fn a_dash_separator_before_a_word_is_still_stripped() {
        let layout = layout(vec![(1, vec![text("Status - Approved", 10.0, 100.0, 100.0, 10.0)])]);

        let value = resolve(&Locator::Label { text: "status".to_string(), page: None }, &layout);

        assert_eq!(value, Some("Approved".to_string()));
    }

    /// Form lines carry several label/value pairs side by side, so the value
    /// has to stop at the gap before the next label rather than swallowing it.
    #[test]
    fn a_value_stops_before_the_next_column_on_the_same_line() {
        let page = vec![
            text("เลขที่:", 10.0, 100.0, 30.0, 10.0),
            text("INV-42", 42.0, 100.0, 40.0, 10.0),
            text("วันที่:", 300.0, 100.0, 30.0, 10.0),
            text("10/08/2569", 332.0, 100.0, 60.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "เลขที่".to_string(), page: None }, &layout);

        assert_eq!(value, Some("INV-42".to_string()));
    }

    /// A font change splits one value into runs that all but touch. Those
    /// join without a space, or the amount parser reads `1,500 .00` as two
    /// numbers and rejects the value it was handed.
    #[test]
    fn runs_split_by_a_font_change_join_without_a_space() {
        let page = vec![
            text("Total:", 10.0, 100.0, 30.0, 10.0),
            text("1,500", 42.0, 100.0, 30.0, 10.0),
            text(".00", 73.0, 100.0, 15.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "total".to_string(), page: None }, &layout);

        assert_eq!(value, Some("1,500.00".to_string()));
    }

    /// A totals block sets its amounts in a right-hand column, too far from
    /// the label for the same-line search, so the search falls to the line
    /// below. Taking that whole line hands the grand total the *next* row's
    /// number — a wrong amount that still parses, which is worse than no
    /// amount at all. Stopping at the column boundary leaves a label, which
    /// no numeric parse accepts.
    #[test]
    fn a_label_does_not_answer_with_the_next_rows_amount() {
        let page = vec![
            text("ยอดรวมทั้งสิ้น", 40.0, 100.0, 60.0, 10.0),
            text("1,605.00", 480.0, 100.0, 50.0, 10.0),
            text("ภาษีมูลค่าเพิ่ม", 40.0, 86.0, 60.0, 10.0),
            text("105.00", 490.0, 86.0, 40.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(
            &Locator::Label {
                text: "ยอดรวมทั้งสิ้น".to_string(), page: None
            },
            &layout,
        );

        assert_eq!(value, Some("ภาษีมูลค่าเพิ่ม".to_string()));
    }

    /// The line below spans the page, so a caption must not reach across it
    /// into a neighbouring column's answer.
    #[test]
    fn a_caption_does_not_reach_into_the_next_column_below() {
        let page = vec![
            text("ที่อยู่", 10.0, 100.0, 30.0, 12.0),
            text("โทรศัพท์", 300.0, 100.0, 40.0, 12.0),
            text("123 ถนนสุขุมวิท", 10.0, 87.0, 90.0, 12.0),
            text("02-123-4567", 300.0, 87.0, 60.0, 12.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "ที่อยู่".to_string(), page: None }, &layout);

        assert_eq!(value, Some("123 ถนนสุขุมวิท".to_string()));
    }

    /// Several PDF producers report no width at all. The label's right edge
    /// then sits at its own `x`, and a position-only test for "to the right"
    /// answers every Label locator with its own caption — silently, since a
    /// caption is perfectly good text.
    #[test]
    fn a_label_with_no_measured_width_does_not_answer_with_itself() {
        let page = vec![
            text("เลขที่:", 10.0, 100.0, 0.0, 10.0),
            text("INV-2569-00042", 50.0, 100.0, 80.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "เลขที่".to_string(), page: None }, &layout);

        assert_eq!(value, Some("INV-2569-00042".to_string()));
    }

    /// Two numeric columns can sit a point apart. Joining them flush fuses
    /// them into one number that parses, so nothing downstream can tell it
    /// from a real amount — a space instead leaves two numbers, which the
    /// amount parser rejects outright.
    #[test]
    fn adjacent_numeric_columns_do_not_fuse_into_one_number() {
        let page = vec![
            text("ยอดรวม:", 10.0, 100.0, 30.0, 10.0),
            text("2", 42.0, 100.0, 8.0, 10.0),
            text("50,000.00", 51.0, 100.0, 50.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "ยอดรวม".to_string(), page: None }, &layout);

        assert_eq!(value, Some("2 50,000.00".to_string()));
    }

    /// A font change carries the separator into its own run. It has to come
    /// off there too, or a value found beside its label keeps punctuation
    /// that the same value found *inside* its label's run would not.
    #[test]
    fn a_separator_in_its_own_run_is_stripped_like_one_inside_the_label() {
        let page = vec![
            text("Status", 10.0, 100.0, 30.0, 10.0),
            text(": Approved", 50.0, 100.0, 60.0, 10.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "status".to_string(), page: None }, &layout);

        assert_eq!(value, Some("Approved".to_string()));
    }

    /// The sign has to survive the same whether the font changed at the
    /// digits or not: a discount that reads -250.00 in one document and
    /// +250.00 in the next is worse than either reading alone.
    #[test]
    fn a_detached_sign_reads_the_same_as_an_attached_one() {
        let split = layout(vec![(
            1,
            vec![
                text("ส่วนลด", 10.0, 100.0, 30.0, 10.0),
                text("-", 50.0, 100.0, 5.0, 10.0),
                text("250.00", 60.0, 100.0, 40.0, 10.0),
            ],
        )]);
        let single = layout(vec![(1, vec![text("ส่วนลด -250.00", 10.0, 100.0, 100.0, 10.0)])]);
        let locator = Locator::Label { text: "ส่วนลด".to_string(), page: None };

        assert_eq!(resolve(&locator, &split), Some("- 250.00".to_string()));
        assert_eq!(resolve(&locator, &single), Some("-250.00".to_string()));
    }

    /// A run wide enough to reach the label from a neighbouring column is
    /// not sitting under it. Without a left bound, an address spanning the
    /// page answers every caption on the line above it.
    #[test]
    fn a_caption_does_not_answer_with_a_wide_run_from_the_left() {
        let page = vec![
            text("โทรศัพท์", 300.0, 100.0, 40.0, 12.0),
            text("123 ถนนสุขุมวิท กรุงเทพฯ", 10.0, 87.0, 400.0, 12.0),
        ];
        let layout = layout(vec![(1, page)]);

        let value = resolve(&Locator::Label { text: "โทรศัพท์".to_string(), page: None }, &layout);

        assert_eq!(value, None);
    }

    #[test]
    fn an_unmatched_label_is_none() {
        let layout =
            layout(vec![(1, vec![text("Nothing relevant here", 10.0, 100.0, 100.0, 10.0)])]);

        let value = resolve(&Locator::Label { text: "เลขที่".to_string(), page: None }, &layout);

        assert_eq!(value, None);
    }

    #[test]
    fn a_page_filter_skips_matches_on_other_pages() {
        let layout = layout(vec![
            (1, vec![text("เลขที่ WRONG-PAGE", 10.0, 100.0, 100.0, 10.0)]),
            (2, vec![text("เลขที่ RIGHT-PAGE", 10.0, 100.0, 100.0, 10.0)]),
        ]);

        let value = resolve(&Locator::Label { text: "เลขที่".to_string(), page: Some(2) }, &layout);

        assert_eq!(value, Some("RIGHT-PAGE".to_string()));
    }
}
