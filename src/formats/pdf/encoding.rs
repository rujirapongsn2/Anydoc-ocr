//! Detecting Thai text that a broken font mapping garbled.
//!
//! Some PDF producers embed a subset font whose ToUnicode CMap maps a few
//! glyphs to the wrong code point: a tone mark comes out as an ASCII
//! punctuation character (`เครื่อง` extracts as `เครื:อง`). pdf-inspector's
//! own `has_encoding_issues` does not see this — every glyph did decode to
//! *something*, just the wrong something — so the text layer looks healthy
//! while being silently wrong.
//!
//! The mapping is lost, not recoverable from the extracted text, so the only
//! repair is to OCR the page instead of trusting its text layer.

/// A page needs this many hits before it counts as garbled. The pattern is
/// specific enough that one stray hit is more likely to be unusual typography
/// than a broken font, while a broken font produces them throughout.
const GARBLED_THRESHOLD: usize = 3;

/// Whether `text` shows the tell-tale sign of a broken Thai font mapping.
pub(super) fn thai_text_is_garbled(text: &str) -> bool {
    garbled_hits(text) >= GARBLED_THRESHOLD
}

/// Counts ASCII punctuation sitting between a Thai combining mark and a Thai
/// consonant. Thai typography never places punctuation there — it falls
/// inside a syllable, between a vowel and the consonant that follows it — so
/// a hit is a mis-mapped glyph rather than real punctuation. Anchoring on
/// both sides is what keeps legitimate uses out: `และ/หรือ` and `ม.ค.` have a
/// letter before the punctuation, `ดังนี้:` has whitespace after it.
fn garbled_hits(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(3)
        .filter(|w| {
            is_combining_mark(w[0]) && is_suspect_punctuation(w[1]) && is_thai_consonant(w[2])
        })
        .count()
}

/// `/` is excluded. Thai writes a pair of alternatives with it — `ได้/ไม่ได้`,
/// `ใช่/ไม่ใช่`, `อยู่/ไม่อยู่` — and the tone mark ending the first
/// alternative puts the slash in exactly this window. Three such pairs is an
/// ordinary form, not a broken font.
fn is_suspect_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() && c != '/'
}

/// The consonants, `ก` through `ฮ`.
///
/// A mis-mapped glyph sits *inside* a syllable, so what follows it continues
/// that syllable. A leading vowel (`เ แ โ ใ ไ`) starts a new one, which is
/// what a real separator between two words is doing there.
fn is_thai_consonant(c: char) -> bool {
    ('\u{0E01}'..='\u{0E2E}').contains(&c)
}

/// The marks that attach above or below a consonant: vowels, tone marks, and
/// the cancellation mark. These never end a syllable, which is what makes
/// punctuation right after one suspicious.
fn is_combining_mark(c: char) -> bool {
    matches!(c, '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_tone_mark_between_a_vowel_and_a_consonant_is_garbled() {
        // "เครื่องแม่ข่าย" with the tone mark mapped to ':' and '%'.
        let text = "จัดหาเครื:องแม่ข่าย ระบบเครื:องมือ เชื%อมต่อเครื:อง";

        assert!(thai_text_is_garbled(text));
    }

    #[test]
    fn clean_thai_prose_is_not_garbled() {
        let text = "สัญญาฉบับนี้ทำขึ้นเมื่อวันที่ 10 สิงหาคม 2569 ณ กรุงเทพมหานคร";

        assert!(!thai_text_is_garbled(text));
    }

    /// The punctuation uses that a looser adjacency check would flag: a slash
    /// joining two words, an abbreviation's periods, a colon ending a clause.
    #[test]
    fn legitimate_thai_punctuation_is_not_garbled() {
        let text = "ผู้ขายและ/หรือผู้ซื้อ ตั้งแต่ ม.ค. ถึง ธ.ค. มีรายละเอียดดังนี้: ครั้งที่ 1";

        assert!(!thai_text_is_garbled(text));
    }

    /// The idiom a Thai form is built out of: a yes/no pair, written with a
    /// slash, where the first alternative ends in a tone mark and the second
    /// opens with a leading vowel. Three of them on a page is a checklist,
    /// not a broken font.
    #[test]
    fn thai_alternatives_written_with_a_slash_are_not_garbled() {
        let text = "ได้/ไม่ได้ ใช่/ไม่ใช่ อยู่/ไม่อยู่ ผ่าน/ไม่ผ่าน";

        assert!(!thai_text_is_garbled(text));
    }

    /// One hit is not enough to condemn a page: a broken font mapping repeats.
    #[test]
    fn an_isolated_hit_is_not_enough_to_flag_a_page() {
        assert!(!thai_text_is_garbled("รายการที:หนึ่ง ตามสัญญาฉบับนี้"));
    }

    #[test]
    fn text_with_no_thai_at_all_is_not_garbled() {
        assert!(!thai_text_is_garbled("Purchase Order No: PO-2026-0210 (rev. 2) 50% deposit"));
    }
}
