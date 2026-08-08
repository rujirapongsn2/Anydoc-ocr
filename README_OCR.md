# Anydoc OCR Fallback — Support Plan for Scanned PDFs

> Add OCR capability to anydoc so scanned/image-only PDFs (no text layer) can be converted to Markdown — without sacrificing speed, keeping installation simple, and maintaining low resource usage.

---

## 📌 Why This Matters

Anydoc converts documents to Markdown at blazing speed (single-digit milliseconds) using a Rust engine. However, **scanned PDFs** — documents captured as images rather than text — have no extractable text layer. The current behavior is:

```
anydoc scanned.pdf
# → Error: "PDF has no extractable text: OCR is required"
```

This plan adds an **opt-in OCR fallback layer** so those pages get recognized and converted.

---

## 🏗️ Architecture: Pluggable OCR Engine

### Design Principles

| Principle | How |
|-----------|-----|
| **Zero overhead** | OCR engine only runs on pages flagged by pdf-inspector as needing it. Text-based PDFs never touch OCR. |
| **Opt-in** | OCR is behind a Cargo feature flag. Users who don't need it see no change — same binary size, same speed. |
| **Pluggable** | `OcrEngine` trait lets developers choose any backend (Tesseract, PaddleOCR, Apple Vision, cloud API). |
| **Easy install** | `pip install firecrawl-anydoc[ocr]` or `cargo build --features ocr-tesseract`. |

### Data Flow

```
PDF bytes
   │
   ▼
pdf-inspector ────► identifies pages_needing_ocr
   │
   ├─ has text layer? ──► ✅ Markdown (fast path, ~5ms)
   │
   └─ pages need OCR? ──► render page → PNG (300 DPI)
                              │
                              ▼
                        OcrEngine.recognize()
                              │
                              ▼
                        text inserted into Markdown
                              at page boundary
```

---

## 📦 What Was Implemented

### New Files

| File | Purpose |
|------|---------|
| `src/formats/pdf/mod.rs` | PDF module entry point; routes to OCR fallback when engine is provided |
| `src/formats/pdf/ocr.rs` | `OcrEngine` trait, `OcrError` type, page rendering helpers |
| `src/formats/pdf/tesseract.rs` | Built-in Tesseract backend (feature-gated) |
| `README_OCR.md` | This document |

### Modified Files

| File | Changes |
|------|---------|
| `src/lib.rs` | Re-exports `OcrEngine`, `OcrError`, `TesseractOcr`; adds `to_markdown_with_ocr()` |
| `src/formats/mod.rs` | `pdf` module is now `pub` so OCR types are reachable |
| `Cargo.toml` | Added `[features]` section: `ocr-tesseract`, `ocr-paddle`, `pdf-render` |
| `node/cli.js` | Added `--ocr <backend>` CLI flag with validation |

### Deleted Files

| File | Replaced By |
|------|-------------|
| `src/formats/pdf.rs` (flat file) | `src/formats/pdf/mod.rs` (module directory) |

---

## 🔌 API Reference

### Rust

#### `to_markdown_with_ocr(bytes, format, ocr_engine)`

```rust
use anydoc::{self, TesseractOcr};

let bytes = std::fs::read("scanned.pdf")?;
let ocr = TesseractOcr::english();    // or TesseractOcr::new("tha")
let markdown = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `bytes` | `&[u8]` | Document file bytes |
| `format` | `Option<Format>` | Format hint; `None` = auto-detect |
| `ocr_engine` | `Option<&dyn OcrEngine>` | OCR backend; `None` = no OCR (original behavior) |

#### `OcrEngine` trait

```rust
pub trait OcrEngine: Send + Sync {
    fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError>;
}
```

Implement this to use any OCR library as a backend.

### CLI

```bash
# No OCR (original behavior — fast)
anydoc document.pdf

# With Tesseract OCR fallback
anydoc scanned.pdf --ocr tesseract

# Auto-detect available engine
anydoc scanned.pdf --ocr auto

# Explicitly disable OCR
anydoc document.pdf --ocr none
```

---

## 🚀 Installation

### Prerequisites

Install the Tesseract system binary (one-time setup):

| Platform | Command |
|----------|---------|
| **macOS** | `brew install tesseract` |
| **Ubuntu/Debian** | `apt install tesseract-ocr` |
| **Fedora/RHEL** | `dnf install tesseract` |
| **Windows** | Download from [UB-Mannheim/tesseract](https://github.com/UB-Mannheim/tesseract/wiki) |

Optional: install additional language data:

```bash
# macOS — Thai + English
brew install tesseract tesseract-lang

# Linux — Thai
apt install tesseract-ocr-tha
```

### Build with OCR Support

```bash
# From source (Rust)
cargo build --release --features ocr-tesseract

# Python (future)
pip install firecrawl-anydoc[ocr]

# Node.js (future)
npm install @firecrawl/anydoc --build-from-source --features ocr-tesseract
```

---

## ⚡ Performance Characteristics

| Scenario | Speed | Memory | Binary Size |
|----------|-------|--------|-------------|
| Text PDF (no OCR) | ~5ms | ~5MB | unchanged |
| Scanned PDF + Tesseract | ~200–500ms / page | ~150MB | +2MB binding |
| Scanned PDF + PaddleOCR (future) | ~100–300ms / page | ~300MB | +5MB + model |
| Scanned PDF + Apple Vision (future) | ~50–100ms / page | ~100MB | +1MB (macOS only) |

> **Key guarantee**: Text PDFs never invoke OCR. The fast path check is `pages_needing_ocr.is_empty()` — a single Vec length check before any rendering.

---

## 🔧 How OCR Fallback Works (Step by Step)

1. **pdf-inspector** processes the PDF and returns:
   - `markdown`: extracted text from pages with a text layer
   - `pages_needing_ocr`: list of page numbers with no text
   - `pdf_type`: classification (text, scanned, mixed)

2. **Fast path** (no OCR engine or no pages need OCR):
   - Return extracted Markdown directly, or
   - Return `Unsupported` error if nothing was extracted

3. **Slow path** (OCR engine provided + pages need OCR):
   - For each flagged page:
     - Render page to PNG at 300 DPI (via `pdftoppm` or native renderer)
     - Pass PNG bytes to `OcrEngine::recognize()`
     - Insert recognized text at the page boundary in Markdown
   - If all pages fail OCR, return `Unsupported` error

4. **Error handling**:
   - `OcrError::NotConfigured` — engine exists but backend missing → page skipped, warning logged
   - `OcrError::Backend(msg)` — engine ran but failed → page skipped, warning logged
   - Failed pages never abort the whole conversion

---

## 🧪 Testing

### Unit Tests

```bash
# Run existing tests (no OCR)
cargo test

# Run with OCR feature enabled
cargo test --features ocr-tesseract
```

### Manual Test

```bash
# Test with a text-based PDF (should work without OCR)
anydoc text_document.pdf --ocr tesseract
# → Fast path: no OCR needed, instant output

# Test with a scanned PDF (should trigger OCR)
anydoc scanned_document.pdf --ocr tesseract
# → OCR pages rendered and recognized

# Test error handling (no OCR engine)
anydoc scanned_document.pdf
# → Error: "OCR is required"
```

---

## 🔮 Roadmap: Future Backends

| Backend | Status | Best For |
|---------|--------|----------|
| **Tesseract** | ✅ Implemented | General use, low resource, 100+ languages |
| **PaddleOCR** | 🔜 Planned | Asian languages, higher accuracy |
| **Apple Vision** | 🔜 Planned | macOS only, best quality, no install needed |
| **Google Vision API** | 🔜 Planned | Cloud-based, highest accuracy, requires API key |
| **Azure Computer Vision** | 🔜 Planned | Cloud-based alternative |

### Implementing a Custom Backend

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};

struct MyCloudOcr {
    api_key: String,
}

impl OcrEngine for MyCloudOcr {
    fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError> {
        // Send image to your OCR API
        // Return extracted text or OcrError::Backend(msg)
        todo!()
    }
}

// Usage:
let ocr = MyCloudOcr { api_key: "sk-...".into() };
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

---

## 📋 Decision Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| OCR as feature flag | `ocr-tesseract` | Keeps default build lean, zero overhead |
| Trait-based backend | `OcrEngine` trait | Developer choice, not locked to one engine |
| Default backend | Tesseract | Free, easy install, good enough quality, 100+ languages |
| Page rendering | `pdftoppm` fallback | Works everywhere without extra Rust deps |
| DPI | 300 | Balance between quality and speed/memory |
| Error policy | Skip + warn, don't abort | Partial output is better than no output |

---

## 📄 License

Same as anydoc: MIT
