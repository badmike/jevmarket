//! Client for Jev (TypeSafe AI's System One model) through OpenRouter's Decisions API.
//!
//! Jev is not a chat model. You hand it a JSON `state` and a map of typed `questions`; it
//! returns a typed decision per question with calibrated probabilities:
//!
//! ```text
//! POST https://openrouter.ai/api/alpha/decisions
//! { "model": "typesafe/jev-1.13", "state": {...}, "questions": { name: {...} } }
//! ```
//!
//! Three question primitives:
//! - `noul`: yes/no, answered as P(yes) in [0, 1]
//! - `choice`: one key out of `criteria` {key: description}, plus per-key probabilities
//! - `score`: position on an ordinal rubric `criteria` [level0, level1, ...], plus probabilities

use std::cell::Cell;
use std::collections::HashMap;
use std::time::Duration;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::openrouter::{OpenRouter, OpenRouterError, Policy};

const SERVICE: &str = "Jev";
const POLICY: Policy = Policy {
    service: SERVICE,
    timeout: Duration::from_secs(30),
    max_attempts: 4,
    base_delay: Duration::from_millis(500),
};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Noul { instructions: String },
    Choice { instructions: String, criteria: IndexMap<String, String> },
    Score { instructions: String, criteria: Vec<String> },
}

impl Question {
    pub fn noul(instructions: impl Into<String>) -> Self {
        Self::Noul { instructions: instructions.into() }
    }

    /// Between 2 and 255 options.
    pub fn choice<'a>(instructions: impl Into<String>, options: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let criteria: IndexMap<_, _> = options.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        assert!((2..=255).contains(&criteria.len()), "choice needs between 2 and 255 options");
        Self::Choice { instructions: instructions.into(), criteria }
    }

    /// Between 2 and 10 rubric levels, lowest first.
    pub fn score<'a>(instructions: impl Into<String>, levels: impl IntoIterator<Item = &'a str>) -> Self {
        let criteria: Vec<_> = levels.into_iter().map(str::to_owned).collect();
        assert!((2..=10).contains(&criteria.len()), "score needs between 2 and 10 levels");
        Self::Score { instructions: instructions.into(), criteria }
    }
}

pub type Questions = IndexMap<&'static str, Question>;

/// One typed answer. Kept as raw JSON on purpose: the endpoint is in beta and field names drift.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct Answer(Map<String, Value>);

impl Answer {
    fn first(&self, names: &[&str]) -> Option<&Value> {
        names.iter().find_map(|n| self.0.get(*n).filter(|v| !v.is_null()))
    }

    /// P(yes) for a noul question.
    pub fn noul(&self) -> Option<f64> {
        match self.first(&["noul", "probability", "p", "value"])? {
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            v => v.as_f64(),
        }
    }

    pub fn choice(&self) -> Option<String> {
        self.first(&["choice", "value"]).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    /// Expected rubric position as returned (e.g. 2.92), not a level.
    pub fn score_mean(&self) -> Option<f64> {
        self.first(&["score", "value"])?.as_f64()
    }

    /// Nearest rubric level.
    pub fn score(&self) -> Option<u8> {
        self.score_mean().map(|v| v.round().clamp(0.0, 255.0) as u8)
    }

    pub fn confidence(&self) -> Option<f64> {
        self.first(&["confidence"])?.as_f64()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Decision {
    pub model: Option<String>,
    pub answers: HashMap<String, Answer>,
    #[serde(default)]
    pub usage: Usage,
}

impl Decision {
    pub fn answer(&self, name: &str) -> Answer {
        self.answers.get(name).cloned().unwrap_or_default()
    }
}

pub struct JevClient {
    api: OpenRouter,
    model: String,
    pub calls: Cell<u32>,
    pub total_input_tokens: Cell<u64>,
    pub total_cost: Cell<f64>,
}

impl JevClient {
    pub fn new(api: OpenRouter, model: impl Into<String>) -> Self {
        Self {
            api,
            model: model.into(),
            calls: Cell::new(0),
            total_input_tokens: Cell::new(0),
            total_cost: Cell::new(0.0),
        }
    }

    /// POST and return the raw JSON body (for schema inspection).
    pub async fn decide_raw(&self, state: &Value, questions: &Questions) -> Result<Value, OpenRouterError> {
        let body = json!({ "model": self.model, "state": state, "questions": questions });
        self.api.post("/alpha/decisions", &body, POLICY).await
    }

    /// Ask and parse. Returns the parsed decision plus the raw body for logging.
    pub async fn decide(&self, state: &Value, questions: &Questions) -> Result<(Decision, Value), OpenRouterError> {
        let raw = self.decide_raw(state, questions).await?;
        let d = parse(&raw)?;
        self.calls.set(self.calls.get() + 1);
        self.total_cost.set(self.total_cost.get() + d.usage.cost);
        self.total_input_tokens.set(self.total_input_tokens.get() + d.usage.input_tokens);
        Ok((d, raw))
    }
}

pub fn parse(raw: &Value) -> Result<Decision, OpenRouterError> {
    Decision::deserialize(raw).map_err(|e| OpenRouterError::invalid(SERVICE, format!("unexpected response shape: {e}")))
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn sample() -> Value {
        json!({
            "id": "gen-dec-1",
            "model": "typesafe/jev-1.13-20260917",
            "provider": "TypeSafe",
            "answers": {
                "urgent": {"noul": 0.83},
                "queue": {"choice": "billing", "probabilities": {"billing": 0.9, "technical": 0.1}, "confidence": 0.88},
                "anger": {"score": 2.92, "legend": ["calm", "annoyed", "angry", "furious"],
                          "probabilities": [0.05, 0.1, 0.25, 0.6], "confidence": 0.7}
            },
            "usage": {"input_tokens": 476, "output_tokens": 0, "cost": 0.000019992}
        })
    }

    async fn client(server: &MockServer) -> JevClient {
        JevClient::new(OpenRouter::new("k", &server.uri()), "typesafe/jev-1.13")
    }

    fn questions() -> Questions {
        IndexMap::from([("q", Question::noul("?"))])
    }

    #[test]
    fn builders_serialize_to_the_wire_format() {
        assert_eq!(serde_json::to_value(Question::noul("x")).unwrap(), json!({"type": "noul", "instructions": "x"}));
        let c = serde_json::to_value(Question::choice("x", [("a", "A"), ("b", "B")])).unwrap();
        assert_eq!(c["criteria"], json!({"a": "A", "b": "B"}));
        let s = serde_json::to_value(Question::score("x", ["l0", "l1"])).unwrap();
        assert_eq!(s, json!({"type": "score", "instructions": "x", "criteria": ["l0", "l1"]}));
    }

    #[test]
    #[should_panic(expected = "between 2 and 255")]
    fn choice_needs_two_options() {
        Question::choice("x", [("a", "A")]);
    }

    #[tokio::test]
    async fn decide_parses_answers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/alpha/decisions"))
            .and(header("authorization", "Bearer k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample()))
            .expect(1)
            .mount(&server)
            .await;
        let jev = client(&server).await;
        let (d, _) = jev.decide(&json!({"t": "hi"}), &questions()).await.unwrap();
        assert_eq!(d.answer("urgent").noul(), Some(0.83));
        assert_eq!(d.answer("queue").choice().as_deref(), Some("billing"));
        assert_eq!(d.answer("queue").confidence(), Some(0.88));
        assert_eq!(d.answer("anger").score(), Some(3));
        assert_eq!(d.answer("anger").score_mean(), Some(2.92));
        assert!((jev.total_cost.get() - 0.000019992).abs() < 1e-12);
        assert_eq!(jev.total_input_tokens.get(), 476);
    }

    #[test]
    fn boolean_noul_is_coerced() {
        let d = parse(&json!({"answers": {"q": {"noul": true}}})).unwrap();
        assert_eq!(d.answer("q").noul(), Some(1.0));
    }

    #[tokio::test]
    async fn payment_required_is_fatal_with_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(402)
                    .set_body_json(json!({"error": {"code": 402, "message": "Insufficient credits"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let err = client(&server).await.decide(&json!({}), &questions()).await.unwrap_err();
        assert!(err.is_fatal());
        assert!(err.to_string().contains("Insufficient credits"));
    }

    #[tokio::test]
    async fn retries_on_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({"error": {"message": "slow down"}})))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample()))
            .expect(1)
            .mount(&server)
            .await;
        let (d, _) = client(&server).await.decide(&json!({}), &questions()).await.unwrap();
        assert_eq!(d.answer("urgent").noul(), Some(0.83));
    }
}
