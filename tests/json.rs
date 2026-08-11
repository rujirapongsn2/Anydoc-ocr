//! JSON shape of the document model, behind the `json` feature.
//!
//! These assert the wire format rather than that serde works: the tags and
//! key names below are what downstream field mapping reads, so a rename here
//! is a breaking change and should fail loudly.
#![cfg(feature = "json")]

use anydoc::model::{
    Asset, AssetId, Block, Cell, CellSlot, Document, ImageSource, Inline, LinkTarget, List,
    ListItem, MarkerKind, Note, NoteKind, Style, Table, TableKind,
};
use serde_json::json;

fn to_value(document: &Document) -> serde_json::Value {
    serde_json::to_value(document).expect("document serializes")
}

#[test]
fn blocks_and_inlines_are_adjacently_tagged() {
    let document = Document {
        blocks: vec![
            Block::Heading {
                level: 2,
                anchor: Some("intro".into()),
                content: vec![Inline::plain("Title")],
            },
            Block::Paragraph(vec![
                Inline::Text { text: "bold".into(), style: Style { bold: true, ..Style::PLAIN } },
                Inline::LineBreak,
                Inline::Link {
                    content: vec![Inline::plain("here")],
                    target: LinkTarget::External("https://example.com".into()),
                },
            ]),
            Block::Rule,
        ],
        ..Document::default()
    };

    assert_eq!(
        to_value(&document),
        json!({
            "blocks": [
                {
                    "kind": "heading",
                    "value": { "level": 2, "anchor": "intro", "content": [
                        { "kind": "text", "value": { "text": "Title" } }
                    ]}
                },
                {
                    "kind": "paragraph",
                    "value": [
                        { "kind": "text", "value": { "text": "bold", "style": { "bold": true } } },
                        { "kind": "lineBreak" },
                        { "kind": "link", "value": {
                            "content": [{ "kind": "text", "value": { "text": "here" } }],
                            "target": { "kind": "external", "value": "https://example.com" }
                        }}
                    ]
                },
                { "kind": "rule" }
            ]
        })
    );
}

/// Assets can be megabytes; JSON carries provenance and a length so a
/// consumer knows what it is missing without paying for the payload.
#[test]
fn an_asset_serializes_as_metadata_not_bytes() {
    let document = Document {
        blocks: vec![Block::Paragraph(vec![Inline::Image {
            alt: "logo".into(),
            source: ImageSource::Asset(AssetId(0)),
        }])],
        assets: vec![Asset {
            id: AssetId(0),
            media_type: "image/png".into(),
            origin_part: "word/media/image1.png".into(),
            bytes: vec![0u8; 4096],
        }],
        ..Document::default()
    };

    let value = to_value(&document);
    assert_eq!(
        value["assets"],
        json!([{
            "id": 0,
            "mediaType": "image/png",
            "originPart": "word/media/image1.png",
            "byteLength": 4096
        }])
    );
    assert_eq!(
        value["blocks"][0]["value"][0],
        json!({ "kind": "image", "value": {
            "alt": "logo",
            "source": { "kind": "asset", "value": 0 }
        }})
    );
}

#[test]
fn tables_keep_spans_and_covered_positions() {
    let table = Table {
        grid: vec![
            vec![CellSlot::Origin(Cell::spanning(vec![], 1, 2))],
            vec![CellSlot::Covered { origin_row: 0, origin_col: 0 }],
        ],
        header_rows: 1,
        kind: TableKind::Data,
    };

    let value = serde_json::to_value(Block::Table(table)).expect("table serializes");

    assert_eq!(
        value,
        json!({ "kind": "table", "value": {
            "grid": [
                [{ "kind": "origin", "value": { "blocks": [], "colSpan": 1, "rowSpan": 2 } }],
                [{ "kind": "covered", "value": { "originRow": 0, "originCol": 0 } }]
            ],
            "headerRows": 1,
            "kind": "data"
        }})
    );
}

#[test]
fn lists_and_notes_omit_their_absent_fields() {
    let document = Document {
        blocks: vec![Block::List(List {
            marker: MarkerKind::LowerRoman,
            start: 3,
            items: vec![
                ListItem::default(),
                ListItem { checked: Some(true), ..ListItem::default() },
            ],
        })],
        notes: vec![Note { id: "fn1".into(), kind: NoteKind::Footnote, blocks: vec![] }],
        ..Document::default()
    };

    assert_eq!(
        to_value(&document),
        json!({
            "blocks": [{ "kind": "list", "value": {
                "marker": "lowerRoman",
                "start": 3,
                "items": [{ "blocks": [] }, { "blocks": [], "checked": true }]
            }}],
            "notes": [{ "id": "fn1", "kind": "footnote", "blocks": [] }]
        })
    );
}

/// An empty document is the degenerate case every skip attribute funnels
/// into; it should stay a one-key object rather than growing empty arrays.
#[test]
fn an_empty_document_carries_only_its_blocks() {
    assert_eq!(to_value(&Document::default()), json!({ "blocks": [] }));
}
