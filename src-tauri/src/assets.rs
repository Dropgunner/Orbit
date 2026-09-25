//! Loads the generated `profiler.json` asset (emitted by
//! `scripts/gen-orbit-assets.ts` from the TypeScript `src/` pipeline) into a
//! typed, static structure. The asset is the single source of truth for the
//! model-visible questions, rubrics, directions, polarities, bot battery, and
//! guardrails policies; the Rust backend sends these exact questions to TypeSafe.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// One human-profile scoring dimension plus its TypeSafe `score` question.
#[derive(Debug, Clone, Deserialize)]
pub struct DimensionDef {
    pub id: String,
    pub category: String,
    pub group: String,
    pub label: String,
    pub polarity: String,
    pub direction: String,
    pub basis: String,
    #[serde(default)]
    pub note: Option<String>,
    /// The raw TypeSafe question (`{ "type": "score", "instructions", "criteria" }`).
    pub question: serde_json::Value,
}

/// One bot-detection dimension plus its weight, probes, and `score` question.
#[derive(Debug, Clone, Deserialize)]
pub struct BotDimensionDef {
    pub id: String,
    pub category: String,
    pub group: String,
    pub label: String,
    pub polarity: String,
    pub direction: String,
    pub basis: String,
    pub weight: f64,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub probes: Vec<String>,
    pub question: serde_json::Value,
}

/// A guardrails hazard definition.
#[derive(Debug, Clone, Deserialize)]
pub struct HazardDef {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub action: String,
}

/// A named routing policy (thresholds).
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyDef {
    #[serde(rename = "reviewThreshold")]
    pub review_threshold: f64,
    #[serde(rename = "actionThreshold")]
    pub action_threshold: f64,
    #[serde(rename = "severityBlock")]
    pub severity_block: f64,
}

/// The parsed asset, available process-wide once loaded.
#[derive(Debug)]
pub struct ProfilerAssets {
    pub rubric_max: f64,
    pub bot_rubric_max: f64,
    pub guard_severity_max: f64,
    pub excerpt_max: usize,
    pub min_stable_samples: usize,
    pub min_stable_turns: usize,
    pub bot_verdict_human: f64,
    pub bot_verdict_machine: f64,
    pub low_confidence_threshold: f64,
    pub max_pattern_examples: usize,
    pub context_chars: usize,
    pub max_buckets: usize,
    pub bucket_spans_ms: Vec<u64>,
    pub dimensions: Vec<DimensionDef>,
    pub bot_dimensions: Vec<BotDimensionDef>,
    pub hazards: Vec<HazardDef>,
    pub profile_hazard_action: HashMap<String, String>,
    pub precedence: Vec<String>,
    pub guard_policies: HashMap<String, PolicyDef>,
    pub default_guard_policy: String,
    pub guard_safety_questions: serde_json::Value,
    pub message_hazards: HashMap<String, Vec<String>>,
    pub message_hazard_action: HashMap<String, String>,
    pub message_batteries: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct RawAssets {
    #[serde(rename = "rubricMax")]
    rubric_max: f64,
    #[serde(rename = "botRubricMax")]
    bot_rubric_max: f64,
    #[serde(rename = "guardSeverityMax")]
    guard_severity_max: f64,
    #[serde(rename = "excerptMax")]
    excerpt_max: usize,
    #[serde(rename = "minStableSamples")]
    min_stable_samples: usize,
    #[serde(rename = "minStableTurns")]
    min_stable_turns: usize,
    #[serde(rename = "botVerdictHuman")]
    bot_verdict_human: f64,
    #[serde(rename = "botVerdictMachine")]
    bot_verdict_machine: f64,
    #[serde(rename = "lowConfidenceThreshold")]
    low_confidence_threshold: f64,
    #[serde(rename = "maxPatternExamples")]
    max_pattern_examples: usize,
    #[serde(rename = "contextChars")]
    context_chars: usize,
    #[serde(rename = "maxBuckets")]
    max_buckets: usize,
    #[serde(rename = "bucketSpansMs")]
    bucket_spans_ms: Vec<u64>,
    dimensions: Vec<DimensionDef>,
    #[serde(rename = "botDimensions")]
    bot_dimensions: Vec<BotDimensionDef>,
    hazards: Vec<HazardDef>,
    #[serde(rename = "profileHazardAction")]
    profile_hazard_action: HashMap<String, String>,
    precedence: Vec<String>,
    #[serde(rename = "guardPolicies")]
    guard_policies: HashMap<String, PolicyDef>,
    #[serde(rename = "defaultGuardPolicy")]
    default_guard_policy: String,
    #[serde(rename = "guardSafetyQuestions")]
    guard_safety_questions: serde_json::Value,
    #[serde(rename = "messageHazards")]
    message_hazards: HashMap<String, Vec<String>>,
    #[serde(rename = "messageHazardAction")]
    message_hazard_action: HashMap<String, String>,
    #[serde(rename = "messageBatteries")]
    message_batteries: serde_json::Value,
}

static ASSETS: OnceLock<ProfilerAssets> = OnceLock::new();

/// Parse and return the static assets. Panics only if the embedded asset is
/// malformed (a build-time invariant), which fails loud rather than silently
/// degrading the analysis.
pub fn assets() -> &'static ProfilerAssets {
    ASSETS.get_or_init(|| {
        let raw: RawAssets = serde_json::from_str(include_str!("../assets/profiler.json"))
            .expect("embedded profiler.json must parse");
        ProfilerAssets {
            rubric_max: raw.rubric_max,
            bot_rubric_max: raw.bot_rubric_max,
            guard_severity_max: raw.guard_severity_max,
            excerpt_max: raw.excerpt_max,
            min_stable_samples: raw.min_stable_samples,
            min_stable_turns: raw.min_stable_turns,
            bot_verdict_human: raw.bot_verdict_human,
            bot_verdict_machine: raw.bot_verdict_machine,
            low_confidence_threshold: raw.low_confidence_threshold,
            max_pattern_examples: raw.max_pattern_examples,
            context_chars: raw.context_chars,
            max_buckets: raw.max_buckets,
            bucket_spans_ms: raw.bucket_spans_ms,
            dimensions: raw.dimensions,
            bot_dimensions: raw.bot_dimensions,
            hazards: raw.hazards,
            profile_hazard_action: raw.profile_hazard_action,
            precedence: raw.precedence,
            guard_policies: raw.guard_policies,
            default_guard_policy: raw.default_guard_policy,
            guard_safety_questions: raw.guard_safety_questions,
            message_hazards: raw.message_hazards,
            message_hazard_action: raw.message_hazard_action,
            message_batteries: raw.message_batteries,
        }
    })
}

/// The profile `score` questions keyed by dimension id (the `QUESTIONS` map).
pub fn profile_questions() -> serde_json::Map<String, serde_json::Value> {
    assets()
        .dimensions
        .iter()
        .map(|d| (d.id.clone(), d.question.clone()))
        .collect()
}

/// The bot `score` questions keyed by dimension id (the `BOT_QUESTIONS` map).
pub fn bot_questions() -> serde_json::Map<String, serde_json::Value> {
    assets()
        .bot_dimensions
        .iter()
        .map(|d| (d.id.clone(), d.question.clone()))
        .collect()
}

/// The profile questions plus the self-harm/severity safety battery
/// (the `GUARD_QUESTIONS` map).
pub fn guard_questions() -> serde_json::Map<String, serde_json::Value> {
    let mut questions = profile_questions();
    if let Some(safety) = assets().guard_safety_questions.as_object() {
        for (key, value) in safety {
            questions.insert(key.clone(), value.clone());
        }
    }
    questions
}
