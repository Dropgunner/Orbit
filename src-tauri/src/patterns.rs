//! Pattern flagging and matching, ported from `src/patterns.ts`. A "flag" is a
//! typed, reusable signature (literal substring, whole word, or regex) the user
//! observed in a bot. Matching is local and deterministic; no TypeSafe call.

use serde::{Deserialize, Serialize};

use crate::assets::assets;
use crate::profiler::excerpt_of;

/// How a pattern matches a message's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatternKind {
    Regex,
    Substring,
    Word,
}

/// A normalized, stored pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotPattern {
    pub id: String,
    pub label: String,
    pub creator: String,
    pub category: String,
    pub kind: PatternKind,
    pub pattern: String,
    #[serde(rename = "regexFlags")]
    pub regex_flags: String,
    #[serde(rename = "caseSensitive")]
    pub case_sensitive: bool,
    pub note: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

/// What the user supplies when flagging a pattern; id and timestamps are filled in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternInput {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    pub creator: String,
    pub category: String,
    pub kind: PatternKind,
    pub pattern: String,
    #[serde(default, rename = "regexFlags")]
    pub regex_flags: Option<String>,
    #[serde(default, rename = "caseSensitive")]
    pub case_sensitive: Option<bool>,
    #[serde(default)]
    pub note: Option<String>,
}

/// One message for matching (index is 1-based, mirroring the CLI).
#[derive(Debug, Clone)]
pub struct MessageInput {
    pub index: usize,
    pub content: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternExample {
    pub index: usize,
    pub timestamp: Option<String>,
    #[serde(rename = "match")]
    pub match_text: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternMatchSummary {
    #[serde(rename = "patternId")]
    pub pattern_id: String,
    pub label: String,
    pub creator: String,
    pub category: String,
    pub kind: PatternKind,
    pub pattern: String,
    pub matches: usize,
    #[serde(rename = "firstIndex")]
    pub first_index: Option<usize>,
    #[serde(rename = "lastIndex")]
    pub last_index: Option<usize>,
    pub examples: Vec<PatternExample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeBucket {
    pub start: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GapStats {
    #[serde(rename = "matchCount")]
    pub match_count: usize,
    #[serde(rename = "bucketSpanMs")]
    pub bucket_span_ms: u64,
    #[serde(rename = "meanGapMs")]
    pub mean_gap_ms: i64,
    #[serde(rename = "medianGapMs")]
    pub median_gap_ms: i64,
    #[serde(rename = "minGapMs")]
    pub min_gap_ms: i64,
    #[serde(rename = "maxGapMs")]
    pub max_gap_ms: i64,
    #[serde(rename = "stddevGapMs")]
    pub stddev_gap_ms: i64,
    #[serde(rename = "coefficientOfVariation")]
    pub coefficient_of_variation: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternMatchReport {
    pub source: String,
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    #[serde(rename = "messageCount")]
    pub message_count: usize,
    #[serde(rename = "patternsScanned")]
    pub patterns_scanned: usize,
    #[serde(rename = "matchedPatterns")]
    pub matched_patterns: usize,
    #[serde(rename = "matchedMessages")]
    pub matched_messages: usize,
    #[serde(rename = "totalMatches")]
    pub total_matches: usize,
    #[serde(rename = "matchedRate")]
    pub matched_rate: f64,
    pub patterns: Vec<PatternMatchSummary>,
    #[serde(rename = "timeSeries")]
    pub time_series: Vec<TimeBucket>,
    #[serde(rename = "gapStats")]
    pub gap_stats: Option<GapStats>,
}

fn fail(message: String) -> String {
    format!("Pattern validation failed: {message}")
}

fn is_non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

/// Derive a stable machine id from a label, or "pattern" when it is empty.
fn slug_for(label: &str) -> String {
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = slug.trim_matches('-').to_string();
    let base = if trimmed.is_empty() { "pattern" } else { &trimmed };
    let sliced: String = base.chars().take(64).collect();
    sliced.trim_end_matches('-').to_string()
}

fn id_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[a-z0-9][a-z0-9_-]{0,63}$").expect("id regex"))
}

fn compile_pattern_regex(pattern: &str, flags: &str) -> Result<regex::Regex, String> {
    let mut builder = regex::RegexBuilder::new(pattern);
    for flag in flags.chars() {
        match flag {
            'i' => {
                builder.case_insensitive(true);
            }
            'm' => {
                builder.multi_line(true);
            }
            's' => {
                builder.dot_matches_new_line(true);
            }
            // g/y are stateful in JS and stripped there; u is unicode (on by
            // default in the regex crate); d is the indices flag, not relevant.
            'g' | 'y' | 'u' | 'd' => {}
            other => {
                return Err(fail(format!(
                    "pattern \"{pattern}\" has unsupported flag \"{other}\""
                )));
            }
        }
    }
    builder.build().map_err(|e| fail(format!("pattern \"{pattern}\" with flags \"{flags}\" is not a valid regular expression: {e}")))
}

/// Validate one pattern and return an error string on the first violation.
pub fn validate_pattern(pattern: &BotPattern) -> Result<(), String> {
    if !id_re().is_match(&pattern.id) {
        return Err(fail(format!("id \"{}\" is not a valid key (lowercase letters, digits, '_' or '-').", pattern.id)));
    }
    if !is_non_empty(&pattern.label) {
        return Err(fail(format!("pattern \"{}\" is missing its label.", pattern.id)));
    }
    if !is_non_empty(&pattern.creator) {
        return Err(fail(format!("pattern \"{}\" is missing its creator.", pattern.id)));
    }
    if !is_non_empty(&pattern.category) {
        return Err(fail(format!("pattern \"{}\" is missing its category.", pattern.id)));
    }
    if !is_non_empty(&pattern.pattern) {
        return Err(fail(format!("pattern \"{}\" is missing its pattern text.", pattern.id)));
    }
    if !is_non_empty(&pattern.created_at) {
        return Err(fail(format!("pattern \"{}\" is missing createdAt.", pattern.id)));
    }
    if !is_non_empty(&pattern.updated_at) {
        return Err(fail(format!("pattern \"{}\" is missing updatedAt.", pattern.id)));
    }
    if pattern.kind == PatternKind::Regex {
        compile_pattern_regex(&pattern.pattern, &pattern.regex_flags)?;
    }
    Ok(())
}

/// Normalize a flagged pattern into a stored record.
pub fn normalize_pattern(input: PatternInput, now: &str) -> Result<BotPattern, String> {
    let pattern = BotPattern {
        id: input.id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| slug_for(&input.label)),
        label: input.label,
        creator: input.creator,
        category: input.category,
        kind: input.kind,
        pattern: input.pattern,
        regex_flags: input.regex_flags.unwrap_or_default(),
        case_sensitive: input.case_sensitive.unwrap_or(false),
        note: input.note.unwrap_or_default(),
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };
    validate_pattern(&pattern)?;
    Ok(pattern)
}

enum Matcher {
    Regex(regex::Regex),
    Substring { needle: String, case_sensitive: bool },
    Word(regex::Regex),
}

impl Matcher {
    fn find(&self, content: &str) -> Option<(usize, usize, String)> {
        match self {
            Matcher::Regex(re) => re.find(content).map(|m| (m.start(), m.len(), m.as_str().to_string())),
            Matcher::Substring { needle, case_sensitive } => {
                if *case_sensitive {
                    content.find(needle).map(|i| (i, needle.len(), content[i..i + needle.len()].to_string()))
                } else {
                    let haystack = content.to_lowercase();
                    let needle_l = needle.to_lowercase();
                    haystack.find(&needle_l).map(|i| (i, needle_l.len(), content[i..i + needle_l.len()].to_string()))
                }
            }
            Matcher::Word(re) => re.find(content).map(|m| (m.start(), m.len(), m.as_str().to_string())),
        }
    }
}

fn compile_matcher(pattern: &BotPattern) -> Result<Matcher, String> {
    match pattern.kind {
        PatternKind::Regex => {
            let flags = pattern.regex_flags.replace(['g', 'y'], "");
            Ok(Matcher::Regex(compile_pattern_regex(&pattern.pattern, &flags)?))
        }
        PatternKind::Word => {
            let escaped = regex::escape(&pattern.pattern);
            let flags = if pattern.case_sensitive { "" } else { "(?i)" };
            let re = regex::Regex::new(&format!(r"{flags}\b{escaped}\b"))
                .map_err(|e| fail(e.to_string()))?;
            Ok(Matcher::Word(re))
        }
        PatternKind::Substring => Ok(Matcher::Substring {
            needle: pattern.pattern.clone(),
            case_sensitive: pattern.case_sensitive,
        }),
    }
}

fn build_window(content: &str, index: usize, length: usize) -> String {
    let context = assets().context_chars;
    let start = index.saturating_sub(context);
    let end = (index + length + context).min(content.len());
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < content.len() { "…" } else { "" };
    format!("{prefix}{}{suffix}", &content[start..end])
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.timestamp_millis());
    }
    let spaced = regex::Regex::new(r"^(\d{4}-\d{2}-\d{2})\s+(\d{2}:\d{2}(?::\d{2})?(?:\.\d+)?)$").ok()?;
    if let Some(caps) = spaced.captures(trimmed) {
        let joined = format!("{}T{}", &caps[1], &caps[2]);
        for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&joined, fmt) {
                return Some(naive.and_utc().timestamp_millis());
            }
        }
    }
    None
}

fn choose_bucket_span(min_ms: i64, max_ms: i64) -> u64 {
    let span = max_ms - min_ms;
    for candidate in &assets().bucket_spans_ms {
        if span as f64 / *candidate as f64 <= assets().max_buckets as f64 {
            return *candidate;
        }
    }
    *assets().bucket_spans_ms.last().unwrap_or(&86_400_000)
}

fn median(values: &[i64]) -> i64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2
    }
}

fn build_gap_stats(ts_ms: &[i64], bucket_span_ms: u64) -> Option<GapStats> {
    if ts_ms.len() < 2 {
        return None;
    }
    let mut sorted = ts_ms.to_vec();
    sorted.sort_unstable();
    let mut gaps: Vec<i64> = Vec::with_capacity(sorted.len() - 1);
    for i in 1..sorted.len() {
        gaps.push(sorted[i] - sorted[i - 1]);
    }
    let mean = gaps.iter().sum::<i64>() as f64 / gaps.len() as f64;
    let variance = gaps.iter().map(|g| (*g as f64 - mean).powi(2)).sum::<f64>() / gaps.len() as f64;
    let stddev = variance.sqrt();
    Some(GapStats {
        match_count: sorted.len(),
        bucket_span_ms,
        mean_gap_ms: mean.round() as i64,
        median_gap_ms: median(&gaps),
        min_gap_ms: *gaps.iter().min().unwrap(),
        max_gap_ms: *gaps.iter().max().unwrap(),
        stddev_gap_ms: stddev.round() as i64,
        coefficient_of_variation: if mean == 0.0 { 0.0 } else { round3(stddev / mean) },
    })
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Scan a transcript against a set of flagged patterns and fold the matches
/// into a report (per-pattern totals, examples, time series, gap statistics).
pub fn build_pattern_report(
    source: &str,
    patterns: &[BotPattern],
    messages: &[MessageInput],
) -> Result<PatternMatchReport, String> {
    for pattern in patterns {
        validate_pattern(pattern)?;
    }

    let by_index: std::collections::HashMap<usize, &MessageInput> = messages.iter().map(|m| (m.index, m)).collect();

    let mut total_matches = 0usize;
    let mut matched_indexes = std::collections::HashSet::new();
    let mut summaries: Vec<PatternMatchSummary> = Vec::with_capacity(patterns.len());
    let max_examples = assets().max_pattern_examples;

    for pattern in patterns {
        let matcher = compile_matcher(pattern)?;
        let mut matches: Vec<(usize, usize, usize, String)> = Vec::new();
        for message in messages {
            if let Some((index, length, text)) = matcher.find(&message.content) {
                matches.push((message.index, index, length, text));
            }
        }
        total_matches += matches.len();
        for m in &matches {
            matched_indexes.insert(m.0);
        }
        let first_index = matches.first().map(|m| m.0);
        let last_index = matches.last().map(|m| m.0);
        let examples: Vec<PatternExample> = matches
            .iter()
            .take(max_examples)
            .filter_map(|(message_index, index, length, text)| {
                let content = &by_index.get(message_index)?.content;
                Some(PatternExample {
                    index: *message_index,
                    timestamp: by_index.get(message_index).and_then(|m| m.timestamp.clone()),
                    match_text: excerpt_of(text),
                    excerpt: excerpt_of(&build_window(content, *index, *length)),
                })
            })
            .collect();
        summaries.push(PatternMatchSummary {
            pattern_id: pattern.id.clone(),
            label: pattern.label.clone(),
            creator: pattern.creator.clone(),
            category: pattern.category.clone(),
            kind: pattern.kind,
            pattern: pattern.pattern.clone(),
            matches: matches.len(),
            first_index,
            last_index,
            examples,
        });
    }

    let matched_patterns = summaries.iter().filter(|s| s.matches > 0).count();
    let matched_messages = matched_indexes.len();

    let mut ts_ms: Vec<i64> = Vec::new();
    for index in &matched_indexes {
        if let Some(message) = by_index.get(index) {
            if let Some(ts) = &message.timestamp {
                if let Some(ms) = parse_timestamp_ms(ts) {
                    ts_ms.push(ms);
                }
            }
        }
    }

    let mut time_series: Vec<TimeBucket> = Vec::new();
    let mut gap_stats: Option<GapStats> = None;
    if !ts_ms.is_empty() {
        let min_ms = *ts_ms.iter().min().unwrap();
        let max_ms = *ts_ms.iter().max().unwrap();
        let span = choose_bucket_span(min_ms, max_ms);
        let start = (min_ms / span as i64) * span as i64;
        let mut t = start;
        while t <= max_ms {
            let count = ts_ms.iter().filter(|&&ms| ms >= t && ms < t + span as i64).count();
            time_series.push(TimeBucket {
                start: chrono::DateTime::from_timestamp_millis(t)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| t.to_string()),
                count,
            });
            t += span as i64;
        }
        gap_stats = build_gap_stats(&ts_ms, span);
    }

    Ok(PatternMatchReport {
        source: source.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        message_count: messages.len(),
        patterns_scanned: patterns.len(),
        matched_patterns,
        matched_messages,
        total_matches,
        matched_rate: if messages.is_empty() { 0.0 } else { round3(matched_messages as f64 / messages.len() as f64) },
        patterns: summaries,
        time_series,
        gap_stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> String {
        "2026-01-01T00:00:00.000Z".to_string()
    }

    fn input(label: &str, kind: PatternKind, pattern: &str) -> PatternInput {
        PatternInput {
            id: None,
            label: label.to_string(),
            creator: "acme".to_string(),
            category: "linguistic".to_string(),
            kind,
            pattern: pattern.to_string(),
            regex_flags: None,
            case_sensitive: None,
            note: None,
        }
    }

    #[test]
    fn normalize_slugs_id_and_defaults() {
        let p = normalize_pattern(input("Canned Apology!", PatternKind::Substring, "i apologize"), &now()).unwrap();
        assert_eq!(p.id, "canned-apology");
        assert!(!p.case_sensitive);
        assert_eq!(p.regex_flags, "");
    }

    #[test]
    fn validate_rejects_bad_id() {
        let mut p = normalize_pattern(input("ok", PatternKind::Substring, "x"), &now()).unwrap();
        p.id = "Bad Id!".to_string();
        assert!(validate_pattern(&p).is_err());
    }

    #[test]
    fn validate_rejects_bad_regex() {
        let p = normalize_pattern(input("ok", PatternKind::Regex, "("), &now());
        assert!(p.is_err());
    }

    #[test]
    fn substring_match_is_case_insensitive_by_default() {
        let p = normalize_pattern(input("apology", PatternKind::Substring, "I APOLOGIZE"), &now()).unwrap();
        let matcher = compile_matcher(&p).unwrap();
        assert!(matcher.find("you are right, i apologize").is_some());
    }

    #[test]
    fn word_match_respects_boundaries() {
        let p = normalize_pattern(input("bot", PatternKind::Word, "bot"), &now()).unwrap();
        let matcher = compile_matcher(&p).unwrap();
        assert!(matcher.find("a bot here").is_some());
        assert!(matcher.find("robot").is_none());
    }

    #[test]
    fn report_counts_matches_and_scrubs() {
        let p = normalize_pattern(input("email", PatternKind::Regex, r"[a-z]+@example\.com"), &now()).unwrap();
        let messages = vec![
            MessageInput {
                index: 1,
                content: "contact alice@example.com please".to_string(),
                timestamp: Some("2026-01-01T10:00:00Z".to_string()),
            },
            MessageInput {
                index: 2,
                content: "nothing here".to_string(),
                timestamp: Some("2026-01-01T11:00:00Z".to_string()),
            },
        ];
        let report = build_pattern_report("test", &[p], &messages).unwrap();
        assert_eq!(report.matched_messages, 1);
        assert_eq!(report.patterns[0].matches, 1);
        let match_text = &report.patterns[0].examples[0].match_text;
        assert!(!match_text.contains("alice@example.com") || match_text.contains("[email]"));
    }

    #[test]
    fn gap_stats_builds_for_two_timestamped_matches() {
        let p = normalize_pattern(input("hi", PatternKind::Substring, "hi"), &now()).unwrap();
        let messages = vec![
            MessageInput {
                index: 1,
                content: "hi there".to_string(),
                timestamp: Some("2026-01-01T10:00:00Z".to_string()),
            },
            MessageInput {
                index: 2,
                content: "hi again".to_string(),
                timestamp: Some("2026-01-01T10:05:00Z".to_string()),
            },
        ];
        let report = build_pattern_report("test", &[p], &messages).unwrap();
        let gap = report.gap_stats.expect("gap stats for two timestamped matches");
        assert_eq!(gap.match_count, 2);
        assert_eq!(gap.mean_gap_ms, 5 * 60 * 1000);
    }
}
