//! Field mapping: pulling named values out of a PDF's geometric layout by
//! fixed coordinates or by a nearby label, rather than by parsing prose.
//!
//! v1 covers the two locator strategies: [`Locator::Bbox`] for forms with a
//! fixed layout (ภ.ง.ด., ภ.พ.30) and [`Locator::Label`] for documents that
//! carry required data elements without a standard layout (ใบกำกับภาษี).
//! Both resolve against a [`Layout`](crate::Layout) — the geometric plane
//! from [`crate::pdf_layout`] — because PDF has no document-model form to
//! fall back on ([`crate::to_document`] is unsupported for PDFs).
//!
//! ```rust,ignore
//! use anydoc::fields::{FieldSpec, Locator, ValueKind};
//!
//! let layout = anydoc::pdf_layout(bytes, None)?;
//! let spec = FieldSpec {
//!     name: "invoice_no".into(),
//!     locator: Locator::Label { text: "เลขที่".into(), page: None },
//!     value_kind: ValueKind::Text,
//! };
//! let field = anydoc::fields::resolve(&spec, &layout)?;
//! ```

mod locator;
mod value;

pub use locator::Locator;
pub use value::{Value, ValueKind};

use crate::Layout;
use std::fmt;

/// One field to extract: where to find it, and how to validate what comes
/// back.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "json", derive(serde::Deserialize), serde(rename_all = "camelCase"))]
pub struct FieldSpec {
    /// Caller-chosen name for the extracted value (a form field name, a
    /// column header, ...).
    pub name: String,
    /// Where to look.
    pub locator: Locator,
    /// How to interpret and validate the text found there.
    pub value_kind: ValueKind,
}

/// The result of resolving one [`FieldSpec`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct FieldValue {
    /// The [`FieldSpec::name`] this came from.
    pub name: String,
    /// The text the locator found, before parsing.
    pub raw: String,
    /// The parsed, validated value.
    pub value: Value,
}

/// Why a field could not be resolved.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "json",
    derive(serde::Serialize),
    serde(tag = "kind", rename_all = "camelCase")
)]
pub enum FieldError {
    /// The locator found nothing.
    NotFound,
    /// The locator found text, but [`ValueKind`] parsing rejected it (wrong
    /// shape, bad checksum, an impossible date, ...).
    Invalid {
        /// The text that failed validation.
        raw: String,
        /// Why it was rejected.
        reason: String,
    },
}

impl FieldError {
    fn invalid(raw: &str, reason: &str) -> FieldError {
        FieldError::Invalid { raw: raw.to_string(), reason: reason.to_string() }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldError::NotFound => write!(f, "field not found"),
            FieldError::Invalid { raw, reason } => write!(f, "invalid value {raw:?}: {reason}"),
        }
    }
}

impl std::error::Error for FieldError {}

/// Resolve one field against a PDF's layout.
pub fn resolve(spec: &FieldSpec, layout: &Layout) -> Result<FieldValue, FieldError> {
    let raw = locator::resolve(&spec.locator, layout).ok_or(FieldError::NotFound)?;
    let value = value::parse(spec.value_kind, &raw)?;
    Ok(FieldValue { name: spec.name.clone(), raw, value })
}

/// Resolve every field in `specs` against a PDF's layout, in order, keeping
/// each field's own outcome rather than failing the whole batch on one miss.
pub fn resolve_all(specs: &[FieldSpec], layout: &Layout) -> Vec<Result<FieldValue, FieldError>> {
    specs.iter().map(|spec| resolve(spec, layout)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PageLayout, PositionedText};

    fn layout_with(text: &str) -> Layout {
        Layout {
            pages: vec![PageLayout {
                page: 1,
                items: vec![PositionedText {
                    text: text.to_string(),
                    x: 10.0,
                    y: 100.0,
                    width: 100.0,
                    height: 10.0,
                }],
            }],
        }
    }

    #[test]
    fn a_resolved_field_carries_its_name_raw_text_and_parsed_value() {
        let spec = FieldSpec {
            name: "invoice_no".to_string(),
            locator: Locator::Label { text: "เลขที่".to_string(), page: None },
            value_kind: ValueKind::Text,
        };

        let field = resolve(&spec, &layout_with("เลขที่ INV-2569-00042")).unwrap();

        assert_eq!(field.name, "invoice_no");
        assert_eq!(field.raw, "INV-2569-00042");
        assert_eq!(field.value, Value::Text("INV-2569-00042".to_string()));
    }

    #[test]
    fn an_unresolvable_locator_reports_not_found_rather_than_panicking() {
        let spec = FieldSpec {
            name: "missing".to_string(),
            locator: Locator::Label { text: "ไม่มีจริง".to_string(), page: None },
            value_kind: ValueKind::Text,
        };

        assert_eq!(
            resolve(&spec, &layout_with("unrelated text")).unwrap_err(),
            FieldError::NotFound
        );
    }

    #[test]
    fn resolve_all_keeps_going_after_one_field_fails() {
        let specs = vec![
            FieldSpec {
                name: "found".to_string(),
                locator: Locator::Label { text: "เลขที่".to_string(), page: None },
                value_kind: ValueKind::Text,
            },
            FieldSpec {
                name: "missing".to_string(),
                locator: Locator::Label {
                    text: "ไม่มีจริง".to_string(), page: None
                },
                value_kind: ValueKind::Text,
            },
        ];

        let results = resolve_all(&specs, &layout_with("เลขที่ INV-2569-00042"));

        assert!(results[0].is_ok());
        assert_eq!(results[1], Err(FieldError::NotFound));
    }
}
