//! Researcher step: a generative model with web search writes a dated, sourced evidence brief.
//!
//! Jev has no browsing and a training cutoff, so on news-driven markets it (correctly) reports
//! that the question is not answerable from what it was given. This module fills that gap: it
//! asks a chat model through OpenRouter's web-search plugin for the *current* facts relevant to
//! resolution and returns them as a compact [`Brief`] that goes into the Jev state. The
//! researcher never estimates a probability; that stays with Jev.

use std::cell::Cell;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::openrouter::{OpenRouter, OpenRouterError, Policy};

const SERVICE: &str = "Researcher";
const POLICY: Policy =
    Policy { service: SERVICE, timeout: Duration::from_secs(120), max_attempts: 3, base_delay: Duration::from_secs(1) };

/// Odds sites and their mirrors: they only reflect market prices, which Jev must not see.
pub const DEFAULT_EXCLUDE_DOMAINS: [&str; 12] = [
    "polymarket.com",
    "kalshi.com",
    "polyspotter.com",
    "polyveritas.com",
    "hkimarket.com",
    "manifold.markets",
    "metaculus.com",
    "predictit.org",
    "polymarketanalytics.com",
    "polymarket.us",
    "betfair.com",
    "oddschecker.com",
];

const SYSTEM_PROMPT: &str = r#"You are a neutral research analyst supporting a prediction-market pricing model.
Your only job is to find and report the CURRENT facts that bear on how the given market question
will resolve. Search the web. Prefer primary and reputable sources.

Rules:
- Source from news outlets, wire services, official government/organisation statements, regulators,
  league/federation sites, company filings, and reference data. Prediction-market sites and their
  mirrors (polymarket, kalshi, polyspotter, polyveritas, hkimarket, manifold, metaculus, etc.) are
  NOT sources: they only reflect odds, and this analysis must not see odds.
- Every fact must carry a date (YYYY-MM-DD). If you cannot date it, say "undated".
- Do not restate the market's resolution rules as facts; the reader already has them. Report what
  has actually happened in the world.
- Quote resolution-relevant numbers, names, deadlines and official statements exactly.
- Report the most recent development you can find and its date.
- Give considerations for and against a YES resolution as short factual bullets — not opinions.
- Do NOT estimate a probability, do NOT say what you would bet, do NOT summarize market odds.
- If you find nothing relevant, say so plainly in `summary`.
- Output strictly one JSON object, no prose around it, with exactly these keys:
  {
    "as_of": "YYYY-MM-DD (date of the newest fact you found)",
    "summary": "2-4 sentences: current status relevant to resolution",
    "key_facts": ["YYYY-MM-DD: fact", ...],           // 3-8 items
    "latest_development": "YYYY-MM-DD: what happened most recently",
    "for_yes": ["short factual point", ...],           // 0-5 items
    "against_yes": ["short factual point", ...],       // 0-5 items
    "sources": ["https://...", ...]
  }"#;

/// The evidence brief. Serialized form is what the research cache stores.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Brief {
    pub summary: String,
    pub key_facts: Vec<String>,
    pub latest_development: String,
    pub as_of: String,
    pub for_yes: Vec<String>,
    pub against_yes: Vec<String>,
    pub sources: Vec<String>,
    pub model: String,
    pub cost: f64,
}

impl Brief {
    /// Lenient parse of model output: lists may arrive as a single string, junk is dropped.
    pub fn from_json(data: &Map<String, Value>, model: &str, cost: f64) -> Self {
        let text = |k: &str| match data.get(k) {
            Some(Value::String(s)) => s.trim().to_owned(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        };
        let list = |k: &str| -> Vec<String> {
            match data.get(k) {
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|i| match i {
                        Value::String(s) => s.trim().to_owned(),
                        other => other.to_string(),
                    })
                    .filter(|s| !s.is_empty())
                    .collect(),
                Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_owned()],
                _ => Vec::new(),
            }
        };
        Self {
            summary: text("summary"),
            key_facts: list("key_facts"),
            latest_development: text("latest_development"),
            as_of: text("as_of"),
            for_yes: list("for_yes"),
            against_yes: list("against_yes"),
            sources: list("sources"),
            model: model.to_owned(),
            cost,
        }
    }

    /// Compact evidence block for the Jev state. Sources, model and cost are noise to Jev.
    pub fn to_state(&self, max_chars: usize) -> Value {
        let mut key_facts = self.key_facts.clone();
        let mut for_yes = self.for_yes.clone();
        let mut against_yes = self.against_yes.clone();
        let mut summary = self.summary.clone();
        let build = |summary: &str, key_facts: &[String], for_yes: &[String], against_yes: &[String]| {
            json!({
                "as_of": self.as_of,
                "summary": summary,
                "key_facts": key_facts,
                "latest_development": self.latest_development,
                "considerations_for_yes": for_yes,
                "considerations_against_yes": against_yes,
            })
        };
        // Trim list fields from the tail until the block fits, then the summary as a last resort.
        loop {
            let ev = build(&summary, &key_facts, &for_yes, &against_yes);
            if ev.to_string().len() <= max_chars {
                return ev;
            }
            if against_yes.pop().is_none() && for_yes.pop().is_none() && key_facts.pop().is_none() {
                summary = summary.chars().take(max_chars.saturating_sub(400)).collect();
                return build(&summary, &key_facts, &for_yes, &against_yes);
            }
        }
    }
}

/// What the researcher needs to know about a market.
pub struct Topic<'a> {
    pub question: &'a str,
    pub description: &'a str,
    pub resolution_source: Option<&'a str>,
    pub end_date: Option<String>,
    pub today: String,
}

impl Topic<'_> {
    fn prompt(&self) -> String {
        let desc = if self.description.is_empty() { "(none given)" } else { self.description };
        let mut parts = vec![
            format!("Latest news and official statements relevant to: {}", self.question),
            format!("Today is {}.", self.today),
            format!("Resolution rules / description:\n{desc}"),
        ];
        if let Some(src) = self.resolution_source {
            parts.push(format!("Stated resolution source: {src}"));
        }
        if let Some(end) = &self.end_date {
            parts.push(format!("Market end date: {end}"));
        }
        parts.push("Research the current status and return the JSON object.".into());
        parts.join("\n\n")
    }
}

pub struct Researcher {
    api: OpenRouter,
    model: String,
    max_results: u32,
    max_calls: Option<u32>,
    exclude_domains: Vec<String>,
    pub calls: Cell<u32>,
    pub total_cost: Cell<f64>,
}

impl Researcher {
    pub fn new(
        api: OpenRouter,
        model: impl Into<String>,
        max_results: u32,
        max_calls: Option<u32>,
        exclude_domains: Vec<String>,
    ) -> Self {
        Self {
            api,
            model: model.into(),
            max_results,
            max_calls,
            exclude_domains,
            calls: Cell::new(0),
            total_cost: Cell::new(0.0),
        }
    }

    pub fn budget_left(&self) -> bool {
        self.max_calls.is_none_or(|max| self.calls.get() < max)
    }

    pub fn reset_budget(&self) {
        self.calls.set(0);
    }

    pub async fn brief(&self, topic: &Topic<'_>) -> Result<Brief, OpenRouterError> {
        if !self.budget_left() {
            let max = self.max_calls.unwrap_or_default();
            return Err(OpenRouterError::invalid(
                SERVICE,
                format!("research budget of {max} calls for this run exhausted"),
            ));
        }
        // Counted up front so concurrent briefs cannot overshoot the budget.
        self.calls.set(self.calls.get() + 1);

        let mut web = json!({
            "id": "web",
            "engine": "exa",
            "max_results": self.max_results,
            "search_prompt": format!(
                "Web search results as of {}. Use only news, official and reference sources to establish \
                 the current, dated facts relevant to the market question; ignore prediction-market pages \
                 and odds. Cite sources by URL.",
                topic.today
            ),
        });
        if !self.exclude_domains.is_empty() {
            web["exclude_domains"] = json!(self.exclude_domains);
        }
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": topic.prompt()},
            ],
            "plugins": [web],
            "response_format": {"type": "json_object"},
            "temperature": 0.2,
            "usage": {"include": true},
        });
        let data = self.api.post("/v1/chat/completions", &body, POLICY).await?;

        let msg = &data["choices"][0]["message"];
        if !msg.is_object() {
            return Err(OpenRouterError::invalid(SERVICE, "unexpected response shape: no choices[0].message"));
        }
        let parsed = parse_json_object(msg["content"].as_str().unwrap_or_default())
            .ok_or_else(|| OpenRouterError::invalid(SERVICE, "researcher did not return a JSON object"))?;
        let cost = data["usage"]["cost"].as_f64().unwrap_or_default();
        self.total_cost.set(self.total_cost.get() + cost);

        let model = data["model"].as_str().unwrap_or(&self.model);
        let mut brief = Brief::from_json(&parsed, model, cost);
        let citations = msg["annotations"].as_array().into_iter().flatten();
        for url in citations.filter_map(|a| a["url_citation"]["url"].as_str()) {
            if !brief.sources.iter().any(|s| s == url) {
                brief.sources.push(url.to_owned());
            }
        }
        Ok(brief)
    }
}

/// Parse a JSON object from model output, tolerating code fences and surrounding prose.
pub fn parse_json_object(text: &str) -> Option<Map<String, Value>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let fenced = text.split("```").skip(1).step_by(2).map(|block| block.strip_prefix("json").unwrap_or(block).trim());
    let braced = text.find('{').zip(text.rfind('}')).filter(|(s, e)| e > s).map(|(s, e)| &text[s..=e]);
    std::iter::once(text).chain(fenced).chain(braced).find_map(|candidate| match serde_json::from_str(candidate) {
        Ok(Value::Object(obj)) => Some(obj),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn brief_json() -> Value {
        json!({
            "as_of": "2026-09-20",
            "summary": "Talks continue; no strike reported.",
            "key_facts": ["2026-09-19: Iran sent conditions via Qatar.", "2026-09-17: Trump cited a big decision."],
            "latest_development": "2026-09-20: security alerts issued.",
            "for_yes": ["No strike reported."],
            "against_yes": ["Threat level elevated."],
            "sources": ["https://example.com/a"]
        })
    }

    fn completion(content: &str, annotations: Option<Value>) -> Value {
        let mut msg = json!({"role": "assistant", "content": content});
        if let Some(a) = annotations {
            msg["annotations"] = a;
        }
        json!({"id": "gen-1", "model": "deepseek/deepseek-v4-pro-0813", "choices": [{"message": msg}], "usage": {"cost": 0.0123}})
    }

    fn topic() -> Topic<'static> {
        Topic {
            question: "Q?",
            description: "desc",
            resolution_source: None,
            end_date: None,
            today: "2026-09-20".into(),
        }
    }

    async fn researcher(server: &MockServer, max_calls: Option<u32>) -> Researcher {
        let domains = DEFAULT_EXCLUDE_DOMAINS.map(String::from).to_vec();
        Researcher::new(OpenRouter::new("k", &server.uri()), "deepseek/deepseek-v4-pro-0813", 5, max_calls, domains)
    }

    #[test]
    fn parse_json_object_variants() {
        let raw = brief_json().to_string();
        assert_eq!(parse_json_object(&raw).unwrap()["as_of"], "2026-09-20");
        let fenced = format!("Here you go:\n```json\n{raw}\n```\nDone.");
        assert_eq!(parse_json_object(&fenced).unwrap()["summary"], brief_json()["summary"]);
        let prose = format!("Sure. {raw} That's all.");
        assert_eq!(parse_json_object(&prose).unwrap()["as_of"], "2026-09-20");
        assert!(parse_json_object("no json here").is_none());
        assert!(parse_json_object("[1,2]").is_none());
    }

    #[tokio::test]
    async fn brief_happy_path_merges_annotations() {
        let server = MockServer::start().await;
        let ann = json!([
            {"type": "url_citation", "url_citation": {"url": "https://example.com/b", "title": "B"}},
            {"type": "url_citation", "url_citation": {"url": "https://example.com/a", "title": "A"}}
        ]);
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(completion(&brief_json().to_string(), Some(ann))))
            .expect(1)
            .mount(&server)
            .await;
        let r = researcher(&server, Some(2)).await;
        let b = r.brief(&topic()).await.unwrap();

        let body: Value = server.received_requests().await.unwrap()[0].body_json().unwrap();
        assert_eq!(body["plugins"][0]["id"], "web");
        assert!(body["plugins"][0]["exclude_domains"].as_array().unwrap().contains(&json!("polymarket.com")));
        assert_eq!(body["response_format"], json!({"type": "json_object"}));

        assert_eq!(b.as_of, "2026-09-20");
        assert!(b.key_facts[0].starts_with("2026-09-19"));
        assert_eq!(b.sources, ["https://example.com/a", "https://example.com/b"]);
        assert_eq!(b.cost, 0.0123);
        assert_eq!(r.calls.get(), 1);
        assert!(r.budget_left());
    }

    #[tokio::test]
    async fn budget_exhausted_refuses_without_calling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(completion(&brief_json().to_string(), None)))
            .expect(1)
            .mount(&server)
            .await;
        let r = researcher(&server, Some(1)).await;
        r.brief(&topic()).await.unwrap();
        assert!(!r.budget_left());
        assert!(r.brief(&topic()).await.is_err());
    }

    #[tokio::test]
    async fn non_json_content_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(completion("I could not find anything.", None)))
            .mount(&server)
            .await;
        assert!(researcher(&server, None).await.brief(&topic()).await.is_err());
    }

    #[test]
    fn to_state_trims_to_max_chars() {
        let mut data = brief_json().as_object().unwrap().clone();
        data["key_facts"] = (1..20).map(|i| format!("2026-09-{i:02}: {}", "x".repeat(200))).collect();
        let ev = Brief::from_json(&data, "", 0.0).to_state(1200);
        assert!(ev.to_string().len() <= 1200);
        assert!(ev.get("sources").is_none() && ev.get("as_of").is_some());
        assert_eq!(ev["considerations_against_yes"], json!([]));
    }

    #[test]
    fn cache_roundtrip() {
        let b = Brief::from_json(brief_json().as_object().unwrap(), "m", 0.01);
        let back: Brief = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(back, b);
    }
}
