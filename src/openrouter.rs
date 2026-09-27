//! Shared OpenRouter transport for Jev and the researcher: one pooled HTTP client,
//! bearer auth, attribution headers, and retries on 429 / 5xx / network errors.

use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;

pub const REPO_URL: &str = "https://github.com/badmike/jevmarket";

#[derive(Debug, thiserror::Error)]
pub enum OpenRouterError {
    #[error("{service}/OpenRouter {status}: {message}")]
    Status { service: &'static str, status: u16, message: String },
    #[error("{service}/OpenRouter network error: {source}")]
    Network {
        service: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("{service}: {message}")]
    Invalid { service: &'static str, message: String },
}

impl OpenRouterError {
    pub fn invalid(service: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid { service, message: message.into() }
    }

    /// Bad key or no credits: retrying other markets would fail the same way.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Status { status: 401 | 402, .. })
    }
}

#[derive(Debug, Clone)]
pub struct OpenRouter {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

/// Per-endpoint call policy.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub service: &'static str,
    pub timeout: Duration,
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl OpenRouter {
    pub fn new(api_key: &str, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.to_owned(),
        }
    }

    /// POST `body` to `path` and return the JSON response.
    pub async fn post(&self, path: &str, body: &impl Serialize, policy: Policy) -> Result<Value, OpenRouterError> {
        let url = format!("{}{path}", self.base_url);
        let mut delay = policy.base_delay;
        let mut attempt = 1;
        loop {
            let err = match self.send(self.http.post(&url).json(body), policy).await {
                Ok(v) => return Ok(v),
                Err(e) => e,
            };
            let retryable = match &err {
                OpenRouterError::Status { status, .. } => *status == 429 || *status >= 500,
                OpenRouterError::Network { .. } => true,
                OpenRouterError::Invalid { .. } => false,
            };
            if !retryable || attempt >= policy.max_attempts {
                return Err(err);
            }
            let sleep = delay.mul_f64(1.0 + fastrand::f64());
            tracing::warn!("{err}; retry {attempt}/{} in {:.1}s", policy.max_attempts, sleep.as_secs_f64());
            tokio::time::sleep(sleep).await;
            delay *= 2;
            attempt += 1;
        }
    }

    /// GET `path` once, without retries. For interactive checks like key validation.
    pub async fn get(&self, path: &str, policy: Policy) -> Result<Value, OpenRouterError> {
        self.send(self.http.get(format!("{}{path}", self.base_url)), policy).await
    }

    async fn send(&self, request: reqwest::RequestBuilder, policy: Policy) -> Result<Value, OpenRouterError> {
        let service = policy.service;
        let network = |source| OpenRouterError::Network { service, source };
        let resp = request
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", REPO_URL)
            .header("X-Title", "jevmarket")
            .timeout(policy.timeout)
            .send()
            .await
            .map_err(network)?;
        let status = resp.status();
        let text = resp.text().await.map_err(network)?;
        if status.is_success() {
            return serde_json::from_str(&text)
                .map_err(|e| OpenRouterError::invalid(service, format!("response is not JSON: {e}")));
        }
        Err(OpenRouterError::Status { service, status: status.as_u16(), message: error_message(status, &text) })
    }
}

/// The most useful human-readable part of an error body.
fn error_message(status: StatusCode, text: &str) -> String {
    if let Ok(body) = serde_json::from_str::<Value>(text) {
        let msg = match body.get("error") {
            Some(Value::Object(err)) => err.get("message").map_or_else(|| as_text(&body["error"]), as_text),
            Some(Value::Null) | None => body.get("detail").map(as_text).unwrap_or_default(),
            Some(other) => as_text(other),
        };
        if !msg.is_empty() {
            return msg;
        }
    }
    let text = text.trim();
    if text.is_empty() {
        status.canonical_reason().unwrap_or_default().to_owned()
    } else {
        text.chars().take(300).collect()
    }
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_shapes() {
        let s = StatusCode::PAYMENT_REQUIRED;
        assert_eq!(
            error_message(s, r#"{"error":{"code":402,"message":"Insufficient credits"}}"#),
            "Insufficient credits"
        );
        assert_eq!(error_message(s, r#"{"error":"nope"}"#), "nope");
        assert_eq!(error_message(s, r#"{"detail":"bad state"}"#), "bad state");
        assert_eq!(error_message(s, "plain"), "plain");
        assert_eq!(error_message(s, ""), "Payment Required");
    }
}
