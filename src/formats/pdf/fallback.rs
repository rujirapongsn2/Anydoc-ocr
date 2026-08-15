//! Fallback OCR chain: try each backend in order, first success wins.
//!
//! Built for the Thai document pipeline: Softnix OCR first (best Thai
//! accuracy, job-based API), Mistral OCR second (good Markdown structure,
//! widely available), Tesseract last (free, local, no network). A backend
//! that is not configured ([`OcrError::NotConfigured`]) or fails
//! ([`OcrError::Backend`]) is skipped and the next one is tried; a backend
//! that succeeds with non-empty text ends the chain.
//!
//! ```rust,ignore
//! use anydoc::FallbackOcr;
//!
//! // Softnix (SOFTNIX_OCR_*) -> Mistral (MISTRAL_API_KEY) -> Tesseract tha+eng
//! let ocr = FallbackOcr::thai_pipeline()?;
//! let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
//! ```

use super::ocr::{OcrEngine, OcrError};

/// Runs engines in order until one returns non-empty text.
///
/// An engine returning `Ok("")` is treated as a failure for this page and
/// the chain continues — an empty page is a skipped page, and anydoc's
/// assembly keeps the text layer in that case anyway.
pub struct FallbackOcr {
    engines: Vec<(String, Box<dyn OcrEngine>)>,
}

impl FallbackOcr {
    /// Chain the given engines, tried in the order supplied.
    pub fn new(engines: Vec<(String, Box<dyn OcrEngine>)>) -> Self {
        Self { engines }
    }

    /// The recommended chain for Thai documents:
    ///
    /// 1. Softnix OCR (`SOFTNIX_OCR_BASE_URL` + `SOFTNIX_OCR_TOKEN`)
    /// 2. Mistral OCR (`MISTRAL_API_KEY`)
    /// 3. Tesseract `tha+eng` (local, always available when installed)
    ///
    /// Backends whose env vars are absent are skipped at build time and
    /// logged, so the chain degrades gracefully on any machine. It is an
    /// error only when nothing at all is available.
    pub fn thai_pipeline() -> Result<Self, OcrError> {
        let mut engines: Vec<(String, Box<dyn OcrEngine>)> = Vec::new();

        #[cfg(feature = "ocr-softnix")]
        {
            match super::softnix::SoftnixOcr::from_env() {
                Ok(engine) => engines.push(("softnix".to_string(), Box::new(engine))),
                Err(OcrError::NotConfigured) => {
                    log::warn!("Softnix OCR not configured (SOFTNIX_OCR_*); skipping");
                }
                Err(e) => return Err(e),
            }
        }

        #[cfg(feature = "ocr-mistral")]
        {
            match super::mistral::MistralOcr::from_env() {
                Ok(engine) => engines.push(("mistral".to_string(), Box::new(engine))),
                Err(OcrError::NotConfigured) => {
                    log::warn!("Mistral OCR not configured (MISTRAL_API_KEY); skipping");
                }
                Err(e) => return Err(e),
            }
        }

        #[cfg(feature = "ocr-tesseract")]
        {
            let engine = super::tesseract::TesseractOcr::new("tha+eng");
            engines.push(("tesseract".to_string(), Box::new(engine)));
        }

        if engines.is_empty() {
            return Err(OcrError::NotConfigured);
        }
        Ok(Self { engines })
    }

    /// Names of the engines in this chain, for logging and diagnostics.
    pub fn engine_names(&self) -> Vec<&str> {
        self.engines.iter().map(|(name, _)| name.as_str()).collect()
    }
}

impl OcrEngine for FallbackOcr {
    fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError> {
        let mut last_error: Option<OcrError> = None;
        for (name, engine) in &self.engines {
            match engine.recognize(image, page) {
                Ok(text) if !text.trim().is_empty() => {
                    log::debug!("page {page}: OCR engine '{name}' succeeded");
                    return Ok(text);
                }
                Ok(_) => {
                    log::warn!("page {page}: OCR engine '{name}' returned empty text; trying next");
                    last_error =
                        Some(OcrError::Backend(format!("engine '{name}' returned empty text")));
                }
                Err(e) => {
                    log::warn!("page {page}: OCR engine '{name}' failed: {e}; trying next");
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or(OcrError::NotConfigured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OkEngine(&'static str);
    impl OcrEngine for OkEngine {
        fn recognize(&self, _image: &[u8], _page: usize) -> Result<String, OcrError> {
            Ok(self.0.to_string())
        }
    }

    struct EmptyEngine;
    impl OcrEngine for EmptyEngine {
        fn recognize(&self, _image: &[u8], _page: usize) -> Result<String, OcrError> {
            Ok(String::new())
        }
    }

    struct FailEngine;
    impl OcrEngine for FailEngine {
        fn recognize(&self, _image: &[u8], _page: usize) -> Result<String, OcrError> {
            Err(OcrError::Backend("boom".into()))
        }
    }

    #[test]
    fn first_success_wins() {
        let chain = FallbackOcr::new(vec![
            ("fail".into(), Box::new(FailEngine)),
            ("ok".into(), Box::new(OkEngine("text"))),
        ]);
        assert_eq!(chain.recognize(b"img", 1).unwrap(), "text");
        assert_eq!(chain.engine_names(), vec!["fail", "ok"]);
    }

    #[test]
    fn empty_text_is_treated_as_failure_and_chain_continues() {
        let chain = FallbackOcr::new(vec![
            ("empty".into(), Box::new(EmptyEngine)),
            ("ok".into(), Box::new(OkEngine("later"))),
        ]);
        assert_eq!(chain.recognize(b"img", 1).unwrap(), "later");
    }

    #[test]
    fn all_failing_reports_the_last_error() {
        let chain = FallbackOcr::new(vec![("fail".into(), Box::new(FailEngine))]);
        let err = chain.recognize(b"img", 1).unwrap_err();
        assert!(matches!(err, OcrError::Backend(msg) if msg.contains("boom")));
    }

    #[test]
    fn an_empty_chain_is_not_configured() {
        let chain = FallbackOcr::new(vec![]);
        assert!(matches!(chain.recognize(b"img", 1), Err(OcrError::NotConfigured)));
    }
}
