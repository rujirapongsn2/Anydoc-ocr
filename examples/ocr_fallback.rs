//! OCR fallback chain demo: Softnix OCR -> Mistral OCR -> Tesseract (tha+eng).
//!
//! Build and run against a scanned or broken-font Thai PDF:
//!
//! ```text
//! cargo build --release --example ocr_fallback \
//!   --features "ocr-softnix ocr-mistral ocr-tesseract"
//!
//! SOFTNIX_OCR_BASE_URL=https://genai.softnix.ai/multipleocr \
//! SOFTNIX_OCR_TOKEN=... \
//! MISTRAL_API_KEY=... \
//! ./target/release/examples/ocr_fallback scanned.pdf
//! ```
//!
//! Backends whose credentials are missing are skipped automatically, so the
//! same binary works with any subset configured.

use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: ocr_fallback <scanned-or-garbled.pdf>");
        return ExitCode::from(2);
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return ExitCode::from(1);
        }
    };

    // Simple stderr logger so engine skip/fallback decisions are visible.
    env_logger_like::init();

    let ocr = match anydoc::FallbackOcr::thai_pipeline() {
        Ok(chain) => chain,
        Err(e) => {
            eprintln!("no OCR backend available: {e}");
            return ExitCode::from(1);
        }
    };
    eprintln!("OCR chain: {:?}", ocr.engine_names());

    match anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr)) {
        Ok(markdown) => {
            print!("{markdown}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("conversion failed: {e}");
            ExitCode::from(1)
        }
    }
}

/// Minimal log facade sink printing warn/debug to stderr, so the demo needs
/// no dependency on a logger implementation.
mod env_logger_like {
    pub fn init() {
        // The `log` crate's macros compile to no-ops without a logger set.
        // Point them at stderr with a tiny static logger.
        struct StderrLogger;
        impl log::Log for StderrLogger {
            fn enabled(&self, metadata: &log::Metadata) -> bool {
                metadata.level() <= log::Level::Debug
            }
            fn log(&self, record: &log::Record) {
                if self.enabled(record.metadata()) {
                    eprintln!("[{}] {}", record.level(), record.args());
                }
            }
            fn flush(&self) {}
        }
        static LOGGER: StderrLogger = StderrLogger;
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Debug);
    }
}
