//! The TypeSafe analysis pipeline ported from the TypeScript `src/` modules:
//! a TypeSafe HTTP client, the per-message `state` builder, live and dry-run
//! evaluators, profile/bot aggregation, and guardrails routing. The question
//! content comes from `assets.rs` (generated from `src/`), so the questions,
//! thresholds, and policies match the CLI exactly.

use std::collections::HashMap;

use futures_util::stream::{self, StreamExt};
use serde::Serialize;

use crate::assets::{self, assets};

/// A routing action, mirroring the TypeSafe guardrails cookbook.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskAction {
    Pass,
    Review,
    Block,
    Support,
}

impl RiskAction {
    fn from_str(value: &str) -> RiskAction {
        match value {
            "review" => RiskAction::Review,
            "block" => RiskAction::Block,
            "support" => RiskAction::Support,
            _ => RiskAction::Pass,
        }
    }
}

/// Token usage for a set of TypeSafe calls.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Per-message scores across the profile or bot battery (not serialized).
#[derive(Debug, Clone, Default)]
pub struct TurnScores {
    pub scores: HashMap<String, f64>,
    pub confidence: HashMap<String, f64>,
    pub self_harm: f64,
    pub severity: f64,
    pub usage: Usage,
}

// ---------------------------------------------------------------------------
// TypeSafe client
// ---------------------------------------------------------------------------

/// A minimal TypeSafe client: POSTs `{ state, questions, model }` to
/// `/v1/systemone` and returns the parsed answers. No retries, no SDK logging.
/// `Clone` is cheap (`reqwest::Client` is internally reference-counted).
#[derive(Clone)]
pub struct TypeSafeClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct SystemOneResponse {
    #[serde(default)]
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) answers: HashMap<String, RawAnswer>,
    #[serde(default)]
    pub(crate) usage: RawUsage,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct RawAnswer {
    #[serde(rename = "type")]
    pub(crate) answer_type: String,
    #[serde(default)]
    pub(crate) score: Option<f64>,
    #[serde(default)]
    pub(crate) noul: Option<f64>,
    #[serde(default)]
    pub(crate) confidence: Option<f64>,
    #[serde(default)]
    pub(crate) choice: Option<String>,
    #[serde(default)]
    pub(crate) probabilities: Option<HashMap<String, f64>>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct RawUsage {
    #[serde(default)]
    pub(crate) input_tokens: u64,
    #[serde(default)]
    pub(crate) output_tokens: u64,
}

impl TypeSafeClient {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self, String> {
        if api_key.trim().is_empty() {
            return Err("No TypeSafe API key configured. Set it in Settings or TYPESAFE_API_KEY.".to_string());
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| format!("client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        })
    }

    /// The configured default model, used as a fallback when a response omits it.
    pub(crate) fn model_name(&self) -> &str {
        &self.model
    }

    pub(crate) async fn system_one(
        &self,
        state: serde_json::Value,
        questions: serde_json::Map<String, serde_json::Value>,
    ) -> Result<SystemOneResponse, String> {        let url = format!("{}/v1/systemone", self.base_url);
        let body = serde_json::json!({
            "state": state,
            "questions": questions,
            "model": self.model,
        });
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("TypeSafe request failed: {e}"))?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("TypeSafe HTTP {status}: {text}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("TypeSafe bad response: {e}"))
    }
}

// ---------------------------------------------------------------------------
// State builder
// ---------------------------------------------------------------------------

fn regex(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("static regex must compile")
}

/// Build the structured `state` for one message, mirroring `buildState` in
/// `src/parse.ts`.
pub fn build_state(
    content: &str,
    timestamp: Option<&str>,
    channel: Option<&str>,
) -> serde_json::Value {
    let trimmed = content.trim();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let mentions: Vec<String> = regex(r"@([\w.-]+)")
        .captures_iter(content)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();
    let has_link = regex(r"\bhttps?://\S+").is_match(content);
    let has_code = regex(r"`{1,3}").is_match(content);
    let is_question = regex(r"\?\s*$").is_match(trimmed)
        || regex(r"^(how|what|why|when|where|who|can|could|would|should|is|are|does|do|did|will|shall|might|may)\b")
            .is_match(trimmed.to_lowercase().as_str());
    let is_reply = regex(r"(?s)>").is_match(content) || regex(r"\breplying to\b").is_match(content);

    serde_json::json!({
        "content": content,
        "length_chars": content.chars().count(),
        "length_words": words.len(),
        "timestamp": timestamp,
        "channel": channel,
        "mentions": mentions,
        "has_link": has_link,
        "has_code": has_code,
        "is_question": is_question,
        "is_reply": is_reply,
    })
}

// ---------------------------------------------------------------------------
// Normalization and scrubbing
// ---------------------------------------------------------------------------

pub(crate) fn clamp01(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Remove direct identifiers and collapse whitespace, mirroring `scrubExcerpt`.
fn scrub(text: &str) -> String {
    let mut out = regex(r"(?i)[\w.+-]+@[\w-]+\.[a-z]{2,}").replace_all(text, "[email]").to_string();
    out = regex(r"\b(?:\+?\d{1,2}[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}\b")
        .replace_all(&out, "[phone]")
        .to_string();
    out = regex(r"(?i)\bhttps?://\S+").replace_all(&out, "[link]").to_string();
    out = regex(r"@[\w.-]+").replace_all(&out, "@handle").to_string();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A scrubbed, truncated excerpt (≤ `excerpt_max` chars), mirroring `excerptOf`.
pub fn excerpt_of(text: &str) -> String {
    let scrubbed = scrub(text);
    let max = assets().excerpt_max;
    if scrubbed.chars().count() <= max {
        return scrubbed;
    }
    let truncated: String = scrubbed.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}

// ---------------------------------------------------------------------------
// Dry-run evaluator (deterministic, keyless)
// ---------------------------------------------------------------------------

/// 32-bit FNV-1a over UTF-16 code units, matching the TypeScript dry-run hash.
fn fnv1a(text: &str) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for unit in text.encode_utf16() {
        hash ^= unit as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn dry_run_scores(ids: &[&str], content: &str) -> (HashMap<String, f64>, HashMap<String, f64>) {
    let mut scores = HashMap::new();
    let mut confidence = HashMap::new();
    for id in ids {
        let hash = fnv1a(&format!("{id}\u{0}{content}"));
        scores.insert((*id).to_string(), (hash % 1000) as f64 / 1000.0);
        confidence.insert((*id).to_string(), 0.5 + ((hash >> 5) % 500) as f64 / 1000.0);
    }
    (scores, confidence)
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Score one message across the profile dimensions.
async fn evaluate_profile(client: &TypeSafeClient, state: serde_json::Value) -> Result<TurnScores, String> {
    let response = client.system_one(state, assets::profile_questions()).await?;
    let mut result = TurnScores::default();
    result.usage = Usage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
    };
    for dimension in &assets().dimensions {
        let answer = response
            .answers
            .get(&dimension.id)
            .ok_or_else(|| format!("TypeSafe returned no answer for {}.", dimension.id))?;
        if answer.answer_type != "score" {
            return Err(format!(
                "TypeSafe returned \"{}\" for {}, expected \"score\".",
                answer.answer_type, dimension.id
            ));
        }
        let score = answer.score.ok_or_else(|| format!("missing score for {}", dimension.id))?;
        let confidence = answer.confidence.unwrap_or(0.0);
        result.scores.insert(dimension.id.clone(), clamp01(score / assets().rubric_max));
        result.confidence.insert(dimension.id.clone(), clamp01(confidence));
    }
    Ok(result)
}

/// Score one message across the bot-detection battery.
async fn evaluate_bot(client: &TypeSafeClient, state: serde_json::Value) -> Result<TurnScores, String> {
    let response = client.system_one(state, assets::bot_questions()).await?;
    let mut result = TurnScores::default();
    result.usage = Usage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
    };
    for dimension in &assets().bot_dimensions {
        let answer = response
            .answers
            .get(&dimension.id)
            .ok_or_else(|| format!("TypeSafe returned no answer for {}.", dimension.id))?;
        if answer.answer_type != "score" {
            return Err(format!(
                "TypeSafe returned \"{}\" for {}, expected \"score\".",
                answer.answer_type, dimension.id
            ));
        }
        let score = answer.score.ok_or_else(|| format!("missing score for {}", dimension.id))?;
        let confidence = answer.confidence.unwrap_or(0.0);
        result.scores.insert(dimension.id.clone(), clamp01(score / assets().bot_rubric_max));
        result.confidence.insert(dimension.id.clone(), clamp01(confidence));
    }
    Ok(result)
}

/// Score one message across the profile dimensions plus the safety battery.
async fn evaluate_guard(client: &TypeSafeClient, state: serde_json::Value) -> Result<TurnScores, String> {
    let response = client.system_one(state, assets::guard_questions()).await?;
    let mut result = TurnScores::default();
    result.usage = Usage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
    };
    for dimension in &assets().dimensions {
        let answer = response
            .answers
            .get(&dimension.id)
            .ok_or_else(|| format!("TypeSafe returned no answer for {}.", dimension.id))?;
        if answer.answer_type != "score" {
            return Err(format!(
                "TypeSafe returned \"{}\" for {}, expected \"score\".",
                answer.answer_type, dimension.id
            ));
        }
        let score = answer.score.ok_or_else(|| format!("missing score for {}", dimension.id))?;
        let confidence = answer.confidence.unwrap_or(0.0);
        result.scores.insert(dimension.id.clone(), clamp01(score / assets().rubric_max));
        result.confidence.insert(dimension.id.clone(), clamp01(confidence));
    }
    let self_harm = response
        .answers
        .get("self_harm")
        .ok_or_else(|| "TypeSafe returned no self-harm answer.".to_string())?;
    if self_harm.answer_type != "noul" {
        return Err(format!("TypeSafe returned \"{}\" for self_harm, expected \"noul\".", self_harm.answer_type));
    }
    result.self_harm = clamp01(self_harm.noul.unwrap_or(0.0));
    let severity = response
        .answers
        .get("severity")
        .ok_or_else(|| "TypeSafe returned no severity answer.".to_string())?;
    if severity.answer_type != "score" {
        return Err(format!("TypeSafe returned \"{}\" for severity, expected \"score\".", severity.answer_type));
    }
    result.severity = clamp01(severity.score.unwrap_or(0.0) / assets().guard_severity_max);
    Ok(result)
}

/// Run `f` over `items` with at most `limit` in flight, preserving order.
/// Returns the per-item scores (or `None` where that item failed) together with
/// the failure messages, so a caller can tell a partial run from a total one.
async fn map_concurrent<T, F, Fut>(
    items: Vec<T>,
    limit: usize,
    f: F,
) -> (Vec<Option<TurnScores>>, Vec<String>)
where
    T: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<TurnScores, String>> + Send + 'static,
{
    let limit = limit.clamp(1, items.len().max(1));
    let results: Vec<(usize, Result<TurnScores, String>)> = stream::iter(items.into_iter().enumerate().map(|(i, item)| {
        let fut = f(item);
        async move { (i, fut.await) }
    }))
    .buffer_unordered(limit)
    .collect::<Vec<_>>()
    .await;

    let mut ordered: Vec<Option<TurnScores>> = vec![None; results.len()];
    let mut errors: Vec<String> = Vec::new();
    for (i, r) in results {
        match r {
            Ok(scores) => ordered[i] = Some(scores),
            Err(error) => errors.push(error),
        }
    }
    (ordered, errors)
}

/// Reject a run in which every message failed. Without this a total outage
/// aggregates to all-zero scores, which reads as a clean `pass` report.
fn reject_total_failure(
    results: &[Option<TurnScores>],
    errors: &[String],
    total: usize,
) -> Result<(), String> {
    if total == 0 || !results.iter().all(|r| r.is_none()) {
        return Ok(());
    }
    match errors.first() {
        Some(first) => Err(format!("All {total} message(s) failed to score: {first}")),
        None => Err(format!("All {total} message(s) failed to score.")),
    }
}

fn sum_usage(results: &[Option<TurnScores>]) -> Usage {
    let mut usage = Usage::default();
    for result in results.iter().flatten() {
        usage.input_tokens += result.usage.input_tokens;
        usage.output_tokens += result.usage.output_tokens;
    }
    usage
}

// ---------------------------------------------------------------------------
// Report types (serialized to the frontend)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceExcerpt {
    pub index: usize,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerMessageScore {
    pub index: usize,
    pub score: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimensionSummary {
    pub id: String,
    pub label: String,
    pub category: String,
    pub group: String,
    pub polarity: String,
    pub direction: String,
    pub basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub score: f64,
    pub stddev: f64,
    pub confidence: f64,
    pub n: usize,
    pub stable: bool,
    pub evidence: Vec<EvidenceExcerpt>,
    pub messages: Vec<PerMessageScore>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileReport {
    pub source: String,
    pub author: Option<String>,
    pub message_count: usize,
    pub scored_count: usize,
    pub generated_at: String,
    pub dimensions: Vec<DimensionSummary>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotDimensionSummary {
    pub id: String,
    pub label: String,
    pub category: String,
    pub group: String,
    pub polarity: String,
    pub direction: String,
    pub basis: String,
    pub weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub score: f64,
    pub stddev: f64,
    pub confidence: f64,
    pub n: usize,
    pub stable: bool,
    pub evidence: Vec<EvidenceExcerpt>,
    pub messages: Vec<PerMessageScore>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotProbe {
    pub dimension: String,
    pub label: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotReport {
    pub source: String,
    pub turn_count: usize,
    pub scored_count: usize,
    pub generated_at: String,
    pub likelihood: f64,
    pub verdict: String,
    pub dimensions: Vec<BotDimensionSummary>,
    pub probes: Vec<BotProbe>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuardHazard {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub value: f64,
    pub action: String,
    pub triggered: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuardReport {
    pub source: String,
    pub policy: String,
    pub action: String,
    pub severity: f64,
    pub hazards: Vec<GuardHazard>,
    pub flagged: Vec<String>,
    pub generated_at: String,
    pub usage: Usage,
}

// ---------------------------------------------------------------------------
// Aggregation (profile + bot)
// ---------------------------------------------------------------------------

/// Fold per-message scores into a profile, mirroring `aggregate` in
/// `src/aggregate.ts`.
pub fn aggregate_profile(
    source: &str,
    author: Option<&str>,
    contents: &[String],
    results: &[Option<TurnScores>],
) -> ProfileReport {
    let a = assets();
    let dimensions: Vec<DimensionSummary> = a
        .dimensions
        .iter()
        .map(|dimension| {
            let mut per_message: Vec<PerMessageScore> = Vec::new();
            for (i, result) in results.iter().enumerate() {
                let Some(result) = result else { continue };
                let Some(score) = result.scores.get(&dimension.id) else { continue };
                let confidence = result.confidence.get(&dimension.id).copied().unwrap_or(0.0);
                per_message.push(PerMessageScore {
                    index: i + 1,
                    score: *score,
                    confidence,
                });
            }
            let n = per_message.len();
            let sum: f64 = per_message.iter().map(|m| m.score).sum();
            let confidence_sum: f64 = per_message.iter().map(|m| m.confidence).sum();
            let mean = if n == 0 { 0.0 } else { sum / n as f64 };
            let variance = if n == 0 {
                0.0
            } else {
                per_message.iter().map(|m| (m.score - mean).powi(2)).sum::<f64>() / n as f64
            };
            DimensionSummary {
                id: dimension.id.clone(),
                label: dimension.label.clone(),
                category: dimension.category.clone(),
                group: dimension.group.clone(),
                polarity: dimension.polarity.clone(),
                direction: dimension.direction.clone(),
                basis: dimension.basis.clone(),
                note: dimension.note.clone(),
                score: round3(mean),
                stddev: round3(variance.sqrt()),
                confidence: round3(if n == 0 { 0.0 } else { confidence_sum / n as f64 }),
                n,
                stable: n >= a.min_stable_samples,
                evidence: build_profile_evidence(&dimension.id, &per_message, contents),
                messages: per_message,
            }
        })
        .collect();

    ProfileReport {
        source: source.to_string(),
        author: author.map(|s| s.to_string()),
        message_count: contents.len(),
        scored_count: results.iter().filter(|r| r.is_some()).count(),
        generated_at: now_iso(),
        dimensions,
        usage: sum_usage(results),
    }
}

/// Pick the highest- and lowest-scoring messages as evidence for a dimension.
fn build_profile_evidence(
    dimension_id: &str,
    per_message: &[PerMessageScore],
    contents: &[String],
) -> Vec<EvidenceExcerpt> {
    if dimension_id == "self_disclosure" || per_message.is_empty() {
        return Vec::new();
    }
    let mut ordered: Vec<&PerMessageScore> = per_message.iter().collect();
    ordered.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut picks: Vec<&PerMessageScore> = Vec::new();
    if let Some(first) = ordered.first() {
        picks.push(*first);
    }
    if let Some(last) = ordered.last() {
        picks.push(*last);
    }
    let mut seen = std::collections::HashSet::new();
    let mut evidence = Vec::new();
    for pick in picks {
        if seen.contains(&pick.index) {
            continue;
        }
        seen.insert(pick.index);
        let Some(content) = contents.get(pick.index.saturating_sub(1)) else { continue };
        evidence.push(EvidenceExcerpt {
            index: pick.index,
            excerpt: excerpt_of(content),
        });
    }
    evidence
}

/// Fold per-turn scores into a bot-detection report, mirroring
/// `aggregateBotReport` in `src/botdetect.ts`.
pub fn aggregate_bot(
    source: &str,
    contents: &[String],
    results: &[Option<TurnScores>],
) -> BotReport {
    let a = assets();
    let dimensions: Vec<BotDimensionSummary> = a
        .bot_dimensions
        .iter()
        .map(|dimension| {
            let mut per_message: Vec<PerMessageScore> = Vec::new();
            for (i, result) in results.iter().enumerate() {
                let Some(result) = result else { continue };
                let Some(score) = result.scores.get(&dimension.id) else { continue };
                let confidence = result.confidence.get(&dimension.id).copied().unwrap_or(0.0);
                per_message.push(PerMessageScore {
                    index: i + 1,
                    score: *score,
                    confidence,
                });
            }
            let n = per_message.len();
            let sum: f64 = per_message.iter().map(|m| m.score).sum();
            let confidence_sum: f64 = per_message.iter().map(|m| m.confidence).sum();
            let mean = if n == 0 { 0.0 } else { sum / n as f64 };
            let variance = if n == 0 {
                0.0
            } else {
                per_message.iter().map(|m| (m.score - mean).powi(2)).sum::<f64>() / n as f64
            };
            BotDimensionSummary {
                id: dimension.id.clone(),
                label: dimension.label.clone(),
                category: dimension.category.clone(),
                group: dimension.group.clone(),
                polarity: dimension.polarity.clone(),
                direction: dimension.direction.clone(),
                basis: dimension.basis.clone(),
                weight: dimension.weight,
                note: dimension.note.clone(),
                score: round3(mean),
                stddev: round3(variance.sqrt()),
                confidence: round3(if n == 0 { 0.0 } else { confidence_sum / n as f64 }),
                n,
                stable: n >= a.min_stable_turns,
                evidence: build_bot_evidence(&per_message, contents),
                messages: per_message,
            }
        })
        .collect();

    let weight_sum: f64 = a.bot_dimensions.iter().map(|d| d.weight).sum();
    let weighted_sum: f64 = dimensions.iter().map(|d| d.score * d.weight).sum();
    let likelihood = if weight_sum == 0.0 { 0.0 } else { round3(weighted_sum / weight_sum) };

    let probes: Vec<BotProbe> = a
        .bot_dimensions
        .iter()
        .map(|d| BotProbe {
            dimension: d.id.clone(),
            label: d.label.clone(),
            items: d.probes.clone(),
        })
        .collect();

    BotReport {
        source: source.to_string(),
        turn_count: contents.len(),
        scored_count: results.iter().filter(|r| r.is_some()).count(),
        generated_at: now_iso(),
        likelihood,
        verdict: verdict_for(likelihood).to_string(),
        dimensions,
        probes,
        usage: sum_usage(results),
    }
}

fn build_bot_evidence(per_message: &[PerMessageScore], contents: &[String]) -> Vec<EvidenceExcerpt> {
    let Some(top) = per_message.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)) else {
        return Vec::new();
    };
    let Some(content) = contents.get(top.index.saturating_sub(1)) else { return Vec::new() };
    vec![EvidenceExcerpt {
        index: top.index,
        excerpt: excerpt_of(content),
    }]
}

fn verdict_for(likelihood: f64) -> &'static str {
    let a = assets();
    if likelihood < a.bot_verdict_human {
        "likely-human"
    } else if likelihood > a.bot_verdict_machine {
        "likely-machine"
    } else {
        "inconclusive"
    }
}

// ---------------------------------------------------------------------------
// Guardrails routing
// ---------------------------------------------------------------------------

fn resolve_policy(name: &str) -> Result<&assets::PolicyDef, String> {
    let a = assets();
    a.guard_policies.get(name).ok_or_else(|| {
        format!(
            "Unknown guard policy \"{name}\"; expected one of: {}.",
            a.guard_policies.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })
}

fn hazard_triggered(probability: f64, action: &RiskAction, severity: f64, policy: &assets::PolicyDef) -> Option<RiskAction> {
    if probability >= policy.action_threshold {
        return Some(if *action == RiskAction::Review && severity >= policy.severity_block {
            RiskAction::Block
        } else {
            action.clone()
        });
    }
    if probability >= policy.review_threshold {
        return Some(if severity >= policy.severity_block { RiskAction::Block } else { RiskAction::Review });
    }
    None
}

fn route(
    probabilities: &HashMap<String, f64>,
    action_for: &HashMap<String, String>,
    severity: f64,
    policy: &assets::PolicyDef,
) -> RiskAction {
    let mut triggered = std::collections::HashSet::new();
    for (hazard, action_str) in action_for {
        if let Some(probability) = probabilities.get(hazard) {
            let action = RiskAction::from_str(action_str);
            if let Some(decision) = hazard_triggered(*probability, &action, severity, policy) {
                triggered.insert(decision);
            }
        }
    }
    for action in &assets().precedence {
        let candidate = RiskAction::from_str(action);
        if triggered.contains(&candidate) {
            return candidate;
        }
    }
    RiskAction::Pass
}

/// Build hazard probabilities from a profile, mirroring `buildHazards`.
fn build_hazards(profile: &ProfileReport) -> HashMap<String, f64> {
    let a = assets();
    let mut hazards: HashMap<String, f64> = a
        .hazards
        .iter()
        .map(|h| (h.id.clone(), 0.0))
        .collect();
    for dimension in &profile.dimensions {
        if dimension.polarity == "inverted-risk" {
            hazards.insert(dimension.id.clone(), dimension.score);
        }
    }
    let scored: Vec<&DimensionSummary> = profile.dimensions.iter().filter(|d| d.n > 0).collect();
    let unstable = scored.iter().any(|d| !d.stable);
    hazards.insert("unstable".to_string(), if unstable { 1.0 } else { 0.0 });
    let confidence_mean = if scored.is_empty() {
        0.0
    } else {
        scored.iter().map(|d| d.confidence).sum::<f64>() / scored.len() as f64
    };
    let low_confidence = !scored.is_empty() && confidence_mean < a.low_confidence_threshold;
    hazards.insert("low_confidence".to_string(), if low_confidence { 1.0 } else { 0.0 });
    hazards
}

/// Assemble a routing report from a profile and safety screen, mirroring
/// `guardProfile`.
pub fn guard_profile(
    profile: &ProfileReport,
    self_harm_max: f64,
    severity_max: f64,
    policy_name: &str,
) -> Result<GuardReport, String> {
    let a = assets();
    let policy = resolve_policy(policy_name)?;
    let severity = clamp01(severity_max);
    let mut probabilities = build_hazards(profile);
    probabilities.insert("self_harm".to_string(), clamp01(self_harm_max));
    let action = route(&probabilities, &a.profile_hazard_action, severity, policy);

    let hazards: Vec<GuardHazard> = a
        .hazards
        .iter()
        .map(|def| {
            let value = probabilities.get(&def.id).copied().unwrap_or(0.0);
            let mapped = RiskAction::from_str(&def.action);
            GuardHazard {
                id: def.id.clone(),
                label: def.label.clone(),
                kind: def.kind.clone(),
                value,
                action: def.action.clone(),
                triggered: hazard_triggered(value, &mapped, severity, policy).map(|a| action_name(&a)),
            }
        })
        .collect();
    let flagged = hazards
        .iter()
        .filter(|h| h.triggered.is_some())
        .map(|h| h.id.clone())
        .collect();

    Ok(GuardReport {
        source: profile.source.clone(),
        policy: policy_name.to_string(),
        action: action_name(&action),
        severity,
        hazards,
        flagged,
        generated_at: now_iso(),
        usage: profile.usage.clone(),
    })
}

fn action_name(action: &RiskAction) -> String {
    match action {
        RiskAction::Pass => "pass".to_string(),
        RiskAction::Review => "review".to_string(),
        RiskAction::Block => "block".to_string(),
        RiskAction::Support => "support".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Message screen (guard_message)
// ---------------------------------------------------------------------------

const MESSAGE_HAZARD_LABELS: &[(&str, &str)] = &[
    ("jailbreak", "Jailbreak"),
    ("broke_policy", "Broke policy"),
    ("harmful_request", "Harmful request"),
    ("medical_advice", "Medical advice"),
    ("self_harm", "Self-harm"),
];

/// Screen a single message (LLM input or output) with the message battery,
/// mirroring `guard_message` in the guardrails cookbook. `dry_run` scores the
/// message locally, like the profile/bot/guard runs, so screening needs no key.
pub async fn screen_message(
    client: &TypeSafeClient,
    side: &str,
    content: &str,
    policy_name: &str,
    dry_run: bool,
) -> Result<GuardReport, String> {
    let a = assets();
    let policy = resolve_policy(policy_name)?;

    let batteries = a
        .message_batteries
        .as_object()
        .ok_or_else(|| "message batteries missing".to_string())?;
    let battery = batteries
        .get(side)
        .ok_or_else(|| format!("unknown side \"{side}\"; expected \"input\" or \"output\""))?
        .as_object()
        .ok_or_else(|| format!("side \"{side}\" battery is not an object"))?
        .clone();

    let hazard_ids = a.message_hazards.get(side).cloned().unwrap_or_default();

    let (probabilities, severity, usage) = if dry_run {
        let mut probabilities: HashMap<String, f64> = HashMap::new();
        for id in &hazard_ids {
            if id == "severity" {
                continue;
            }
            probabilities
                .insert(id.clone(), (fnv1a(&format!("{id}\u{0}{content}")) % 1000) as f64 / 1000.0);
        }
        let severity = (fnv1a(&format!("severity\u{0}{content}")) % 1000) as f64 / 1000.0;
        (
            probabilities,
            clamp01(severity),
            Usage {
                input_tokens: content.chars().count() as u64,
                output_tokens: hazard_ids.len() as u64,
            },
        )
    } else {
        let state = build_state(content, None, None);
        let response = client.system_one(state, battery).await?;
        let mut probabilities: HashMap<String, f64> = HashMap::new();
        let mut severity = 0.0;
        for (id, answer) in &response.answers {
            if id == "severity" {
                severity = clamp01(answer.score.unwrap_or(0.0) / a.guard_severity_max);
            } else if answer.answer_type == "noul" {
                probabilities.insert(id.clone(), clamp01(answer.noul.unwrap_or(0.0)));
            }
        }
        (
            probabilities,
            severity,
            Usage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
            },
        )
    };

    let action = route(&probabilities, &a.message_hazard_action, severity, policy);

    let mut hazards: Vec<GuardHazard> = Vec::new();
    for id in &hazard_ids {
        if id == "severity" {
            continue;
        }
        let value = probabilities.get(id).copied().unwrap_or(0.0);
        let mapped_str = a.message_hazard_action.get(id).cloned().unwrap_or_else(|| "review".to_string());
        let mapped = RiskAction::from_str(&mapped_str);
        let label = MESSAGE_HAZARD_LABELS
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, l)| (*l).to_string())
            .unwrap_or_else(|| id.clone());
        hazards.push(GuardHazard {
            id: id.clone(),
            label,
            kind: if id == "self_harm" { "safety".to_string() } else { "message".to_string() },
            value,
            action: mapped_str,
            triggered: hazard_triggered(value, &mapped, severity, policy).map(|a| action_name(&a)),
        });
    }

    let flagged = hazards
        .iter()
        .filter(|h| h.triggered.is_some())
        .map(|h| h.id.clone())
        .collect();

    Ok(GuardReport {
        source: format!("message ({side})"),
        policy: policy_name.to_string(),
        action: action_name(&action),
        severity,
        hazards,
        flagged,
        generated_at: now_iso(),
        usage,
    })
}

// ---------------------------------------------------------------------------
// Shared evaluation entry points used by commands
// ---------------------------------------------------------------------------

/// Evaluate many message contents across the profile dimensions and aggregate.
pub async fn run_profile(
    client: &TypeSafeClient,
    source: &str,
    author: Option<&str>,
    contents: &[String],
    timestamps: &[Option<String>],
    channels: &[Option<String>],
    concurrency: usize,
    dry_run: bool,
) -> Result<ProfileReport, String> {
    let ids: Vec<&str> = assets().dimensions.iter().map(|d| d.id.as_str()).collect();
    let (results, errors) = if dry_run {
        (dry_run_all(contents, &ids, None, None), Vec::new())
    } else {
        let states = build_states(contents, timestamps, channels);
        let client = client.clone();
        map_concurrent(states, concurrency, move |state| {
            let client = client.clone();
            async move { evaluate_profile(&client, state).await }
        })
        .await
    };
    reject_total_failure(&results, &errors, contents.len())?;
    Ok(aggregate_profile(source, author, contents, &results))
}

/// Evaluate many message contents across the bot battery and aggregate.
pub async fn run_bot(
    client: &TypeSafeClient,
    source: &str,
    contents: &[String],
    timestamps: &[Option<String>],
    channels: &[Option<String>],
    concurrency: usize,
    dry_run: bool,
) -> Result<BotReport, String> {
    let ids: Vec<&str> = assets().bot_dimensions.iter().map(|d| d.id.as_str()).collect();
    let (results, errors) = if dry_run {
        (dry_run_all(contents, &ids, None, None), Vec::new())
    } else {
        let states = build_states(contents, timestamps, channels);
        let client = client.clone();
        map_concurrent(states, concurrency, move |state| {
            let client = client.clone();
            async move { evaluate_bot(&client, state).await }
        })
        .await
    };
    reject_total_failure(&results, &errors, contents.len())?;
    Ok(aggregate_bot(source, contents, &results))
}

/// Evaluate many message contents across the profile + safety battery, then
/// route to a guard decision.
pub async fn run_guard(
    client: &TypeSafeClient,
    source: &str,
    author: Option<&str>,
    contents: &[String],
    timestamps: &[Option<String>],
    channels: &[Option<String>],
    concurrency: usize,
    policy: &str,
    dry_run: bool,
) -> Result<GuardReport, String> {
    let ids: Vec<&str> = assets().dimensions.iter().map(|d| d.id.as_str()).collect();
    let (results, errors): (Vec<Option<TurnScores>>, Vec<String>) = if dry_run {
        let ids_owned: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        let dry: Vec<Option<TurnScores>> = contents
            .iter()
            .map(|content| {
                let (scores, confidence) = dry_run_scores(&ids, content);
                Some(TurnScores {
                    scores,
                    confidence,
                    self_harm: (fnv1a(&format!("self_harm\u{0}{content}")) % 1000) as f64 / 1000.0,
                    severity: (fnv1a(&format!("severity\u{0}{content}")) % 1000) as f64 / 1000.0,
                    usage: Usage {
                        input_tokens: content.chars().count() as u64,
                        output_tokens: (ids_owned.len() + 2) as u64,
                    },
                })
            })
            .collect();
        (dry, Vec::new())
    } else {
        let states = build_states(contents, timestamps, channels);
        let client = client.clone();
        map_concurrent(states, concurrency, move |state| {
            let client = client.clone();
            async move { evaluate_guard(&client, state).await }
        })
        .await
    };
    reject_total_failure(&results, &errors, contents.len())?;

    let profile = aggregate_profile(source, author, contents, &results);
    let mut self_harm_max: f64 = 0.0;
    let mut severity_max: f64 = 0.0;
    for result in results.iter().flatten() {
        self_harm_max = self_harm_max.max(result.self_harm);
        severity_max = severity_max.max(result.severity);
    }
    guard_profile(&profile, self_harm_max, severity_max, policy)
}

/// Build per-message `state` values aligned with `contents`.
fn build_states(
    contents: &[String],
    timestamps: &[Option<String>],
    channels: &[Option<String>],
) -> Vec<serde_json::Value> {
    contents
        .iter()
        .enumerate()
        .map(|(i, content)| {
            build_state(
                content,
                timestamps.get(i).and_then(|t| t.as_deref()),
                channels.get(i).and_then(|c| c.as_deref()),
            )
        })
        .collect()
}

/// Produce dry-run results for a dimension-id list (no self-harm/severity).
fn dry_run_all(
    contents: &[String],
    ids: &[&str],
    _self_harm: Option<()>,
    _severity: Option<()>,
) -> Vec<Option<TurnScores>> {
    contents
        .iter()
        .map(|content| {
            let (scores, confidence) = dry_run_scores(ids, content);
            Some(TurnScores {
                scores,
                confidence,
                self_harm: 0.0,
                severity: 0.0,
                usage: Usage {
                    input_tokens: content.chars().count() as u64,
                    output_tokens: ids.len() as u64,
                },
            })
        })
        .collect()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(review: f64, action: f64, block: f64) -> crate::assets::PolicyDef {
        crate::assets::PolicyDef {
            review_threshold: review,
            action_threshold: action,
            severity_block: block,
        }
    }

    #[test]
    fn a_run_where_every_message_failed_is_rejected() {
        // Without this, an outage aggregates to all-zero scores and routes to
        // `pass`, presenting a total failure as a clean report.
        let results: Vec<Option<TurnScores>> = vec![None, None];
        let errors = vec!["401 Unauthorized".to_string()];
        let error = reject_total_failure(&results, &errors, 2).unwrap_err();
        assert!(error.contains("401 Unauthorized"), "{error}");
        assert!(error.contains('2'), "{error}");
    }

    #[test]
    fn partial_and_complete_runs_are_accepted() {
        let scored = Some(TurnScores::default());
        assert!(reject_total_failure(&[scored.clone(), None], &["boom".into()], 2).is_ok());
        assert!(reject_total_failure(&[scored.clone(), scored], &[], 2).is_ok());
        // No messages at all is a different error, raised by the callers.
        assert!(reject_total_failure(&[], &[], 0).is_ok());
    }

    #[test]
    fn hazard_below_review_passes() {
        let p = policy(0.5, 0.7, 0.7);
        assert_eq!(hazard_triggered(0.4, &RiskAction::Block, 0.0, &p), None);
    }

    #[test]
    fn hazard_at_review_routes_review() {
        let p = policy(0.5, 0.7, 0.7);
        assert_eq!(hazard_triggered(0.5, &RiskAction::Block, 0.0, &p), Some(RiskAction::Review));
    }

    #[test]
    fn hazard_at_action_triggers_action() {
        let p = policy(0.5, 0.7, 0.7);
        assert_eq!(hazard_triggered(0.8, &RiskAction::Block, 0.0, &p), Some(RiskAction::Block));
    }

    #[test]
    fn severity_promotes_review_to_block() {
        let p = policy(0.5, 0.7, 0.7);
        assert_eq!(hazard_triggered(0.5, &RiskAction::Review, 0.8, &p), Some(RiskAction::Block));
    }

    #[test]
    fn precedence_support_wins() {
        let p = policy(0.5, 0.7, 0.7);
        let mut probs = HashMap::new();
        probs.insert("self_harm".to_string(), 0.9);
        probs.insert("frustration_hostility".to_string(), 0.9);
        let mut action_for = HashMap::new();
        action_for.insert("self_harm".to_string(), "support".to_string());
        action_for.insert("frustration_hostility".to_string(), "block".to_string());
        assert_eq!(route(&probs, &action_for, 0.0, &p), RiskAction::Support);
    }

    #[test]
    fn route_defaults_to_pass() {
        let p = policy(0.5, 0.7, 0.7);
        assert_eq!(route(&HashMap::new(), &HashMap::new(), 0.0, &p), RiskAction::Pass);
    }

    #[test]
    fn verdict_thresholds() {
        assert_eq!(verdict_for(0.1), "likely-human");
        assert_eq!(verdict_for(0.5), "inconclusive");
        assert_eq!(verdict_for(0.9), "likely-machine");
    }

    #[test]
    fn excerpt_scrubs_pii() {
        let e = excerpt_of("email bob@example.com or call 555-123-4567, ping @ann at http://x.com");
        assert!(e.contains("[email]"));
        assert!(e.contains("[phone]"));
        assert!(e.contains("[link]"));
        assert!(e.contains("@handle"));
        assert!(!e.contains("bob@example.com"));
        assert!(!e.contains("555-123-4567"));
    }

    fn dim(id: &str, polarity: &str, score: f64, confidence: f64, n: usize, stable: bool) -> DimensionSummary {
        DimensionSummary {
            id: id.to_string(),
            label: id.to_string(),
            category: "x".to_string(),
            group: "x".to_string(),
            polarity: polarity.to_string(),
            direction: "high = more".to_string(),
            basis: "text".to_string(),
            note: None,
            score,
            stddev: 0.0,
            confidence,
            n,
            stable,
            evidence: vec![],
            messages: vec![],
        }
    }

    fn profile(dims: Vec<DimensionSummary>) -> ProfileReport {
        ProfileReport {
            source: "test".to_string(),
            author: None,
            message_count: 3,
            scored_count: 3,
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            dimensions: dims,
            usage: Usage::default(),
        }
    }

    #[test]
    fn build_hazards_maps_inverted_risk_and_meta() {
        let hazards = build_hazards(&profile(vec![
            dim("frustration_hostility", "inverted-risk", 0.9, 0.8, 5, true),
            dim("clarity", "desirable", 0.7, 0.8, 5, true),
            dim("escalation_readiness", "inverted-risk", 0.6, 0.8, 2, false),
        ]));
        assert_eq!(hazards["frustration_hostility"], 0.9);
        assert_eq!(hazards["escalation_readiness"], 0.6);
        // one unstable dimension => unstable meta-hazard fires
        assert_eq!(hazards["unstable"], 1.0);
        assert_eq!(hazards["low_confidence"], 0.0);
    }

    #[test]
    fn dry_run_aggregate_yields_all_asset_dimensions() {
        let ids: Vec<&str> = crate::assets::assets().dimensions.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids.len(), 35);
        let contents = vec![
            "hello world".to_string(),
            "another message".to_string(),
            "third message".to_string(),
        ];
        let results: Vec<Option<TurnScores>> = contents
            .iter()
            .map(|content| {
                let (scores, confidence) = dry_run_scores(&ids, content);
                Some(TurnScores {
                    scores,
                    confidence,
                    self_harm: 0.0,
                    severity: 0.0,
                    usage: Usage::default(),
                })
            })
            .collect();
        let report = aggregate_profile("test", Some("alice"), &contents, &results);
        assert_eq!(report.dimensions.len(), 35);
        assert_eq!(report.scored_count, 3);
        for d in &report.dimensions {
            assert_eq!(d.n, 3);
            assert!(d.stable);
        }
    }
}
