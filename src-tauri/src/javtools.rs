//! The seven generic Jev (TypeSafe) toolkit tools, ported from the harness
//! bundle `plugin/index.ts`: classification, rubric scoring, single-message
//! psychological profiling, a yes/no gate, output pruning, transcript
//! compaction, and greedy next-token generation. Each tool sends one or more
//! `POST /v1/systemone` calls through the shared `TypeSafeClient` and validates
//! the wire answers, mirroring the TypeScript implementation exactly.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::assets::assets;
use crate::profiler::{clamp01, RawAnswer, SystemOneResponse, TypeSafeClient, Usage};

/// Maximum caller-supplied criteria (labels or rubric levels), matching the
/// harness bundle's `maxCriteria` default.
const MAX_CRITERIA: usize = 20;
const PRUNE_MAX_CHUNKS: usize = 200;
const PRUNE_MAX_LINE_CHARS: usize = 2000;
const PRUNE_DEFAULT_CHUNK_LINES: usize = 20;
const PRUNE_KEEP_THRESHOLD: f64 = 0.5;
const COMPACT_PRESERVE_RECENT: usize = 6;
const COMPACT_KEEP_THRESHOLD: f64 = 0.5;
const COMPACT_STATE_TEXT_CHARS: usize = 1000;
const COMPACT_TRUNCATE_HEAD_CHARS: usize = 300;
const GENERATE_DEFAULT_TOKENS: usize = 10;
const GENERATE_MAX_TOKENS_CAP: usize = 50;

// ---------------------------------------------------------------------------
// Question builders (mirror the TypeSafe SDK `noul` / `score` / `choice`)
// ---------------------------------------------------------------------------

fn noul_plain(instructions: &str) -> serde_json::Value {
    serde_json::json!({ "type": "noul", "instructions": instructions })
}

fn noul_with_criteria(instructions: &str, yes: Option<&str>, no: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "type": "noul",
        "instructions": instructions,
        "criteria": { "true": yes, "false": no }
    })
}

fn score_question(instructions: &str, rubric: &[String]) -> serde_json::Value {
    serde_json::json!({ "type": "score", "instructions": instructions, "criteria": rubric })
}

fn choice_question(instructions: &str, labels: &[String]) -> serde_json::Value {
    let criteria: serde_json::Map<String, serde_json::Value> = labels
        .iter()
        .map(|label| (label.clone(), serde_json::Value::Null))
        .collect();
    serde_json::json!({ "type": "choice", "instructions": instructions, "criteria": criteria })
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

fn answer<'a>(response: &'a SystemOneResponse, key: &str) -> Result<&'a RawAnswer, String> {
    response
        .answers
        .get(key)
        .ok_or_else(|| format!("TypeSafe returned no answer for {key}."))
}

fn usage_of(response: &SystemOneResponse) -> Usage {
    Usage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
    }
}

/// The model the response actually used, falling back to the configured model.
fn model_of(client: &TypeSafeClient, response: &SystemOneResponse) -> String {
    if response.model.is_empty() {
        client.model_name().to_string()
    } else {
        response.model.clone()
    }
}

fn nonempty(value: &str, what: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{what} must not be empty."));
    }
    Ok(())
}

fn probability(value: Option<f64>, key: &str) -> Result<f64, String> {
    let value = value.ok_or_else(|| format!("TypeSafe returned no {key}."))?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(format!("TypeSafe returned an invalid {key}."));
    }
    Ok(value)
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        value.chars().take(max).collect()
    }
}

// ---------------------------------------------------------------------------
// typesafe_classify
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ClassifyProbability {
    pub label: String,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassifyResult {
    pub model: String,
    pub usage: Usage,
    pub choice: String,
    pub confidence: f64,
    pub probabilities: Vec<ClassifyProbability>,
}

/// Choose one of `labels` for `text`, with reported probabilities.
pub async fn classify(
    client: &TypeSafeClient,
    text: &str,
    instructions: &str,
    labels: &[String],
) -> Result<ClassifyResult, String> {
    nonempty(text, "Text")?;
    nonempty(instructions, "Instructions")?;
    if labels.len() < 2 || labels.len() > MAX_CRITERIA {
        return Err(format!("Provide 2 to {MAX_CRITERIA} distinct labels."));
    }
    let mut seen = HashSet::new();
    for label in labels {
        nonempty(label, "Labels")?;
        if !seen.insert(label) {
            return Err("Classification labels must be distinct.".to_string());
        }
    }

    let questions: serde_json::Map<String, serde_json::Value> =
        [("result".to_string(), choice_question(instructions, labels))]
            .into_iter()
            .collect();
    let response = client
        .system_one(serde_json::Value::String(text.to_string()), questions)
        .await?;
    let result = answer(&response, "result")?;
    if result.answer_type != "choice" {
        return Err(format!(
            "TypeSafe returned \"{}\" for result, expected \"choice\".",
            result.answer_type
        ));
    }
    let choice = result
        .choice
        .as_deref()
        .ok_or_else(|| "TypeSafe returned no choice.".to_string())?;
    if !labels.iter().any(|label| label == choice) {
        return Err("TypeSafe returned an unknown label.".to_string());
    }
    let probabilities = result
        .probabilities
        .as_ref()
        .ok_or_else(|| "TypeSafe returned no probabilities.".to_string())?;
    if probabilities.len() != labels.len() || labels.iter().any(|label| !probabilities.contains_key(label)) {
        return Err("TypeSafe returned probabilities for unexpected criteria.".to_string());
    }

    Ok(ClassifyResult {
        model: model_of(client, &response),
        usage: usage_of(&response),
        choice: choice.to_string(),
        confidence: clamp01(result.confidence.unwrap_or(0.0)),
        probabilities: labels
            .iter()
            .map(|label| ClassifyProbability {
                label: label.clone(),
                probability: clamp01(*probabilities.get(label).unwrap()),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// typesafe_score
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ScoreProbability {
    pub level: usize,
    pub description: String,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreResult {
    pub model: String,
    pub usage: Usage,
    pub score: f64,
    pub confidence: f64,
    pub probabilities: Vec<ScoreProbability>,
}

/// Score `text` against an ordered rubric, returning the expected (fractional)
/// level and per-level probabilities.
pub async fn score_text(
    client: &TypeSafeClient,
    text: &str,
    instructions: &str,
    rubric: &[String],
) -> Result<ScoreResult, String> {
    nonempty(text, "Text")?;
    nonempty(instructions, "Instructions")?;
    if rubric.len() < 2 || rubric.len() > MAX_CRITERIA {
        return Err(format!("Provide 2 to {MAX_CRITERIA} rubric levels."));
    }
    for level in rubric {
        nonempty(level, "Rubric descriptions")?;
    }

    let questions: serde_json::Map<String, serde_json::Value> =
        [("result".to_string(), score_question(instructions, rubric))]
            .into_iter()
            .collect();
    let response = client
        .system_one(serde_json::Value::String(text.to_string()), questions)
        .await?;
    let result = answer(&response, "result")?;
    if result.answer_type != "score" {
        return Err(format!(
            "TypeSafe returned \"{}\" for result, expected \"score\".",
            result.answer_type
        ));
    }
    let score = result
        .score
        .ok_or_else(|| "TypeSafe returned no score.".to_string())?;
    let max = (rubric.len() - 1) as f64;
    if !score.is_finite() || !(0.0..=max).contains(&score) {
        return Err("TypeSafe returned a score outside the rubric range.".to_string());
    }
    let probabilities = result
        .probabilities
        .as_ref()
        .ok_or_else(|| "TypeSafe returned no probabilities.".to_string())?;
    let expected: HashSet<String> = (0..rubric.len()).map(|i| i.to_string()).collect();
    if probabilities.len() != rubric.len()
        || probabilities.keys().any(|key| !expected.contains(key))
    {
        return Err("TypeSafe returned probabilities for unexpected levels.".to_string());
    }

    Ok(ScoreResult {
        model: model_of(client, &response),
        usage: usage_of(&response),
        score,
        confidence: clamp01(result.confidence.unwrap_or(0.0)),
        probabilities: rubric
            .iter()
            .enumerate()
            .map(|(level, description)| ScoreProbability {
                level,
                description: description.clone(),
                probability: clamp01(*probabilities.get(&level.to_string()).unwrap()),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// psych_profile (single message)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PsychDimension {
    pub id: String,
    pub category: String,
    pub label: String,
    pub polarity: String,
    pub direction: String,
    pub score: f64,
    pub confidence: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PsychProfileResult {
    pub model: String,
    pub usage: Usage,
    pub dimensions: Vec<PsychDimension>,
}

/// Score one message across all profile dimensions in a single call.
pub async fn psych_profile(client: &TypeSafeClient, text: &str) -> Result<PsychProfileResult, String> {
    nonempty(text, "Text")?;
    let response = client
        .system_one(
            serde_json::Value::String(text.to_string()),
            crate::assets::profile_questions(),
        )
        .await?;
    let rubric_max = assets().rubric_max;
    let mut dimensions = Vec::with_capacity(assets().dimensions.len());
    for dimension in &assets().dimensions {
        let result = answer(&response, &dimension.id)?;
        if result.answer_type != "score" {
            return Err(format!(
                "TypeSafe returned \"{}\" for {}, expected \"score\".",
                result.answer_type, dimension.id
            ));
        }
        let score = result
            .score
            .ok_or_else(|| format!("TypeSafe returned no score for {}.", dimension.id))?;
        dimensions.push(PsychDimension {
            id: dimension.id.clone(),
            category: dimension.category.clone(),
            label: dimension.label.clone(),
            polarity: dimension.polarity.clone(),
            direction: dimension.direction.clone(),
            score: clamp01(score / rubric_max),
            confidence: clamp01(result.confidence.unwrap_or(0.0)),
            note: dimension.note.clone().unwrap_or_default(),
        });
    }
    Ok(PsychProfileResult {
        model: model_of(client, &response),
        usage: usage_of(&response),
        dimensions,
    })
}

// ---------------------------------------------------------------------------
// typesafe_noul
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct NoulResult {
    pub model: String,
    pub usage: Usage,
    pub yes_probability: f64,
}

/// Answer a yes/no question and return the probability of a yes.
pub async fn noul_gate(
    client: &TypeSafeClient,
    text: &str,
    instructions: &str,
    yes: Option<&str>,
    no: Option<&str>,
) -> Result<NoulResult, String> {
    nonempty(text, "Text")?;
    nonempty(instructions, "Instructions")?;
    let questions: serde_json::Map<String, serde_json::Value> =
        [("result".to_string(), noul_with_criteria(instructions, yes, no))]
            .into_iter()
            .collect();
    let response = client
        .system_one(serde_json::Value::String(text.to_string()), questions)
        .await?;
    let result = answer(&response, "result")?;
    if result.answer_type != "noul" {
        return Err(format!(
            "TypeSafe returned \"{}\" for result, expected \"noul\".",
            result.answer_type
        ));
    }
    Ok(NoulResult {
        model: model_of(client, &response),
        usage: usage_of(&response),
        yes_probability: probability(result.noul, "noul probability")?,
    })
}

// ---------------------------------------------------------------------------
// typesafe_prune
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PruneResult {
    pub model: String,
    pub usage: Usage,
    pub pruned: String,
    pub kept_chunks: usize,
    pub dropped_chunks: usize,
    pub omitted_lines: usize,
}

struct Chunk {
    text: String,
    lines: usize,
}

/// Split over-long lines, then group into chunks of at most `size` lines.
fn split_chunks(text: &str, size: usize) -> Vec<Chunk> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.chars().count() <= PRUNE_MAX_LINE_CHARS {
            lines.push(line.to_string());
        } else {
            for piece in line.chars().collect::<Vec<_>>().chunks(PRUNE_MAX_LINE_CHARS) {
                lines.push(piece.iter().collect());
            }
        }
    }
    let mut chunks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let end = (i + size).min(lines.len());
        chunks.push(Chunk {
            text: lines[i..end].join("\n"),
            lines: end - i,
        });
        i += size;
    }
    chunks
}

/// Ask, per chunk, whether any line must stay; drop the rest, keeping the
/// first and last chunks unconditionally.
pub async fn prune(
    client: &TypeSafeClient,
    text: &str,
    goal: Option<&str>,
    chunk_lines: usize,
    keep_threshold: f64,
) -> Result<PruneResult, String> {
    nonempty(text, "Text")?;
    if chunk_lines == 0 || chunk_lines > 200 {
        return Err("chunk_lines must be an integer between 1 and 200.".to_string());
    }
    if !keep_threshold.is_finite() || !(0.0..=1.0).contains(&keep_threshold) {
        return Err("keep_threshold must be between 0 and 1.".to_string());
    }
    let chunks = split_chunks(text, chunk_lines);
    if chunks.is_empty() {
        return Err("Nothing to prune.".to_string());
    }
    if chunks.len() == 1 {
        return Ok(PruneResult {
            model: client.model_name().to_string(),
            usage: Usage::default(),
            pruned: text.to_string(),
            kept_chunks: 1,
            dropped_chunks: 0,
            omitted_lines: 0,
        });
    }
    if chunks.len() > PRUNE_MAX_CHUNKS {
        return Err(format!(
            "Output splits into {} chunks; the maximum is {PRUNE_MAX_CHUNKS}.",
            chunks.len()
        ));
    }

    let questions: serde_json::Map<String, serde_json::Value> = (0..chunks.len())
        .map(|i| {
            (
                format!("chunk_{i}"),
                noul_plain(&format!("Does any line in chunk {i} need to remain available?")),
            )
        })
        .collect();
    let state = serde_json::json!({
        "goal": goal,
        "chunks": chunks.iter().map(|c| c.text.clone()).collect::<Vec<_>>(),
    });
    let response = client.system_one(state, questions).await?;

    let mut keep = Vec::with_capacity(chunks.len());
    for (i, _) in chunks.iter().enumerate() {
        let result = answer(&response, &format!("chunk_{i}"))?;
        if result.answer_type != "noul" {
            return Err(format!(
                "TypeSafe returned \"{}\" for chunk_{i}, expected \"noul\".",
                result.answer_type
            ));
        }
        let probability = clamp01(result.noul.unwrap_or(0.0));
        keep.push(probability >= keep_threshold || i == 0 || i == chunks.len() - 1);
    }

    let mut out: Vec<String> = Vec::new();
    let mut omitted = 0usize;
    let mut total_omitted = 0usize;
    let mut kept = 0usize;
    let mut dropped = 0usize;
    for (i, chunk) in chunks.iter().enumerate() {
        if keep[i] {
            if omitted > 0 {
                out.push(format!("[{omitted} lines omitted]"));
                total_omitted += omitted;
                omitted = 0;
            }
            out.push(chunk.text.clone());
            kept += 1;
        } else {
            omitted += chunk.lines;
            dropped += 1;
        }
    }
    if omitted > 0 {
        out.push(format!("[{omitted} lines omitted]"));
        total_omitted += omitted;
    }

    Ok(PruneResult {
        model: model_of(client, &response),
        usage: usage_of(&response),
        pruned: out.join("\n"),
        kept_chunks: kept,
        dropped_chunks: dropped,
        omitted_lines: total_omitted,
    })
}

// ---------------------------------------------------------------------------
// typesafe_compact
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CompactEntry {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactDecision {
    pub id: String,
    pub keep_call: bool,
    pub keep_result: bool,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactStats {
    pub before: usize,
    pub after: usize,
    pub dropped: usize,
    pub truncated: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactResult {
    pub model: String,
    pub usage: Usage,
    pub entries: Vec<CompactEntry>,
    pub decisions: Vec<CompactDecision>,
    pub stats: CompactStats,
}

/// Decide, per tool call/result pair, whether it still needs to stay; drop or
/// truncate stale results while keeping everything else verbatim.
pub async fn compact(
    client: &TypeSafeClient,
    entries: &[CompactEntry],
    goal: Option<&str>,
    preserve_recent: usize,
    keep_threshold: f64,
) -> Result<CompactResult, String> {
    if entries.is_empty() {
        return Err("Provide at least one transcript entry.".to_string());
    }
    let mut ids = HashSet::new();
    for entry in entries {
        if entry.id.trim().is_empty() {
            return Err("Every entry needs a non-empty id.".to_string());
        }
        if !ids.insert(entry.id.clone()) {
            return Err(format!("Duplicate entry id \"{}\".", entry.id));
        }
        if entry.text.trim().is_empty() {
            return Err(format!("Entry \"{}\" needs non-empty text.", entry.id));
        }
    }
    if !keep_threshold.is_finite() || !(0.0..=1.0).contains(&keep_threshold) {
        return Err("keep_threshold must be between 0 and 1.".to_string());
    }

    let tool_entries: Vec<&CompactEntry> = entries.iter().filter(|e| e.result.is_some()).collect();
    let mut decisions_out: Vec<CompactDecision> = Vec::new();
    if tool_entries.is_empty() {
        return Ok(CompactResult {
            model: client.model_name().to_string(),
            usage: Usage::default(),
            entries: entries.to_vec(),
            decisions: decisions_out,
            stats: CompactStats {
                before: entries.len(),
                after: entries.len(),
                dropped: 0,
                truncated: 0,
            },
        });
    }

    let mut pinned: HashSet<String> = HashSet::new();
    pinned.insert(entries[0].id.clone());
    let start = entries.len().saturating_sub(preserve_recent).max(1);
    for entry in &entries[start..] {
        pinned.insert(entry.id.clone());
    }
    let candidates: Vec<&CompactEntry> = tool_entries
        .into_iter()
        .filter(|entry| !pinned.contains(&entry.id))
        .collect();

    let mut questions: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for entry in &candidates {
        questions.insert(
            format!("call_{}", entry.id),
            noul_plain("Does knowing this tool call was made, with its input, still matter?"),
        );
        questions.insert(
            format!("result_{}", entry.id),
            noul_plain("Is this tool result still needed verbatim; would re-running the tool not do?"),
        );
    }

    let state_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            let mut item = serde_json::json!({
                "id": entry.id,
                "text": truncate_chars(&entry.text, COMPACT_STATE_TEXT_CHARS),
            });
            if let Some(result) = &entry.result {
                item["result_note"] =
                    serde_json::json!(format!("ok, {} chars (omitted)", result.chars().count()));
            }
            item
        })
        .collect();
    let state = serde_json::json!({ "goal": goal, "entries": state_entries });
    let response = client.system_one(state, questions).await?;

    let mut decisions: HashMap<String, (&str, bool, bool)> = HashMap::new();
    for entry in &candidates {
        let call_result = answer(&response, &format!("call_{}", entry.id))?;
        let result_result = answer(&response, &format!("result_{}", entry.id))?;
        if call_result.answer_type != "noul" || result_result.answer_type != "noul" {
            return Err("TypeSafe returned the wrong answer type for a compaction decision.".to_string());
        }
        let keep_call = clamp01(call_result.noul.unwrap_or(0.0)) >= keep_threshold;
        let keep_result = clamp01(result_result.noul.unwrap_or(0.0)) >= keep_threshold;
        let action = if keep_result {
            "keep"
        } else if keep_call {
            "truncate"
        } else {
            "drop"
        };
        decisions.insert(entry.id.clone(), (action, keep_call, keep_result));
        decisions_out.push(CompactDecision {
            id: entry.id.clone(),
            keep_call,
            keep_result,
            action: action.to_string(),
        });
    }

    let mut rebuilt: Vec<CompactEntry> = Vec::new();
    let mut truncated = 0usize;
    for entry in entries {
        if entry.result.is_none() || pinned.contains(&entry.id) {
            rebuilt.push(entry.clone());
            continue;
        }
        let Some((action, _, _)) = decisions.get(&entry.id) else {
            rebuilt.push(entry.clone());
            continue;
        };
        match *action {
            "keep" => rebuilt.push(entry.clone()),
            "truncate" => {
                truncated += 1;
                let full = entry.result.as_deref().unwrap_or("");
                let head = truncate_chars(full, COMPACT_TRUNCATE_HEAD_CHARS);
                let omitted = full.chars().count().saturating_sub(head.chars().count());
                let result = if omitted > 0 {
                    format!("{head} … [{omitted} chars omitted]")
                } else {
                    head
                };
                rebuilt.push(CompactEntry {
                    id: entry.id.clone(),
                    text: entry.text.clone(),
                    result: Some(result),
                });
            }
            _ => {}
        }
    }

    let before = entries.len();
    let after = rebuilt.len();
    Ok(CompactResult {
        model: model_of(client, &response),
        usage: usage_of(&response),
        entries: rebuilt,
        decisions: decisions_out,
        stats: CompactStats {
            before,
            after,
            dropped: before - after,
            truncated,
        },
    })
}

// ---------------------------------------------------------------------------
// typesafe_generate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GenerateResult {
    pub model: String,
    pub usage: Usage,
    pub text: String,
    pub tokens: Vec<String>,
}

/// Greedily pick the next token from a candidate set, one choice per step.
pub async fn generate(
    client: &TypeSafeClient,
    prompt: &str,
    candidates: &[String],
    max_tokens: usize,
    stop: &[String],
) -> Result<GenerateResult, String> {
    nonempty(prompt, "Prompt")?;
    if candidates.len() < 2 {
        return Err("Provide at least two candidate tokens.".to_string());
    }
    let mut seen = HashSet::new();
    for candidate in candidates {
        nonempty(candidate, "Candidate tokens")?;
        if !seen.insert(candidate) {
            return Err("Candidate tokens must be distinct.".to_string());
        }
    }
    let max_tokens = max_tokens.min(GENERATE_MAX_TOKENS_CAP);
    if max_tokens == 0 {
        return Err("max_tokens must be a positive integer.".to_string());
    }
    let stop: HashSet<&String> = stop.iter().collect();

    let mut text_so_far = prompt.to_string();
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut model = client.model_name().to_string();
    let mut tokens: Vec<String> = Vec::new();

    for _ in 0..max_tokens {
        let state = serde_json::json!({ "prompt": prompt, "so_far": text_so_far });
        let questions: serde_json::Map<String, serde_json::Value> = [(
            "next".to_string(),
            choice_question("Which token best continues this text?", candidates),
        )]
        .into_iter()
        .collect();
        let response = client.system_one(state, questions).await?;
        if !response.model.is_empty() {
            model = response.model.clone();
        }
        input_tokens += response.usage.input_tokens;
        output_tokens += response.usage.output_tokens;
        let result = answer(&response, "next")?;
        if result.answer_type != "choice" {
            return Err(format!(
                "TypeSafe returned \"{}\" for next, expected \"choice\".",
                result.answer_type
            ));
        }
        let selected = result
            .choice
            .as_deref()
            .ok_or_else(|| "TypeSafe returned no candidate.".to_string())?;
        if !candidates.iter().any(|candidate| candidate == selected) {
            return Err("TypeSafe returned an unknown candidate.".to_string());
        }
        tokens.push(selected.to_string());
        text_so_far = if text_so_far.is_empty() {
            selected.to_string()
        } else {
            format!("{text_so_far} {selected}")
        };
        if stop.contains(&selected.to_string()) {
            break;
        }
    }

    Ok(GenerateResult {
        model,
        usage: Usage {
            input_tokens,
            output_tokens,
        },
        text: text_so_far,
        tokens,
    })
}

/// Defaults exposed to the command layer so callers can pass `None` cleanly.
pub fn default_chunk_lines() -> usize {
    PRUNE_DEFAULT_CHUNK_LINES
}

pub fn default_keep_threshold() -> f64 {
    PRUNE_KEEP_THRESHOLD
}

pub fn default_compact_keep_threshold() -> f64 {
    COMPACT_KEEP_THRESHOLD
}

pub fn default_preserve_recent() -> usize {
    COMPACT_PRESERVE_RECENT
}

pub fn default_max_tokens() -> usize {
    GENERATE_DEFAULT_TOKENS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_chunks_groups_lines_and_counts_them() {
        let chunks = split_chunks("a\nb\nc\nd\ne", 2);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "a\nb");
        assert_eq!(chunks[0].lines, 2);
        assert_eq!(chunks[2].text, "e");
        assert_eq!(chunks[2].lines, 1);
    }

    #[test]
    fn split_chunks_splits_overlong_lines() {
        let long = "x".repeat(PRUNE_MAX_LINE_CHARS + 10);
        let chunks = split_chunks(&long, 100);
        // One 2010-char line becomes two lines (2000 + 10), grouped into one chunk.
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].lines, 2);
        assert_eq!(chunks[0].text.chars().count(), PRUNE_MAX_LINE_CHARS + 1 + 10);
    }

    #[test]
    fn truncate_chars_respects_char_boundaries() {
        assert_eq!(truncate_chars("hello", 3), "hel");
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn question_builders_emit_the_sdk_shapes() {
        let noul = noul_plain("is it?");
        assert_eq!(noul["type"], "noul");
        assert!(noul.get("criteria").is_none());

        let noul_c = noul_with_criteria("is it?", Some("yes"), None);
        assert_eq!(noul_c["criteria"]["true"], "yes");
        assert!(noul_c["criteria"]["false"].is_null());

        let score = score_question("how much?", &["low".to_string(), "high".to_string()]);
        assert_eq!(score["type"], "score");
        assert_eq!(score["criteria"].as_array().unwrap().len(), 2);

        let choice = choice_question("which?", &["a".to_string(), "b".to_string()]);
        assert_eq!(choice["type"], "choice");
        assert!(choice["criteria"]["a"].is_null());
    }
}
