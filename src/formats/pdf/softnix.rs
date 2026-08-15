//! Softnix OCR cloud backend (feature `ocr-softnix`).
//!
//! Softnix OCR V3 is a job-based API: submit the page image, poll
//! `/status` until `completed`, then read `/result`. The base URL and token
//! come from `SOFTNIX_OCR_BASE_URL` + `SOFTNIX_OCR_TOKEN` (see
//! [`SoftnixOcr::from_env`]).
//!
//! HTTP goes through `curl`, matching the crate's zero-extra-deps policy;
//! responses are parsed with `serde_json`.

use std::time::Duration;

use super::ocr::{OcrEngine, OcrError};

/// Softnix OCR engine wrapper.
pub struct SoftnixOcr {
    /// e.g. `https://genai.softnix.ai/multipleocr`
    pub base_url: String,
    /// Bearer token from `POST /login` or a pre-issued token.
    pub token: String,
    /// `true` only for instances with a self-signed TLS certificate.
    pub insecure_tls: bool,
    /// Delay between `/status` polls.
    pub poll_interval: Duration,
    /// Overall job budget (submit + poll + result).
    pub timeout: Duration,
}

impl SoftnixOcr {
    /// Engine from explicit settings with sensible poll defaults.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            insecure_tls: false,
            poll_interval: Duration::from_millis(500),
            timeout: Duration::from_secs(120),
        }
    }

    /// Engine reading `SOFTNIX_OCR_BASE_URL` + `SOFTNIX_OCR_TOKEN`.
    ///
    /// `SOFTNIX_OCR_INSECURE_TLS=true` accepts self-signed certificates
    /// (development instances only). [`OcrError::NotConfigured`] when either
    /// required variable is absent, so a
    /// [`FallbackOcr`](super::fallback::FallbackOcr) chain can skip this
    /// backend and try the next one.
    pub fn from_env() -> Result<Self, OcrError> {
        let engine = Self::from_optional_parts(
            std::env::var("SOFTNIX_OCR_BASE_URL").ok(),
            std::env::var("SOFTNIX_OCR_TOKEN").ok(),
        )?;
        let insecure = std::env::var("SOFTNIX_OCR_INSECURE_TLS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);
        Ok(engine.with_insecure_tls(insecure))
    }

    /// Build from optional env values; any `None`/blank is
    /// [`OcrError::NotConfigured`].
    fn from_optional_parts(
        base_url: Option<String>,
        token: Option<String>,
    ) -> Result<Self, OcrError> {
        match (base_url, token) {
            (Some(base), Some(token)) if !base.trim().is_empty() && !token.trim().is_empty() => {
                Ok(Self::new(base, token))
            }
            _ => Err(OcrError::NotConfigured),
        }
    }

    /// Permit self-signed TLS (development instances only).
    pub fn with_insecure_tls(mut self, insecure: bool) -> Self {
        self.insecure_tls = insecure;
        self
    }

    /// Override the poll interval.
    pub fn with_poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    /// Override the overall timeout.
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// Run curl, check its exit status, and parse stdout as JSON.
    fn curl(
        &self,
        args: &[&str],
        stdin_data: Option<&[u8]>,
    ) -> Result<serde_json::Value, OcrError> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let max_time = self.timeout.as_secs().max(1).to_string();
        let mut full_args: Vec<&str> = Vec::new();
        if self.insecure_tls {
            full_args.push("-k");
        }
        full_args.extend(["-s", "-S", "-f", "--max-time", &max_time]);
        full_args.extend_from_slice(args);

        let mut child = Command::new("curl")
            .args(&full_args)
            .stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| OcrError::Backend(format!("curl spawn failed: {e}")))?;

        // stdin on a separate thread: writing it inline before
        // wait_with_output() deadlocks once the stdout pipe fills.
        let stdin_writer = stdin_data.map(|data| {
            let mut stdin = child.stdin.take().unwrap();
            let data = data.to_vec();
            std::thread::spawn(move || stdin.write_all(&data))
        });

        let output = child
            .wait_with_output()
            .map_err(|e| OcrError::Backend(format!("curl wait failed: {e}")))?;

        if let Some(handle) = stdin_writer {
            handle
                .join()
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

        // Step 1: submit the job. The image goes over stdin (`-F file=@-`),
        // so no temp file and no path string for curl -F to misread.
        let submit_url = format!("{}/v3/ai-process-file", self.base_url);
        let submit = self.curl(
            &[
                "-X",
                "POST",
                &submit_url,
                "-H",
                &auth,
                "-F",
                "file=@-;filename=page.png;type=image/png",
                "-F",
                "disable_structure=true",
            ],
            Some(image),
        )?;

        let job_id = submit["job_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                OcrError::Backend(format!("Softnix submit returned no job_id: {submit}"))
            })?
            .to_string();

        // Step 2: poll /status until completed or failed.
        let status_url = format!("{}/v3/ai-process-file/{job_id}/status", self.base_url);
        let started = std::time::Instant::now();
        loop {
            if started.elapsed() > self.timeout {
                return Err(OcrError::Backend(format!("Softnix job {job_id} timed out")));
            }
            let status = self.curl(&["-X", "GET", &status_url, "-H", &auth], None)?;
            match status["status"].as_str() {
                Some("completed") => break,
                Some("failed") => {
                    return Err(OcrError::Backend(format!(
                        "Softnix job {job_id} failed: {status}"
                    )));
                }
                _ => std::thread::sleep(self.poll_interval),
            }
        }

        // Step 3: fetch the result. Prefer ai_processing.content (Markdown)
        // only when ai_processing.success == true and it is non-empty;
        // otherwise fall back to raw ocr_text (OCR succeeded, VLM step
        // failed).
        let result_url = format!("{}/v3/ai-process-file/{job_id}/result", self.base_url);
        let result = self.curl(&["-X", "GET", &result_url, "-H", &auth], None)?;

        let pages = result["results"]["pages"]
            .as_array()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| OcrError::Backend(format!("Softnix result had no pages: {result}")))?;
        let page = &pages[0];
        let ai_content = page["ai_processing"]["success"]
            .as_bool()
            .unwrap_or(false)
            .then(|| page["ai_processing"]["content"].as_str())
            .flatten()
            .filter(|s| !s.is_empty());
        let ocr_text = page["ocr_text"].as_str().filter(|s| !s.is_empty());

        ai_content.or(ocr_text).map(str::to_string).ok_or_else(|| {
            OcrError::Backend(format!("Softnix result had no content/ocr_text: {result}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_env_parts_are_not_configured() {
        let url = Some("https://genai.softnix.ai/multipleocr".to_string());
        let tok = Some("secret".to_string());
        assert!(matches!(
            SoftnixOcr::from_optional_parts(None, None),
            Err(OcrError::NotConfigured)
        ));
        assert!(matches!(
            SoftnixOcr::from_optional_parts(url.clone(), None),
            Err(OcrError::NotConfigured)
        ));
        assert!(matches!(
            SoftnixOcr::from_optional_parts(None, tok.clone()),
            Err(OcrError::NotConfigured)
        ));
        assert!(matches!(
            SoftnixOcr::from_optional_parts(Some("  ".into()), tok),
            Err(OcrError::NotConfigured)
        ));
        assert!(SoftnixOcr::from_optional_parts(url, Some("secret".into())).is_ok());
    }
}
