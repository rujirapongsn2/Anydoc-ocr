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
| **Pluggable** | `OcrEngine` trait lets developers choose any backend (Tesseract locally, or a cloud API like Mistral OCR or Softnix OCR). |
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
| `Cargo.toml` | Added `[features]` section: `ocr-tesseract` |
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
| Scanned PDF + cloud API (Mistral / Softnix) | ~1–10s / page (network + model) | ~5MB | unchanged |

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
     - Render page to PNG at 300 DPI (via `pdftoppm`)
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

> **Backend ที่เขียนเองไม่ต้องเปิด feature flag ใดๆ** — `OcrEngine`, `OcrError`
> และการ render หน้าเป็นภาพ มีอยู่ในทุก build ต้องการเพียง `pdftoppm`
> (poppler-utils) บน `PATH` เท่านั้น feature `ocr-tesseract` มีไว้สำหรับ
> backend Tesseract ที่มาให้ในตัวเท่านั้น

### เปรียบเทียบ Backends ทั้งหมด

| Backend | Type | คุณภาพ | ความเร็ว | ค่าใช้จ่าย | ภาษาไทย |
|---------|------|--------|---------|-----------|---------|
| **Tesseract** | Local | ★★★☆☆ | ★★★☆☆ | ฟรี | ✅ |
| **Mistral OCR** | Cloud API | ★★★★☆ | ★★★★☆ | เสียเงิน | ✅ |
| **Softnix OCR** | Cloud API | ★★★★☆ | ★★★☆☆ (async job) | เสียเงิน | ✅✅ |
| **Custom** | กำหนดเอง | — | — | — | — |

---

## 🖥️ Local Backend (รันในเครื่อง)

### Tesseract (ค่าเริ่มต้น — ติดตั้งแล้ว)

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

### 1. Mistral OCR API

> **ต้องเพิ่ม dependency:** `serde_json = "1"` — parse response ด้วย
> `serde_json::Value` จริง ไม่ใช่ string search ตรงๆ

```rust
use anydoc::{OcrEngine, OcrError};
use serde_json::Value;
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

        // -f/--fail ให้ curl exit ไม่เป็น 0 เมื่อ HTTP ตอบ 4xx/5xx (กัน auth/quota
        // error หลุดผ่านไปเป็น "ไม่เจอ field" เงียบๆ), --max-time กันเคส network ค้าง
        let output = Command::new("curl")
            .args([
                "-s", "-S", "-f", "--max-time", "60", "-X", "POST",
                "https://api.mistral.ai/v1/ocr",
                "-H", &format!("Authorization: Bearer {}", self.api_key),
                "-H", "Content-Type: application/json",
                "-d", &body,
            ])
            .output()
            .map_err(|e| OcrError::Backend(format!("curl failed: {e}")))?;

        if !output.status.success() {
            return Err(OcrError::Backend(format!(
                "curl exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        // parse ด้วย serde_json::Value จริง (ไม่ใช่ string search) เพราะ response
        // อาจเป็น JSON แบบ compact และ markdown อาจมี escaped quote อยู่ข้างใน
        let json: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| OcrError::Backend(format!("Mistral OCR returned invalid JSON: {e}")))?;

        json["pages"][0]["markdown"].as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| OcrError::Backend(format!("Mistral OCR returned no text: {json}")))
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

### 2. Softnix OCR API

[Softnix OCR](https://genai.softnix.ai) เวอร์ชัน V3 เป็น **job-based API** — submit
ไฟล์แล้วได้ `job_id` กลับมาทันที ส่วนงาน OCR/AI จริงทำงานเบื้องหลัง ต้อง poll
`/status` จนกว่าจะ `completed` แล้วค่อยดึง `/result` (หรือใช้ SSE stream /
webhook ก็ได้ แต่ `OcrEngine::recognize()` เป็น synchronous call เดียว จึงต้อง
poll แบบ blocking อยู่ภายในฟังก์ชัน)

> **หมายเหตุ:** V3 มี pipeline "Structured Output" เต็มรูปแบบ (intention
> extraction → schema generation → per-page extraction) ซึ่งเกินความจำเป็นของ
> anydoc ที่ต้องการแค่ข้อความ/Markdown ต่อหน้า ตัวอย่างนี้จึงส่ง
> `disable_structure=true` เพื่อข้าม pipeline นั้นและได้ผลลัพธ์เร็วขึ้น

> **ต้องเพิ่ม dependency:** `serde_json = "1"` — ตัวอย่างนี้ parse response
> ด้วย `serde_json::Value` จริง ไม่ใช่ string search ตรงๆ เพราะ response
> อาจเป็น JSON แบบ compact (ไม่มีเว้นวรรคหลัง `:`) และข้อความ OCR อาจมี
> เครื่องหมายคำพูด (`\"`) อยู่ข้างใน ซึ่ง string search แบบง่ายจะพังทั้งสองกรณี

```rust
use anydoc::{OcrEngine, OcrError};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct SoftnixOcr {
    /// เช่น "https://genai.softnix.ai/multipleocr" หรือ instance ของคุณเอง
    pub base_url: String,
    /// Bearer token จาก `POST /login` (username/password) หรือ token ที่ออกให้ล่วงหน้า
    pub token: String,
    /// ใส่ `true` เฉพาะตอนทดสอบกับ instance ที่ใช้ self-signed TLS cert
    pub insecure_tls: bool,
    pub poll_interval: Duration,
    pub timeout: Duration,
}

impl SoftnixOcr {
    /// เรียก curl แล้ว parse stdout เป็น JSON จริง — เช็ก exit status เสมอ
    /// เพื่อไม่ให้ error การเชื่อมต่อ (DNS/TLS/auth) เงียบหายไปกลายเป็น "ไม่เจอ field"
    /// ทุก call จะจำกัดเวลาด้วย `--max-time` (ตาม self.timeout) เพื่อไม่ให้ curl
    /// เดี่ยวๆ ค้างรอ network ตลอดไป และ `-f` ทำให้ curl exit ไม่เป็น 0 เมื่อ HTTP
    /// ตอบ 4xx/5xx (กัน token หมดอายุ/quota error หลุดผ่านไปเป็น "ไม่เจอ field")
    fn curl(&self, args: &[&str], stdin_data: Option<&[u8]>) -> Result<Value, OcrError> {
        let max_time = self.timeout.as_secs().max(1).to_string();
        let mut full_args = Vec::new();
        if self.insecure_tls {
            full_args.push("-k");
        }
        full_args.push("-s");
        full_args.push("-S"); // -s ปิด progress bar, -S เปิด error message ไว้
        full_args.push("-f"); // exit ไม่เป็น 0 เมื่อ HTTP status เป็น error
        full_args.push("--max-time");
        full_args.push(&max_time);
        full_args.extend_from_slice(args);

        let mut child = Command::new("curl")
            .args(&full_args)
            .stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| OcrError::Backend(format!("curl spawn failed: {e}")))?;

        // เขียน stdin จาก thread แยก แล้วให้ wait_with_output() (thread หลัก) อ่าน
        // stdout/stderr พร้อมกัน — ถ้าเขียน stdin แบบ blocking บน thread หลักก่อน
        // wait_with_output() เพียวๆ จะเกิด deadlock ได้ถ้า curl เขียนลง stdout/
        // stderr (pipe buffer เต็ม) ก่อนที่จะอ่าน stdin ของรูปที่อัปโหลดหมด
        let stdin_writer = stdin_data.map(|data| {
            let mut stdin = child.stdin.take().unwrap();
            let data = data.to_vec();
            std::thread::spawn(move || stdin.write_all(&data))
        });

        let output = child.wait_with_output()
            .map_err(|e| OcrError::Backend(format!("curl wait failed: {e}")))?;

        if let Some(handle) = stdin_writer {
            handle.join()
                .map_err(|_| OcrError::Backend("curl stdin writer thread panicked".into()))?
                .map_err(|e| OcrError::Backend(format!("curl stdin write failed: {e}")))?;
        }

        if !output.status.success() {
            return Err(OcrError::Backend(format!(
                "curl exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        serde_json::from_slice(&output.stdout)
            .map_err(|e| OcrError::Backend(format!("Softnix returned invalid JSON: {e}")))
    }
}

impl OcrEngine for SoftnixOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        let auth = format!("Authorization: Bearer {}", self.token);

        // ขั้นตอน 1: submit job (V3 เป็น async — ตอบกลับ job_id ทันที)
        // ส่งรูปผ่าน stdin ตรงๆ (-F file=@-) ไม่ต้องเขียน temp file และไม่มี
        // path string ให้ curl -F ตีความผิดเวลามีอักขระพิเศษ (เช่น ';')
        let submit = self.curl(&[
            "-X", "POST", &format!("{}/v3/ai-process-file", self.base_url),
            "-H", &auth,
            "-F", "file=@-;filename=page.png;type=image/png",
            "-F", "disable_structure=true",
        ], Some(image))?;

        let job_id = submit["job_id"].as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| OcrError::Backend(format!("Softnix submit returned no job_id: {submit}")))?
            .to_string();

        // ขั้นตอน 2: poll /status จนกว่าจะ completed หรือ failed
        let status_url = format!("{}/v3/ai-process-file/{job_id}/status", self.base_url);
        let started = Instant::now();
        loop {
            if started.elapsed() > self.timeout {
                return Err(OcrError::Backend(format!("Softnix job {job_id} timed out")));
            }
            let status = self.curl(&["-X", "GET", &status_url, "-H", &auth], None)?;
            match status["status"].as_str() {
                Some("completed") => break,
                Some("failed") => return Err(OcrError::Backend(format!(
                    "Softnix job {job_id} failed: {status}"
                ))),
                _ => std::thread::sleep(self.poll_interval),
            }
        }

        // ขั้นตอน 3: ดึงผลลัพธ์ — ใช้ ai_processing.content (Markdown) เฉพาะเมื่อ
        // ai_processing.success == true และไม่ใช่ค่าว่าง ไม่งั้น fallback เป็น
        // ocr_text ดิบ (กรณี VLM step ล้มเหลวแต่ OCR สำเร็จ)
        let result_url = format!("{}/v3/ai-process-file/{job_id}/result", self.base_url);
        let result = self.curl(&["-X", "GET", &result_url, "-H", &auth], None)?;

        // เช็กว่า pages เป็น array จริงและไม่ว่างก่อน index [0] — แยก error
        // "response schema ไม่ตรงที่คาด / ไม่มีหน้าเลย" ออกจาก "มีหน้าแต่ไม่มี
        // ข้อความ" ไม่ให้ทั้งสองเคสไปโผล่ error message เดียวกันจนสับสน
        let pages = result["results"]["pages"].as_array()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| OcrError::Backend(format!("Softnix result had no pages: {result}")))?;
        let page = &pages[0];
        let ai_content = page["ai_processing"]["success"].as_bool().unwrap_or(false)
            .then(|| page["ai_processing"]["content"].as_str())
            .flatten()
            .filter(|s| !s.is_empty());
        let ocr_text = page["ocr_text"].as_str().filter(|s| !s.is_empty());

        ai_content.or(ocr_text)
            .map(str::to_string)
            .ok_or_else(|| OcrError::Backend(format!("Softnix result had no content/ocr_text: {result}")))
    }
}

// ใช้งาน
let ocr = SoftnixOcr {
    base_url: std::env::var("SOFTNIX_BASE_URL").unwrap(),
    token: std::env::var("SOFTNIX_TOKEN").unwrap(),
    insecure_tls: false,
    poll_interval: Duration::from_millis(500),
    timeout: Duration::from_secs(120),
};
let md = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
```

**รับ token (`POST /login`):**
```bash
TOKEN=$(curl -s -X POST "https://genai.softnix.ai/multipleocr/login" \
  -F "username=your-username" \
  -F "password=your-password" | python3 -c "import sys,json; print(json.load(sys.stdin)['access_token'])")
export SOFTNIX_TOKEN="$TOKEN"
```

| ข้อดี | ข้อเสีย |
|-------|---------|
| รองรับภาษาไทยดี รวม structured extraction (schema/JSON) ถ้าต้องการ | เป็น async job — ต้อง poll หรือ webhook เพิ่ม latency ต่อหน้า |
| มี SSE stream และ webhook สำหรับ integration ระดับ backend | เสียเงิน ต้องเชื่อมต่ออินเทอร์เน็ต |
| ปรับ prompt/schema/model เองได้ผ่าน request parameters | ข้อมูลส่งไปเซิร์ฟเวอร์ |

---

## 🧩 Custom Backend (สร้างเอง)

สร้าง backend ของคุณเอง ทำได้โดย implement `OcrEngine` trait:

```rust
use anydoc::{OcrEngine, OcrError};

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
- [ ] temp file ต้องใช้ชื่อที่เดาไม่ได้ และสร้างแบบ fail-if-exists — จะใช้ crate `tempfile` หรือสุ่มชื่อจาก OS entropy เองก็ได้ (anydoc ใช้แบบหลัง ดู `src/formats/pdf/tempdir.rs`)
- [ ] ไม่ panic! บน error ใดๆ (ใช้ `Result` เสมอ)
- [ ] ทดสอบกับ PDF หลายภาษา

---

## 🐍 Python Developer Guide

### ติดตั้ง

```bash
# เฉพาะ conversion (ไม่มี OCR)
pip install firecrawl-anydoc

# รวม Tesseract OCR (ต้องลง Tesseract binary ก่อน)
pip install firecrawl-anydoc pytesseract pillow
brew install tesseract tesseract-lang    # macOS
apt install tesseract-ocr tesseract-ocr-tha  # Linux
```

### การใช้งานพื้นฐาน (ไม่มี OCR)

```python
import anydoc

# แปลงไฟล์เป็น Markdown
markdown = anydoc.to_markdown("report.docx")

# แปลงจาก bytes (format auto-detect)
markdown = anydoc.to_markdown_bytes(data)

# แปลง CSV (ต้องระบุ format)
markdown = anydoc.to_markdown_bytes(data, "csv")

# ดู document model (รวม embedded assets)
document = anydoc.to_document(data, "docx")
for block in document.blocks:
    print(f"{block.kind}: {block}")
for asset in document.assets:
    print(f"Asset: {asset.media_type}, {len(asset.data)} bytes")
```

### การใช้งาน OCR สำหรับ Scanned PDF

เนื่องจาก Python bindings ของ anydoc ยังไม่เปิดเผย `to_markdown_with_ocr()`
โดยตรง นักพัฒนา Python สามารถใช้ OCR ได้ 2 วิธี:

---

### วิธีที่ 1: ใช้ Anydoc CLI ผ่าน subprocess (แนะนำ)

วิธีที่ง่ายที่สุด — เรียก CLI ที่มี OCR feature อยู่แล้ว:

```python
import subprocess
import shutil
from pathlib import Path


def convert_with_ocr(pdf_path: str, ocr_backend: str = "tesseract") -> str:
    """แปลง PDF เป็น Markdown ด้วย OCR fallback

    Args:
        pdf_path: ที่อยู่ไฟล์ PDF
        ocr_backend: "tesseract" | "auto" | "none"

    Returns:
        Markdown string

    Raises:
        FileNotFoundError: anydoc CLI ไม่ได้ติดตั้ง
        RuntimeError: การแปลงล้มเหลว
    """
    # หา anydoc CLI
    anydoc_bin = shutil.which("anydoc")
    if anydoc_bin is None:
        # ลองใช้ npx
        anydoc_bin = "npx"
        prefix = ["npx", "@firecrawl/anydoc"]
    else:
        prefix = [anydoc_bin]

    result = subprocess.run(
        prefix + [pdf_path, "--ocr", ocr_backend],
        capture_output=True,
        text=True,
    )

    if result.returncode != 0:
        raise RuntimeError(
            f"anydoc failed (exit {result.returncode}): {result.stderr.strip()}"
        )

    return result.stdout


# ใช้งาน
markdown = convert_with_ocr("scanned.pdf", ocr_backend="tesseract")
print(markdown)

# หรือแปลงแล้วเซฟลงไฟล์
result = subprocess.run(
    ["anydoc", "scanned.pdf", "--ocr", "tesseract", "-o", "output.md"],
    capture_output=True,
)
```

---

### วิธีที่ 2: ใช้ Python OCR Library โดยตรง (Pure Python)

วิธีนี้ไม่ต้องใช้ CLI ควบคุมได้ทุกขั้นตอนจาก Python:

```python
import anydoc
import pytesseract
from PIL import Image
import io
import tempfile
import subprocess


def pdf_to_markdown_with_ocr(pdf_path: str, lang: str = "eng+tha") -> str:
    """แปลง PDF เป็น Markdown ถ้าเป็น scanned PDF จะใช้ OCR

    Args:
        pdf_path: ที่อยู่ไฟล์ PDF
        lang: รหัสภาษา Tesseract ("eng", "tha", "eng+tha")

    Returns:
        Markdown string
    """
    with open(pdf_path, "rb") as f:
        pdf_bytes = f.read()

    # ลองแปลงด้วย anydoc ก่อน (เร็วมาก)
    try:
        return anydoc.to_markdown(pdf_path)
    except anydoc.UnsupportedError as e:
        if "OCR" not in str(e):
            raise  # ถ้าไม่ใช่เรื่อง OCR ให้ throw ต่อ

    # anydoc บอกว่าต้องใช้ OCR → เรียก OCR เอง
    print(f"PDF needs OCR, running Tesseract (lang={lang})...")

    # แปลง PDF เป็นรูปภาพด้วย pdftoppm
    with tempfile.TemporaryDirectory() as tmpdir:
        # render แต่ละหน้าเป็น PNG ที่ 300 DPI
        subprocess.run(
            ["pdftoppm", "-png", "-r", "300", pdf_path,
             f"{tmpdir}/page"],
            check=True,
            capture_output=True,
        )

        # OCR แต่ละหน้า
        pages = sorted(Path(tmpdir).glob("page-*.png"))
        markdown_parts = []

        for i, page_img in enumerate(pages, 1):
            image = Image.open(page_img)
            text = pytesseract.image_to_string(image, lang=lang)

            if text.strip():
                markdown_parts.append(f"<!-- OCR: page {i} -->\n\n{text}")

    return "\n\n".join(markdown_parts)


# ใช้งาน
markdown = pdf_to_markdown_with_ocr("scanned.pdf", lang="tha+eng")
```

---

### วิธีที่ 3: ใช้ Cloud OCR API จาก Python

```python
import anydoc
import base64
import json
import time
import urllib.request
import os


def pdf_to_markdown_cloud_ocr(pdf_path: str, provider: str = "mistral") -> str:
    """แปลง scanned PDF โดยใช้ Cloud OCR API

    Args:
        pdf_path: ที่อยู่ไฟล์ PDF
        provider: "mistral" | "softnix"

    Returns:
        Markdown string
    """
    with open(pdf_path, "rb") as f:
        pdf_bytes = f.read()

    # ลองแปลงด้วย anydoc ก่อน
    try:
        return anydoc.to_markdown(pdf_path)
    except anydoc.UnsupportedError:
        pass  # ต้องใช้ OCR

    # render PDF เป็นรูปภาพ (ต้องมี pdftoppm หรือ PyMuPDF)
    images = render_pdf_to_images(pdf_bytes)

    # ส่งรูปภาพไปยัง Cloud OCR API
    markdown_parts = []
    for i, img_bytes in enumerate(images, 1):
        text = call_cloud_ocr(img_bytes, provider)
        if text.strip():
            markdown_parts.append(f"<!-- OCR: page {i} -->\n\n{text}")

    return "\n\n".join(markdown_parts)


def call_cloud_ocr(image_bytes: bytes, provider: str) -> str:
    """เรียก Cloud OCR API"""

    if provider == "mistral":
        api_key = os.environ["MISTRAL_API_KEY"]
        b64 = base64.b64encode(image_bytes).decode()
        body = json.dumps({
            "model": "mistral-ocr-latest",
            "document": {
                "type": "image_url",
                "image_url": f"data:image/png;base64,{b64}",
            },
        }).encode()

        req = urllib.request.Request(
            "https://api.mistral.ai/v1/ocr",
            data=body,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            },
        )
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read())
            return data.get("pages", [{}])[0].get("markdown", "")

    elif provider == "softnix":
        return call_softnix_ocr(image_bytes)

    raise ValueError(f"Unknown provider: {provider}")


def call_softnix_ocr(image_bytes: bytes, poll_interval: float = 0.5, timeout: float = 120.0) -> str:
    """เรียก Softnix OCR API (V3) — เป็น async job ต้อง submit แล้ว poll /status

    ต้องตั้ง SOFTNIX_BASE_URL (เช่น "https://genai.softnix.ai/multipleocr")
    และ SOFTNIX_TOKEN (จาก POST /login หรือ token ที่ออกให้ล่วงหน้า)
    """
    import uuid

    base_url = os.environ["SOFTNIX_BASE_URL"]
    token = os.environ["SOFTNIX_TOKEN"]
    headers = {"Authorization": f"Bearer {token}"}

    # multipart/form-data แบบไม่ใช้ dependency เพิ่ม (requests library จะสะดวกกว่านี้)
    boundary = uuid.uuid4().hex
    body = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="file"; filename="page.png"\r\n'
        f"Content-Type: image/png\r\n\r\n"
    ).encode() + image_bytes + (
        f"\r\n--{boundary}\r\n"
        f'Content-Disposition: form-data; name="disable_structure"\r\n\r\ntrue\r\n'
        f"--{boundary}--\r\n"
    ).encode()

    req = urllib.request.Request(
        f"{base_url}/v3/ai-process-file",
        data=body,
        headers={**headers, "Content-Type": f"multipart/form-data; boundary={boundary}"},
        method="POST",
    )
    with urllib.request.urlopen(req) as resp:
        submit = json.loads(resp.read())
    job_id = submit.get("job_id")
    if not job_id:
        raise RuntimeError(f"Softnix submit returned no job_id: {submit}")

    status_req = urllib.request.Request(
        f"{base_url}/v3/ai-process-file/{job_id}/status", headers=headers
    )
    started = time.monotonic()
    while True:
        if time.monotonic() - started > timeout:
            raise TimeoutError(f"Softnix job {job_id} timed out")
        with urllib.request.urlopen(status_req) as resp:
            status_body = json.loads(resp.read())
        status = status_body.get("status")
        if status == "completed":
            break
        if status == "failed":
            raise RuntimeError(f"Softnix job {job_id} failed: {status_body}")
        time.sleep(poll_interval)

    result_req = urllib.request.Request(
        f"{base_url}/v3/ai-process-file/{job_id}/result", headers=headers
    )
    with urllib.request.urlopen(result_req) as resp:
        result = json.loads(resp.read())

    # เช็กว่า pages เป็น list จริงและไม่ว่างก่อน index [0] — กัน IndexError ที่ไม่มี
    # context เวลา response schema ไม่ตรงคาด หรือหน้าอ่านไม่ออก/ไม่มีหน้าเลย
    pages = (result.get("results") or {}).get("pages") or []
    if not pages:
        raise RuntimeError(f"Softnix result had no pages: {result}")
    page = pages[0]

    # ใช้ ai_processing.content เฉพาะเมื่อ success == true และไม่ใช่ค่าว่าง ไม่งั้น
    # fallback เป็น ocr_text ดิบ (กรณี VLM step ล้มเหลวแต่ OCR สำเร็จ) — ให้ตรงกับ
    # logic ของเวอร์ชัน Rust ด้านบน
    ai_processing = page.get("ai_processing") or {}
    content = ai_processing.get("content") if ai_processing.get("success") else None
    ocr_text = page.get("ocr_text")
    text = content or ocr_text
    if not text:
        raise RuntimeError(f"Softnix result had no content/ocr_text: {result}")
    return text


def render_pdf_to_images(pdf_bytes: bytes) -> list[bytes]:
    """แปลง PDF แต่ละหน้าเป็น PNG bytes"""
    import tempfile
    import subprocess
    from pathlib import Path

    images = []
    with tempfile.TemporaryDirectory() as tmpdir:
        pdf_path = Path(tmpdir) / "input.pdf"
        pdf_path.write_bytes(pdf_bytes)

        subprocess.run(
            ["pdftoppm", "-png", "-r", "300", str(pdf_path), f"{tmpdir}/page"],
            check=True,
            capture_output=True,
        )

        for png in sorted(Path(tmpdir).glob("page-*.png")):
            images.append(png.read_bytes())

    return images


# ใช้งาน
markdown = pdf_to_markdown_cloud_ocr("scanned.pdf", provider="softnix")
```

---

### Python Wrapper Class (ครบทุกฟีเจอร์)

โค้ดทั้งหมดรวมในคลาสเดียว พร้อมใช้งาน:

```python
import anydoc
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Optional, Literal


class AnydocConverter:
    """แปลงเอกสารเป็น Markdown รองรับ OCR สำหรับ scanned PDF

    Example:
        converter = AnydocConverter(ocr_backend="tesseract", lang="tha+eng")
        md = converter.convert("document.pdf")
    """

    def __init__(
        self,
        ocr_backend: Literal["tesseract", "auto", "none", "python"] = "none",
        lang: str = "eng",
        cli_path: Optional[str] = None,
    ):
        """
        Args:
            ocr_backend:
                "tesseract" - ใช้ Tesseract ผ่าน CLI
                "auto"      - auto-detect engine
                "none"      - ไม่ใช้ OCR (default)
                "python"    - ใช้ Python OCR library (pytesseract)
            lang: รหัสภาษา ("eng", "tha", "eng+tha")
            cli_path: ที่อยู่ anydoc CLI (None = auto-detect)
        """
        self.ocr_backend = ocr_backend
        self.lang = lang
        self.cli_path = cli_path or shutil.which("anydoc")

    def convert(self, file_path: str | Path) -> str:
        """แปลงเอกสารเป็น Markdown

        Raises:
            FileNotFoundError: ไฟล์หรือ CLI ไม่อยู่
            anydoc.UnsupportedError: แปลงไม่ได้ (เช่น scanned PDF โดยไม่มี OCR)
            RuntimeError: การทำงานล้มเหลว
        """
        file_path = Path(file_path)
        if not file_path.exists():
            raise FileNotFoundError(f"File not found: {file_path}")

        # ไม่ใช่ PDF → แปลงตรงๆ
        if file_path.suffix.lower() != ".pdf":
            return anydoc.to_markdown(str(file_path))

        # PDF: ลองแปลงก่อน ถ้าต้องใช้ OCR ค่อย fallback
        try:
            return anydoc.to_markdown(str(file_path))
        except anydoc.UnsupportedError as e:
            if "OCR" not in str(e) or self.ocr_backend == "none":
                raise

        # ต้องใช้ OCR
        if self.ocr_backend == "python":
            return self._ocr_python(file_path)
        else:
            return self._ocr_cli(file_path)

    def _ocr_cli(self, pdf_path: Path) -> str:
        """ใช้ anydoc CLI กับ --ocr flag"""
        if not self.cli_path:
            raise FileNotFoundError(
                "anydoc CLI not found. Install: npm install -g @firecrawl/anydoc"
            )
        result = subprocess.run(
            [self.cli_path, str(pdf_path), "--ocr", self.ocr_backend],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"anydoc CLI failed: {result.stderr.strip()}"
            )
        return result.stdout

    def _ocr_python(self, pdf_path: Path) -> str:
        """ใช้ pytesseract โดยตรง"""
        import pytesseract
        from PIL import Image

        with tempfile.TemporaryDirectory() as tmpdir:
            subprocess.run(
                ["pdftoppm", "-png", "-r", "300",
                 str(pdf_path), f"{tmpdir}/page"],
                check=True,
                capture_output=True,
            )

            parts = []
            for png in sorted(Path(tmpdir).glob("page-*.png")):
                img = Image.open(png)
                text = pytesseract.image_to_string(img, lang=self.lang)
                if text.strip():
                    page_num = int(png.stem.split("-")[-1])
                    parts.append(f"<!-- OCR: page {page_num} -->\n\n{text}")

        if not parts:
            raise RuntimeError("OCR extracted no text from any page")
        return "\n\n".join(parts)

    def convert_bytes(self, data: bytes, format: Optional[str] = None) -> str:
        """แปลงจาก bytes"""
        return anydoc.to_markdown_bytes(data, format)


# ═══════════════════════════════════════════════════════
# ตัวอย่างการใช้งาน
# ═══════════════════════════════════════════════════════

# 1. แปลงเอกสารทั่วไป (ไม่มี OCR)
converter = AnydocConverter()
md = converter.convert("report.docx")

# 2. แปลง PDF ที่อาจเป็น scanned + OCR ภาษาไทย
converter = AnydocConverter(ocr_backend="python", lang="tha+eng")
md = converter.convert("scanned.pdf")

# 3. ใช้ CLI กับ Tesseract
converter = AnydocConverter(ocr_backend="tesseract")
md = converter.convert("scanned.pdf")

# 4. แปลงหลายไฟล์
converter = AnydocConverter(ocr_backend="python", lang="tha+eng")
for pdf in Path(".").glob("*.pdf"):
    try:
        md = converter.convert(pdf)
        (pdf.with_suffix(".md")).write_text(md)
        print(f"✅ {pdf.name} → {pdf.stem}.md")
    except Exception as e:
        print(f"❌ {pdf.name}: {e}")
```

---

### Python Dependencies Reference

```bash
# พื้นฐาน (แปลงเอกสารทั่วไป)
pip install firecrawl-anydoc

# OCR ผ่าน Python (pytesseract)
pip install firecrawl-anydoc pytesseract pillow

# PDF rendering (ต้องมี pdftoppm)
brew install poppler          # macOS
apt install poppler-utils     # Linux

# หรือใช้ PyMuPDF แทน pdftoppm
pip install PyMuPDF
```

```python
# ทางเลือก: ใช้ PyMuPDF แทน pdftoppm (ไม่ต้องลง poppler)
import fitz  # PyMuPDF

def render_pdf_with_pymupdf(pdf_bytes: bytes) -> list[bytes]:
    """Render PDF pages to PNG using PyMuPDF"""
    doc = fitz.open(stream=pdf_bytes, filetype="pdf")
    images = []
    for page in doc:
        pix = page.get_pixmap(dpi=300)
        images.append(pix.tobytes("png"))
    doc.close()
    return images
```

---

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
