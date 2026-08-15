//! Mistral OCR cloud backend (feature `ocr-mistral`).
//!
//! Posts the rendered page PNG to <https://api.mistral.ai/v1/ocr> as a
//! base64 data URI and returns the Markdown of the first page. The API key
//! comes from `MISTRAL_API_KEY` (see [`MistralOcr::from_env`]).
//!
//! HTTP goes through the `curl` binary, matching the crate's zero-extra-deps
//! philosophy: no async runtime, no TLS stack, no HTTP client added to the
//! dependency tree. The response is parsed with `serde_json` because string
//! searching a JSON document with escaped quotes inside the markdown is not
//! reliable.

use super::ocr::{OcrEngine, OcrError};

/// Mistral OCR engine wrapper.
///
/// ```rust,ignore
/// use anydoc::MistralOcr;
///
/// let ocr = MistralOcr::from_env()?;          // reads MISTRAL_API_KEY
/// let markdown = anydoc::to_markdown_with_ocr(&bytes, None, Some(&ocr))?;
/// ```
pub struct MistralOcr {
    /// Bearer token for api.mistral.ai.
    pub api_key: String,
    /// Model id; `mistral-ocr-latest` unless overridden.
    pub model: String,
    /// Per-request timeout in seconds (also the curl `--max-time`).
    pub timeout_secs: u64,
}

impl MistralOcr {
    /// Engine using an explicit API key and the default model.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { api_key: api_key.into(), model: "mistral-ocr-latest".to_string(), timeout_secs: 60 }
    }

    /// Engine reading `MISTRAL_API_KEY` from the environment.
    ///
    /// [`OcrError::NotConfigured`] when the variable is absent, so a
    /// [`FallbackOcr`](super::fallback::FallbackOcr) chain can skip this
    /// backend and try the next one.
    pub fn from_env() -> Result<Self, OcrError> {
        let api_key = std::env::var("MISTRAL_API_KEY").ok();
        Self::from_optional_key(api_key)
    }

    /// Build from an optional key; `None` is [`OcrError::NotConfigured`].
    fn from_optional_key(api_key: Option<String>) -> Result<Self, OcrError> {
        match api_key {
            Some(key) if !key.trim().is_empty() => Ok(Self::new(key)),
            _ => Err(OcrError::NotConfigured),
        }
    }

    /// Override the model id.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override the per-request timeout.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

impl OcrEngine for MistralOcr {
    fn recognize(&self, image: &[u8], _page: usize) -> Result<String, OcrError> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let body = request_body(image, &self.model)
            .map_err(|e| OcrError::Backend(format!("base64 encode failed: {e}")))?;

        let mut child = Command::new("curl")
            .args([
                "-s",
                "-S",
                "-f", // non-zero exit on HTTP 4xx/5xx
                "--max-time",
                &self.timeout_secs.to_string(),
                "-X",
                "POST",
                "https://api.mistral.ai/v1/ocr",
                "-H",
                &format!("Authorization: Bearer {}", self.api_key),
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                "@-", // body on stdin: keeps it off argv (ARG_MAX)
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| OcrError::Backend(format!("curl spawn failed: {e}")))?;

        // Write stdin from a separate thread: reading stdout with
        // wait_with_output() on the main thread while writing stdin inline
        // can deadlock once curl fills the pipe buffer.
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| OcrError::Backend("curl stdin unavailable".into()))?;
            let data = body.into_bytes();
            let writer = std::thread::spawn(move || stdin.write_all(&data));
            let output = child
                .wait_with_output()
                .map_err(|e| OcrError::Backend(format!("curl wait failed: {e}")))?;
            writer
                .join()
                .map_err(|_| OcrError::Backend("stdin writer thread panicked".into()))?
                .map_err(|e| OcrError::Backend(format!("curl stdin write failed: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(OcrError::Backend(format!(
                    "mistral ocr curl exited with {}: {}",
                    output.status,
                    stderr.trim()
                )));
            }
            return parse_response(&output.stdout);
        }
    }
}

/// The JSON request body: the page image as a base64 data URI.
fn request_body(image: &[u8], model: &str) -> Result<String, OcrError> {
    // Base64 without an external crate: 3 bytes -> 4 chars, padded.
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::with_capacity(image.len().div_ceil(3) * 4);
    for chunk in image.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        b64.push(TABLE[(n >> 18) as usize & 63] as char);
        b64.push(TABLE[(n >> 12) as usize & 63] as char);
        b64.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        b64.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    Ok(format!(
        r#"{{"model":"{model}","document":{{"type":"image_url","image_url":"data:image/png;base64,{b64}"}}}}"#
    ))
}

/// Extract `pages[0].markdown` from the API response.
fn parse_response(stdout: &[u8]) -> Result<String, OcrError> {
    let json: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|e| OcrError::Backend(format!("mistral ocr returned invalid JSON: {e}")))?;
    json["pages"][0]["markdown"]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| OcrError::Backend("mistral ocr returned no markdown for the page".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_taken_from_the_first_page() {
        let body = r##"{"pages":[{"index":0,"markdown":"# Section 52 sample"}],"model":"mistral-ocr-latest"}"##;
        assert_eq!(parse_response(body.as_bytes()).unwrap(), "# Section 52 sample");
    }

    #[test]
    fn thai_markdown_survives_parsing() {
        // JSON strings cannot hold a literal newline, so the escaped
        // two-character sequence \n is what the API actually returns.
        let body = concat!(r#"{"pages":[{"index":0,"markdown":"มาตรา"#, r"\n", r#"ตัวอย่าง"}]}"#);
        let parsed = parse_response(body.as_bytes()).unwrap();
        assert!(parsed.contains('ม') && parsed.contains('\n'));
    }

    #[test]
    fn escaped_quotes_inside_markdown_survive_parsing() {
        let body = r#"{"pages":[{"markdown":"quoted \"text\" inside"}]}"#;
        assert_eq!(parse_response(body.as_bytes()).unwrap(), "quoted \"text\" inside");
    }

    #[test]
    fn an_empty_markdown_is_an_error_not_an_empty_page() {
        assert!(parse_response(r#"{"pages":[{"markdown":"  "}]}"#.as_bytes()).is_err());
    }

    #[test]
    fn a_response_without_pages_is_an_error() {
        assert!(parse_response(br#"{"detail":"Not Found"}"#).is_err());
        assert!(parse_response(b"not json at all").is_err());
    }

    #[test]
    fn base64_body_wraps_the_image_and_model() {
        // "PNG" bytes -> base64 "UE5H"
        let body = request_body(b"PNG", "mistral-ocr-latest").unwrap();
        assert!(body.contains(r#""data:image/png;base64,UE5H""#));
        // 1-byte input pads to a full quad: "P" (0x50) -> "UA==".
        let body = request_body(b"P", "m").unwrap();
        assert!(body.contains("UA=="));
        // No stray whitespace between the JSON glue and the data URI.
        assert!(body.contains(r#""image_url":"data:image"#));
    }

    #[test]
    fn a_missing_key_is_not_configured() {
        assert!(matches!(MistralOcr::from_optional_key(None), Err(OcrError::NotConfigured)));
        assert!(matches!(
            MistralOcr::from_optional_key(Some("   ".into())),
            Err(OcrError::NotConfigured)
        ));
        assert!(MistralOcr::from_optional_key(Some("sk-xx".into())).is_ok());
    }
}
