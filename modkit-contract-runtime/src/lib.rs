pub mod directory;

use std::time::Duration;

use futures_core::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

pub use directory::{ClientConfig, RetryConfig};

// ---------------------------------------------------------------------------
// ProblemDetails (RFC 9457 + CyberFabric extensions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    pub error_code: String,
    pub error_domain: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub context: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Retry helper — used by generated REST clients for retryable methods
// ---------------------------------------------------------------------------

/// Execute an async operation with retry logic.
/// Used by generated REST clients for methods marked `#[modkit_contract(retryable)]`.
pub async fn with_retry<F, Fut, T, E>(
    config: &RetryConfig,
    is_retryable: impl Fn(&E) -> bool,
    mut op: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_err = None;

    for attempt in 0..=config.max_retries {
        match op().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt < config.max_retries && is_retryable(&e) {
                    let delay = config.delay_for_attempt(attempt);
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                } else {
                    return Err(e);
                }
            }
        }
    }

    Err(last_err.unwrap())
}

// ---------------------------------------------------------------------------
// SSE stream parser — used by generated REST clients
// ---------------------------------------------------------------------------

/// Parse a byte stream (from reqwest) into a stream of deserialized SSE events.
pub fn sse_stream<T, S>(byte_stream: S) -> impl Stream<Item = T>
where
    T: for<'de> Deserialize<'de> + Send + 'static,
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    futures_util::stream::unfold(
        SseParser::new(byte_stream),
        |mut parser| async move {
            loop {
                match parser.next_event::<T>().await {
                    Some(Ok(event)) => return Some((event, parser)),
                    Some(Err(_)) => continue,
                    None => return None,
                }
            }
        },
    )
}

struct SseParser<S> {
    stream: S,
    buffer: String,
}

impl<S> SseParser<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: String::new(),
        }
    }

    async fn next_event<T: for<'de> Deserialize<'de>>(
        &mut self,
    ) -> Option<Result<T, String>> {
        loop {
            if let Some(result) = self.try_parse_event() {
                return Some(result);
            }
            match self.stream.next().await {
                Some(Ok(chunk)) => {
                    let text = String::from_utf8_lossy(&chunk);
                    self.buffer.push_str(&text);
                }
                Some(Err(e)) => {
                    let msg: String = e.to_string();
                    return Some(Err(msg));
                }
                None => {
                    if self.buffer.trim().is_empty() {
                        return None;
                    }
                    return self.try_parse_event();
                }
            }
        }
    }

    fn try_parse_event<T: for<'de> Deserialize<'de>>(
        &mut self,
    ) -> Option<Result<T, String>> {
        let separator = "\n\n";
        let pos = self.buffer.find(separator)?;
        let event_block = self.buffer[..pos].to_owned();
        self.buffer = self.buffer[pos + separator.len()..].to_owned();

        let mut data = String::new();
        for line in event_block.lines() {
            if line.starts_with(':') {
                continue;
            }
            if let Some(value) = line.strip_prefix("data: ") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
        }
        if data.is_empty() {
            return None;
        }
        match serde_json::from_str::<T>(&data) {
            Ok(event) => Some(Ok(event)),
            Err(e) => Some(Err(format!("JSON parse error: {e}"))),
        }
    }
}
