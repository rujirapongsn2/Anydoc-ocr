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

## 🔄 Changing OCR Backends

Anydoc ใช้ `OcrEngine` trait ทำให้สลับ backend ได้โดยไม่ต้องแก้ core engine
แค่ implement trait ตัวเดียวแล้วส่งเข้า `to_markdown_with_ocr()`

### หลักการ

```rust
// ทุก backend ต้อง implement trait นี้:
pub trait OcrEngine: Send + Sync {
    fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError>;
}

// สลับ backend แค่เปลี่ยน object ที่ส่งเข้าไป:
let markdown = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr_engine))?;
```

### เปรียบเทียบ Backends ทั้งหมด

| Backend | Type | คุณภาพ | ความเร็ว | ค่าใช้จ่าย | ภาษาไทย |
|---------|------|--------|---------|-----------|---------|
| **Tesseract** | Local | ★★★☆☆ | ★★★☆☆ | ฟรี | ✅ |
| **PaddleOCR** | Local | ★★★★☆ | ★★★★☆ | ฟรี | ✅✅ |
| **Apple Vision** | Local (macOS) | ★★★★★ | ★★★★★ | ฟรี | ✅ |
| **Google Vision** | Cloud API | ★★★★★ | ★★★★☆ | เสียเงิน | ✅✅ |
| **Azure CV** | Cloud API | ★★★★☆ | ★★★★☆ | เสียเงิน | ✅ |
| **AWS Textract** | Cloud API | ★★★★★ | ★★★☆☆ | เสียเงิน | ✅ |
| **Mistral OCR** | Cloud API | ★★★★☆ | ★★★★☆ | เสียเงิน | ✅ |
| **Custom** | กำหนดเอง | — | — | — | — |

---

## 🖥️ Local Backends (รันในเครื่อง)

### 1. Tesseract (ค่าเริ่มต้น — ติดตั้งแล้ว)

```rust
use anydoc::TesseractOcr;

// ค่าเริ่มต้น: ภาษาอังกฤษ
let ocr = TesseractOcr::english();

// ภาษาไทย
let ocr = TesseractOcr::new("tha");

// ไทย + อังกฤษ
let ocr = TesseractOcr::new("tha+eng");

let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

**ติดตั้ง:**
```bash
brew install tesseract tesseract-lang    # macOS
apt install tesseract-ocr tesseract-ocr-tha  # Linux
```

### 2. PaddleOCR (รองรับภาษาเอเชียดีกว่า)

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};
use std::process::Command;

pub struct PaddleOcr {
    pub lang: String,       // "ch", "en", "french", "korean", "japan"
    pub use_gpu: bool,
}

impl OcrEngine for PaddleOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        // เขียน image ลง temp file
        let mut tmp = tempfile::NamedTempFile::new()
            .map_err(|e| OcrError::Backend(format!("temp file failed: {e}")))?;
        use std::io::Write;
        tmp.write_all(image)
            .map_err(|e| OcrError::Backend(format!("write failed: {e}")))?;

        let img_path = tmp.path().to_str()
            .ok_or_else(|| OcrError::Backend("non-UTF8 path".into()))?;

        // เรียก paddleocr CLI
        let mut cmd = Command::new("paddleocr");
        cmd.args(["--image_dir", img_path, "--lang", &self.lang, "--use_gpu", &self.use_gpu.to_string()]);

        let output = cmd.output()
            .map_err(|_e| OcrError::NotConfigured)?;

        if !output.status.success() {
            return Err(OcrError::Backend(format!(
                "paddleocr exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ใช้งาน
let ocr = PaddleOcr { lang: "en".into(), use_gpu: false };
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

**ติดตั้ง:**
```bash
pip install paddlepaddle paddleocr
```

### 3. Apple Vision (macOS เท่านั้น — คุณภาพสูงสุด ไม่ต้องลงอะไร)

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};
use std::process::Command;

pub struct AppleVisionOcr;

impl OcrEngine for AppleVisionOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".png")
            .map_err(|e| OcrError::Backend(format!("temp file failed: {e}")))?;
        use std::io::Write;
        tmp.write_all(image)
            .map_err(|e| OcrError::Backend(format!("write failed: {e}")))?;

        let img_path = tmp.path().to_str()
            .ok_or_else(|| OcrError::Backend("non-UTF8 path".into()))?;

        // เรียกผ่าน Swift CLI หรือ shortcut
        let output = Command::new("shortcuts")
            .args(["run", "OCR-Extract", "-i", img_path])
            .output()
            .map_err(|_e| OcrError::NotConfigured)?;

        if !output.status.success() {
            return Err(OcrError::Backend(format!(
                "Vision failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ใช้งาน
let ocr = AppleVisionOcr;
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

> **หมายเหตุ:** ต้องสร้าง Shortcut "OCR-Extract" ในแอป Shortcuts บน macOS
> หรือใช้ `objc2-Vision` crate เพื่อเรียก Vision framework โดยตรง

---

## ☁️ Cloud API Backends (เรียกผ่านเน็ต)

### วิธีการทำงาน

```
PDF page → render PNG → HTTP POST → Cloud OCR API → text → Markdown
```

Cloud API เหมาะเมื่อ:
- ต้องการคุณภาพสูงสุด (โดยเฉพาะเอกสารที่ซับซ้อน)
- ไม่มีทรัพยากรเครื่องเพียงพอ
- เอกสารภาษาเอเชียที่ local OCR ไม่แม่นยำ

### 1. Google Cloud Vision API

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};
use std::process::Command;

pub struct GoogleVisionOcr {
    pub api_key: String,
    pub language_hints: Vec<String>,  // เช่น ["th", "en"]
}

impl OcrEngine for GoogleVisionOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        use base64::{engine::general_purpose, Engine};

        let b64 = general_purpose::STANDARD.encode(image);
        let lang_json = if self.language_hints.is_empty() {
            String::from("[]")
        } else {
            format!("{:?}", self.language_hints)
                .replace('"', "'")
        };

        // สร้าง request body
        let body = format!(
            r#"{{"requests":[{{"image":{{"content":"{b64}"}},"features":[{{"type":"DOCUMENT_TEXT_DETECTION"}}],"imageContext":{{"languageHints":{lang_json}}}}}]}}"#
        );

        let url = format!(
            "https://vision.googleapis.com/v1/images:annotate?key={}",
            self.api_key
        );

        let output = Command::new("curl")
            .args(["-s", "-X", "POST", &url,
                   "-H", "Content-Type: application/json",
                   "-d", &body])
            .output()
            .map_err(|e| OcrError::Backend(format!("curl failed: {e}")))?;

        let json = String::from_utf8_lossy(&output.stdout).to_string();

        // ดึง fullTextAnnotation จาก JSON response
        // (ใช้ serde_json ในโปรเจกต์จริงแทนการ parse ด้วย string)
        if let Some(start) = json.find("\"text\": \"") {
            let rest = &json[start + 9..];
            if let Some(end) = rest.find("\"") {
                return Ok(rest[..end].to_string());
            }
        }

        Err(OcrError::Backend(format!(
            "Google Vision returned no text: {}",
            json.chars().take(200).collect::<String>()
        )))
    }
}

// ใช้งาน
let ocr = GoogleVisionOcr {
    api_key: std::env::var("GOOGLE_VISION_API_KEY")
        .expect("set GOOGLE_VISION_API_KEY"),
    language_hints: vec!["th".into(), "en".into()],
};
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

**ติดตั้ง:**
```bash
# ตั้งค่า Google Cloud และรับ API key
export GOOGLE_VISION_API_KEY="AIza..."
```

| ข้อดี | ข้อเสีย |
|-------|---------|
| คุณภาพสูงมาก (99%+ accuracy) | เสียเงิน ($1.50 / 1000 images) |
| รองรับ 50+ ภาษา | ต้องเชื่อมต่ออินเทอร์เน็ต |
| รู้จักตารางและฟอร์ม | ข้อมูลส่งไปเซิร์ฟเวอร์ |

### 2. Azure Computer Vision (Read API)

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};
use std::process::Command;

pub struct AzureVisionOcr {
    pub endpoint: String,     // เช่น "https://my-resource.cognitiveservices.azure.com"
    pub api_key: String,
}

impl OcrEngine for AzureVisionOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        // Azure Read API เป็น 2 ขั้นตอน: submit แล้ว get result
        let url = format!(
            "{}/vision/v3.2/read/analyze",
            self.endpoint
        );

        // ขั้นตอน 1: submit image
        let submit = Command::new("curl")
            .args([
                "-s", "-X", "POST", &url,
                "-H", &format!("Ocp-Apim-Subscription-Key: {}", self.api_key),
                "-H", "Content-Type: application/octet-stream",
                "--data-binary", "@-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| OcrError::Backend(format!("curl spawn failed: {e}")))?;

        // ส่ง image bytes ผ่าน stdin
        use std::io::Write;
        if let Some(mut stdin) = submit.stdin.as_ref() {
            stdin.write_all(image)
                .map_err(|e| OcrError::Backend(format!("stdin write: {e}")))?;
        }
        let output = submit.wait_with_output()
            .map_err(|e| OcrError::Backend(format!("curl wait: {e}")))?;

        let response = String::from_utf8_lossy(&output.stdout).to_string();

        // ดึง operation-location URL จาก header (ในโปรเจกต์จริงควรใช้ HTTP client library)
        let operation_url = /* ดึงจาก response headers */ "";

        // ขั้นตอน 2: poll ผลลัพธ์ (รอ 1-3 วินาที)
        std::thread::sleep(std::time::Duration::from_secs(2));

        let result = Command::new("curl")
            .args([
                "-s", "-X", "GET", operation_url,
                "-H", &format!("Ocp-Apim-Subscription-Key: {}", self.api_key),
            ])
            .output()
            .map_err(|e| OcrError::Backend(format!("result fetch: {e}")))?;

        let json = String::from_utf8_lossy(&result.stdout).to_string();

        // ดึง text จาก analyzeResult.readResults
        // (ใช้ serde_json ในโปรเจกต์จริง)
        let mut text = String::new();
        // ... parse JSON ...

        if text.is_empty() {
            Err(OcrError::Backend("Azure returned no text".into()))
        } else {
            Ok(text)
        }
    }
}

// ใช้งาน
let ocr = AzureVisionOcr {
    endpoint: "https://my-resource.cognitiveservices.azure.com".into(),
    api_key: std::env::var("AZURE_VISION_KEY").unwrap(),
};
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

**ติดตั้ง:**
```bash
export AZURE_VISION_KEY="your-key"
export AZURE_VISION_ENDPOINT="https://my-resource.cognitiveservices.azure.com"
```

### 3. AWS Textract

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};

pub struct AwsTextractOcr {
    pub region: String,      // เช่น "ap-southeast-1"
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl OcrEngine for AwsTextractOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        // ใช้ aws-sdk-textract crate ในโปรเจกต์จริง:
        //
        // let config = aws_config::load_defaults(Region::new(self.region.clone())).await;
        // let client = aws_sdk_textract::Client::new(&config);
        //
        // let result = client.detect_document_text()
        //     .document(Document::builder()
        //         .bytes(Blob::new(image))
        //         .build())
        //     .send()
        //     .await
        //     .map_err(|e| OcrError::Backend(e.to_string()))?;
        //
        // let mut text = String::new();
        // for block in result.blocks() {
        //     if block.block_type() == &BlockType::Line {
        //         if let Some(t) = block.text() {
        //             text.push_str(t);
        //             text.push('\n');
        //         }
        //     }
        // }
        //
        // Ok(text)

        // NOTE: ต้องใช้ async runtime (tokio) สำหรับ AWS SDK
        Err(OcrError::Backend("see commented example above — requires async runtime".into()))
    }
}

// ใช้งาน (ต้องใช้ tokio runtime)
// let ocr = AwsTextractOcr {
//     region: "ap-southeast-1".into(),
//     access_key_id: std::env::var("AWS_ACCESS_KEY_ID").unwrap(),
//     secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
// };
```

**ติดตั้ง:**
```toml
# Cargo.toml
[dependencies]
aws-config = "1"
aws-sdk-textract = "1"
tokio = { version = "1", features = ["full"] }
```

### 4. Mistral OCR API

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};
use std::process::Command;

pub struct MistralOcr {
    pub api_key: String,
    pub model: String,    // "mistral-ocr-latest"
}

impl OcrEngine for MistralOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        use base64::{engine::general_purpose, Engine};

        let b64 = general_purpose::STANDARD.encode(image);
        let data_uri = format!("data:image/png;base64,{b64}");

        let body = format!(
            r#"{{"model":"{}","document":{{"type":"image_url","image_url":"{data_uri}"}}}}"#,
            self.model
        );

        let output = Command::new("curl")
            .args([
                "-s", "-X", "POST",
                "https://api.mistral.ai/v1/ocr",
                "-H", "Authorization: Bearer ",
                "-H", &format!("Authorization: Bearer {}", self.api_key),
                "-H", "Content-Type: application/json",
                "-d", &body,
            ])
            .output()
            .map_err(|e| OcrError::Backend(format!("curl failed: {e}")))?;

        let json = String::from_utf8_lossy(&output.stdout).to_string();

        if let Some(start) = json.find("\"markdown\": \"") {
            let rest = &json[start + 13..];
            if let Some(end) = rest.find("\",") {
                return Ok(rest[..end].to_string());
            }
        }

        Err(OcrError::Backend(format!(
            "Mistral OCR returned no text: {}",
            json.chars().take(200).collect::<String>()
        )))
    }
}

// ใช้งาน
let ocr = MistralOcr {
    api_key: std::env::var("MISTRAL_API_KEY").unwrap(),
    model: "mistral-ocr-latest".into(),
};
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

**ติดตั้ง:**
```bash
export MISTRAL_API_KEY="your-key"
```

| ข้อดี | ข้อเสีย |
|-------|---------|
| รักษา Markdown formatting ได้ดี | เสียเงิน |
| คุณภาพสูง | ต้องเชื่อมต่ออินเทอร์เน็ต |
| รองรับหลายภาษา | ข้อมูลส่งไปเซิร์ฟเวอร์ |

### 5. OpenAI (GPT-4o Vision)

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};
use std::process::Command;

pub struct OpenAIVisionOcr {
    pub api_key: String,
    pub model: String,    // "gpt-4o" or "gpt-4o-mini"
}

impl OcrEngine for OpenAIVisionOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        use base64::{engine::general_purpose, Engine};

        let b64 = general_purpose::STANDARD.encode(image);
        let data_uri = format!("data:image/png;base64,{b64}");

        let body = format!(
            r#"{{"model":"{}","messages":[{{"role":"user","content":[{{"type":"text","text":"Extract all text from this image as clean Markdown. Preserve headings, lists, and tables. Output only the text."}},{{"type":"image_url","image_url":{{"url":"{data_uri}"}}}}]]}}],"max_tokens":4096}}"#,
            self.model
        );

        let output = Command::new("curl")
            .args([
                "-s", "-X", "POST",
                "https://api.openai.com/v1/chat/completions",
                "-H", &format!("Authorization: Bearer {}", self.api_key),
                "-H", "Content-Type: application/json",
                "-d", &body,
            ])
            .output()
            .map_err(|e| OcrError::Backend(format!("curl failed: {e}")))?;

        let json = String::from_utf8_lossy(&output.stdout).to_string();

        // ดึง content จาก response
        if let Some(start) = json.find("\"content\": \"") {
            let rest = &json[start + 11..];
            if let Some(end) = rest.find("\",") {
                return Ok(rest[..end].to_string());
            }
        }

        Err(OcrError::Backend(format!(
            "OpenAI returned no text: {}",
            json.chars().take(200).collect::<String>()
        )))
    }
}

// ใช้งาน
let ocr = OpenAIVisionOcr {
    api_key: std::env::var("OPENAI_API_KEY").unwrap(),
    model: "gpt-4o".into(),
};
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

---

## 🧩 Custom Backend (สร้างเอง)

สร้าง backend ของคุณเอง ทำได้โดย implement `OcrEngine` trait:

```rust
use anydoc::formats::pdf::ocr::{OcrEngine, OcrError};

struct MyCustomOcr {
    // กำหนด fields ตามที่ backend ต้องการ
    endpoint: String,
    api_key: Option<String>,
    timeout_seconds: u64,
}

impl OcrEngine for MyCustomOcr {
    fn recognize(&self, image: &[u8], page: usize) -> Result<String, OcrError> {
        // 1. image = PNG bytes ของหน้า PDF ที่ render แล้ว (300 DPI)
        // 2. page = เลขหน้า (1-based)
        // 3. ส่ง image ไปยัง OCR engine ของคุณ (API, binary, library)
        // 4. คืนข้อความที่ extract ได้

        // ตัวอย่าง: เรียก HTTP API ของคุณเอง
        let output = std::process::Command::new("curl")
            .args([
                "-s",
                "-X", "POST",
                &format!("{}/ocr", self.endpoint),
                "--data-binary", "@-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| OcrError::Backend(format!("spawn failed: {e}")))?;

        // ... send image, read result ...

        Ok("extracted text".to_string())
    }
}

// ใช้งาน
let ocr = MyCustomOcr {
    endpoint: "http://localhost:8080".into(),
    api_key: None,
    timeout_seconds: 30,
};
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

### Checklist สำหรับ Custom Backend

- [ ] Implement `OcrEngine` trait (ต้องมี `Send + Sync`)
- [ ] รับ `image: &[u8]` (PNG bytes) แล้วส่งต่อได้
- [ ] คืน `Ok(String)` เมื่อสำเร็จ
- [ ] คืน `Err(OcrError::Backend(msg))` เมื่อเกิดข้อผิดพลาด
- [ ] คืน `Err(OcrError::NotConfigured)` เมื่อ backend ไม่ได้ติดตั้ง
- [ ] ใช้ `tempfile` สำหรับ temp files (อย่าใช้ชื่อไฟล์คาดเดาได้)
- [ ] ไม่ panic! บน error ใดๆ (ใช้ `Result` เสมอ)
- [ ] ทดสอบกับ PDF หลายภาษา

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
