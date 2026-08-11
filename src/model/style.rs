/// Fully resolved character style. Tri-state deltas exist only during
/// frontend resolution (`shared::delta`); by the time content reaches the
/// model every toggle has a definite value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct Style {
    /// Bold weight.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "is_false"))]
    pub bold: bool,
    /// Italic or oblique.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "is_false"))]
    pub italic: bool,
    /// Struck through.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "is_false"))]
    pub strike: bool,
    /// Monospace, from a code or teletype character style.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "is_false"))]
    pub code: bool,
}

impl Style {
    /// No toggle set.
    pub const PLAIN: Style = Style { bold: false, italic: false, strike: false, code: false };

    /// True when no toggle is set.
    pub fn is_plain(&self) -> bool {
        *self == Style::PLAIN
    }
}

#[cfg(feature = "json")]
fn is_false(b: &bool) -> bool {
    !*b
}
