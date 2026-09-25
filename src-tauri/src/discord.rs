//! Discord data import: parse DiscordChatExporter JSON (full fidelity — guild,
//! channels, users with bot flag, messages), plaintext logs (the same formats
//! the CLI accepts), and DiscordChatExporter HTML (best-effort). Produces a
//! normalized [`DiscordDataset`] the analysis commands operate on.

use serde::{Deserialize, Serialize};

/// A manual flag the user attaches to a Discord entity (user or message),
/// persisted to the app config dir. Complements the automatic guardrails routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityFlag {
    pub entity_type: String,
    pub entity_id: String,
    pub label: String,
    pub action: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordGuild {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordChannel {
    pub id: Option<String>,
    pub name: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordUser {
    pub id: String,
    pub name: String,
    pub discriminator: Option<String>,
    pub nickname: Option<String>,
    pub is_bot: bool,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordMessage {
    pub id: Option<String>,
    pub channel_id: Option<String>,
    pub author_id: String,
    pub timestamp: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct DiscordDataset {
    pub guild: DiscordGuild,
    pub channels: Vec<DiscordChannel>,
    pub users: Vec<DiscordUser>,
    pub messages: Vec<DiscordMessage>,
}

impl DiscordDataset {
    /// Messages for one author, in dataset order.
    pub fn messages_for(&self, author_id: &str) -> Vec<&DiscordMessage> {
        self.messages
            .iter()
            .filter(|m| m.author_id == author_id)
            .collect()
    }

    /// A user by id.
    pub fn user(&self, id: &str) -> Option<&DiscordUser> {
        self.users.iter().find(|u| u.id == id)
    }
}

/// Render the dataset as a DiscordChatExporter-compatible JSON value, so an
/// export round-trips through [`import`] and stays readable by other tools.
pub fn to_dce_json(dataset: &DiscordDataset) -> serde_json::Value {
    use serde_json::json;

    let channel = dataset.channels.first();
    let messages: Vec<serde_json::Value> = dataset
        .messages
        .iter()
        .map(|message| {
            let author = dataset.user(&message.author_id);
            json!({
                "id": &message.id,
                "type": "Default",
                "timestamp": &message.timestamp,
                "content": &message.content,
                "author": {
                    "id": &message.author_id,
                    "name": author.map(|u| u.name.as_str()).unwrap_or(&message.author_id),
                    "discriminator": author.and_then(|u| u.discriminator.as_deref()),
                    "nickname": author.and_then(|u| u.nickname.as_deref()),
                    "isBot": author.map(|u| u.is_bot).unwrap_or(false),
                },
            })
        })
        .collect();

    json!({
        "guild": { "id": &dataset.guild.id, "name": &dataset.guild.name },
        "channel": {
            "id": channel.and_then(|c| c.id.as_deref()),
            "name": channel.map(|c| c.name.as_str()).unwrap_or("messages"),
            "category": channel.and_then(|c| c.category.as_deref()),
        },
        "messages": messages,
        "messageCount": dataset.messages.len(),
        "exportedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "exportedBy": "Orbit Discord analysis",
    })
}

/// Auto-detect and parse an import (JSON, HTML, or plaintext).
pub fn import(content: &str, name_hint: &str) -> Result<DiscordDataset, String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return parse_json(content).or_else(|json_err| {
            // A paste may start with JSON but be malformed; fail loud with the
            // JSON error rather than silently mis-parsing as text.
            Err(json_err)
        });
    }
    if looks_like_html(trimmed) {
        return parse_html(content, name_hint);
    }
    parse_plaintext(content, name_hint)
}

fn looks_like_html(text: &str) -> bool {
    let head = text.get(..2000).unwrap_or(text).to_lowercase();
    head.contains("chatlog__") || head.contains("<html") || head.contains("<div") || head.contains("<body")
}

// ---------------------------------------------------------------------------
// JSON (DiscordChatExporter)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct DceExport {
    #[serde(default)]
    guild: Option<DceGuild>,
    #[serde(default)]
    channel: Option<DceChannel>,
    #[serde(default)]
    messages: Vec<DceMessage>,
}

#[derive(serde::Deserialize)]
struct DceGuild {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(serde::Deserialize)]
struct DceChannel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(serde::Deserialize)]
struct DceMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    author: Option<DceAuthor>,
}

#[derive(serde::Deserialize)]
struct DceAuthor {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    discriminator: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(rename = "isBot", default)]
    is_bot: Option<bool>,
}

fn parse_json(content: &str) -> Result<DiscordDataset, String> {
    let export: DceExport =
        serde_json::from_str(content).map_err(|e| format!("Not a valid DiscordChatExporter JSON export: {e}"))?;

    let guild_name = export
        .guild
        .as_ref()
        .and_then(|g| g.name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Discord server".to_string());
    let guild_id = export.guild.as_ref().and_then(|g| g.id.clone());

    let channel_name = export
        .channel
        .as_ref()
        .and_then(|c| c.name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "general".to_string());
    let channel = DiscordChannel {
        id: export.channel.as_ref().and_then(|c| c.id.clone()),
        name: channel_name,
        category: export.channel.as_ref().and_then(|c| c.category.clone()),
    };

    let mut users: Vec<DiscordUser> = Vec::new();
    let mut messages: Vec<DiscordMessage> = Vec::new();
    for message in export.messages.into_iter() {
        let author_id = message
            .author
            .as_ref()
            .and_then(|a| a.id.clone())
            .or_else(|| message.author.as_ref().and_then(|a| a.name.clone()))
            .unwrap_or_else(|| "unknown".to_string());
        let author_name = message
            .author
            .as_ref()
            .and_then(|a| a.nickname.clone())
            .or_else(|| message.author.as_ref().and_then(|a| a.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string());
        let discriminator = message.author.as_ref().and_then(|a| a.discriminator.clone());
        let nickname = message.author.as_ref().and_then(|a| a.nickname.clone());
        let is_bot = message.author.as_ref().and_then(|a| a.is_bot).unwrap_or(false);

        if let Some(user) = users.iter_mut().find(|u| u.id == author_id) {
            user.message_count += 1;
        } else {
            users.push(DiscordUser {
                id: author_id.clone(),
                name: author_name,
                discriminator,
                nickname,
                is_bot,
                message_count: 1,
            });
        }

        messages.push(DiscordMessage {
            id: message.id,
            channel_id: channel.id.clone(),
            author_id,
            timestamp: message.timestamp,
            content: message.content.unwrap_or_default(),
        });
    }

    users.sort_by(|a, b| b.message_count.cmp(&a.message_count));
    if messages.is_empty() {
        return Err("The export contains no messages.".to_string());
    }
    Ok(DiscordDataset {
        guild: DiscordGuild {
            id: guild_id,
            name: guild_name,
        },
        channels: vec![channel],
        users,
        messages,
    })
}

// ---------------------------------------------------------------------------
// Plaintext (ported from src/parse.ts)
// ---------------------------------------------------------------------------

struct ParsedMsg {
    author: Option<String>,
    timestamp: Option<String>,
    content: String,
}

fn re(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("static regex must compile")
}

fn parse_plaintext(content: &str, name_hint: &str) -> Result<DiscordDataset, String> {
    let messages = parse_chat_log(content);
    if messages.is_empty() {
        return Err("No messages parsed from the text. Use a DiscordChatExporter plaintext/JSON export or a copied chat log.".to_string());
    }

    let mut users: Vec<DiscordUser> = Vec::new();
    let mut out: Vec<DiscordMessage> = Vec::new();
    for message in messages {
        let author = message.author.unwrap_or_else(|| "unknown".to_string());
        if let Some(user) = users.iter_mut().find(|u| u.id == author) {
            user.message_count += 1;
        } else {
            let (name, discriminator) = split_discriminator(&author);
            users.push(DiscordUser {
                id: author.clone(),
                name,
                discriminator,
                nickname: None,
                is_bot: false,
                message_count: 1,
            });
        }
        out.push(DiscordMessage {
            id: None,
            channel_id: None,
            author_id: author,
            timestamp: message.timestamp,
            content: message.content,
        });
    }
    users.sort_by(|a, b| b.message_count.cmp(&a.message_count));

    Ok(DiscordDataset {
        guild: DiscordGuild {
            id: None,
            name: name_hint.trim().to_string(),
        },
        channels: vec![DiscordChannel {
            id: None,
            name: "messages".to_string(),
            category: None,
        }],
        users,
        messages: out,
    })
}

fn split_discriminator(author: &str) -> (String, Option<String>) {
    if let Some((name, disc)) = author.rsplit_once('#') {
        if !disc.is_empty() && disc.chars().all(|c| c.is_ascii_digit()) {
            return (name.to_string(), Some(disc.to_string()));
        }
    }
    (author.to_string(), None)
}

fn parse_chat_log(text: &str) -> Vec<ParsedMsg> {
    let lines: Vec<String> = text.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
    let style = detect_style(&lines);

    match style {
        Style::Raw => parse_raw(&lines),
        _ => {
            let mut messages: Vec<ParsedMsg> = Vec::new();
            let mut current: Option<ParsedMsg> = None;
            let flush = |messages: &mut Vec<ParsedMsg>, current: &mut Option<ParsedMsg>| {
                if let Some(c) = current.take() {
                    if !c.content.trim().is_empty() {
                        messages.push(c);
                    }
                }
            };
            for raw in &lines {
                let line = raw.trim();
                if line.is_empty() {
                    flush(&mut messages, &mut current);
                    continue;
                }
                if let Some(header) = classify_header(line, style) {
                    flush(&mut messages, &mut current);
                    let content = header.2.trim().to_string();
                    if !content.is_empty() {
                        messages.push(ParsedMsg {
                            author: header.0,
                            timestamp: header.1,
                            content,
                        });
                    } else {
                        current = Some(ParsedMsg {
                            author: header.0,
                            timestamp: header.1,
                            content: String::new(),
                        });
                    }
                } else if let Some(cur) = current.as_mut() {
                    cur.content = if cur.content.is_empty() {
                        line.to_string()
                    } else {
                        format!("{} {}", cur.content, line)
                    };
                } else {
                    messages.push(ParsedMsg {
                        author: None,
                        timestamp: None,
                        content: line.to_string(),
                    });
                }
            }
            flush(&mut messages, &mut current);
            messages
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    DcExporter,
    CopyPaste,
    Markdown,
    SimpleAuthor,
    Raw,
}

fn detect_style(lines: &[String]) -> Style {
    let mut counts = [0usize; 4];
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if match_dc(line).is_some() {
            counts[0] += 1;
        }
        if match_markdown(line).is_some() {
            counts[1] += 1;
        }
        if match_copy_paste(line).is_some() {
            counts[2] += 1;
        }
        if match_simple(line).is_some() {
            counts[3] += 1;
        }
    }
    let mut best = 0usize;
    let mut best_count = 0usize;
    for (i, count) in counts.iter().enumerate() {
        if *count > best_count {
            best = i;
            best_count = *count;
        }
    }
    if best_count == 0 {
        Style::Raw
    } else {
        match best {
            0 => Style::DcExporter,
            1 => Style::Markdown,
            2 => Style::CopyPaste,
            _ => Style::SimpleAuthor,
        }
    }
}

/// Returns (author, timestamp, content) or None.
fn classify_header(line: &str, style: Style) -> Option<(Option<String>, Option<String>, String)> {
    match style {
        Style::DcExporter => match_dc(line),
        Style::Markdown => match_markdown(line).map(|(a, c)| (a, None, c)),
        Style::CopyPaste => match_copy_paste(line),
        Style::SimpleAuthor => match_simple(line),
        Style::Raw => None,
    }
}

fn match_dc(line: &str) -> Option<(Option<String>, Option<String>, String)> {
    let caps = re(r"^\[([^\]]+)\]\s*([^:]+):\s*(.*)$").captures(line)?;
    if !caps.get(1)?.as_str().chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let author = caps.get(2)?.as_str().trim();
    Some((
        if author.is_empty() { None } else { Some(author.to_string()) },
        Some(caps.get(1)?.as_str().trim().to_string()),
        caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default(),
    ))
}

fn match_markdown(line: &str) -> Option<(Option<String>, String)> {
    let caps = re(r"^\*\*([^*]+)\*\*\s*(?::\s*)?(.*)$").captures(line)?;
    let author = caps.get(1)?.as_str().trim();
    if author.is_empty() {
        return None;
    }
    Some((Some(author.to_string()), caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default()))
}

fn match_copy_paste(line: &str) -> Option<(Option<String>, Option<String>, String)> {
    let caps = re(r"^(.+?)\s+[—–-]\s+(.+)$").captures(line)?;
    let right = caps.get(2)?.as_str();
    if !re(r"\d{1,2}:\d{2}\s*(?:AM|PM)?|\d{1,2}[/-]\d{1,2}[/-]\d{2,4}|(?:Today|Yesterday)").is_match(right) {
        return None;
    }
    let author = caps.get(1)?.as_str().trim();
    Some((
        if author.is_empty() { None } else { Some(author.to_string()) },
        Some(right.trim().to_string()),
        String::new(),
    ))
}

fn match_simple(line: &str) -> Option<(Option<String>, Option<String>, String)> {
    if line.contains("://") {
        return None;
    }
    let caps = re(r"^([A-Za-z][A-Za-z0-9 ._'-]{0,31}):\s+(.+)$").captures(line)?;
    let author = caps.get(1)?.as_str().trim();
    if author.is_empty() {
        return None;
    }
    Some((
        Some(author.to_string()),
        None,
        caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
    ))
}

fn parse_raw(lines: &[String]) -> Vec<ParsedMsg> {
    let has_blank = lines.iter().any(|l| l.trim().is_empty());
    let mut messages: Vec<ParsedMsg> = Vec::new();
    let push = |messages: &mut Vec<ParsedMsg>, content: String| {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            messages.push(ParsedMsg {
                author: None,
                timestamp: None,
                content: trimmed,
            });
        }
    };
    if has_blank {
        let mut block = String::new();
        for raw in lines {
            let line = raw.trim();
            if line.is_empty() {
                push(&mut messages, std::mem::take(&mut block));
            } else {
                block = if block.is_empty() { line.to_string() } else { format!("{block} {line}") };
            }
        }
        push(&mut messages, block);
    } else {
        for raw in lines {
            push(&mut messages, raw.trim().to_string());
        }
    }
    messages
}

// ---------------------------------------------------------------------------
// HTML (DiscordChatExporter, best-effort)
// ---------------------------------------------------------------------------

fn strip_script_style(html: &str) -> String {
    let no_script = re(r"(?is)<script[^>]*>.*?</script>").replace_all(html, "").to_string();
    re(r"(?is)<style[^>]*>.*?</style>").replace_all(&no_script, "").to_string()
}

fn strip_tags(text: &str) -> String {
    re(r"<[^>]+>").replace_all(text, "").to_string()
}

fn unescape_html(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
}

fn parse_html(content: &str, name_hint: &str) -> Result<DiscordDataset, String> {
    let body = strip_script_style(content);
    let group_re = re(r"chatlog__message-group");
    let groups: Vec<&str> = group_re.split(&body).collect();

    let author_re = re(r#"chatlog__author-name"[^>]*title="([^"]+)"[^>]*>([^<]*)"#);
    let content_re = re(r#"class="preserve-whitespace">([\s\S]*?)</span>"#);

    let mut users: Vec<DiscordUser> = Vec::new();
    let mut messages: Vec<DiscordMessage> = Vec::new();

    for group in groups.into_iter().skip(1) {
        // First message-group header may be empty; skipping index 0 keeps
        // only content that follows a group marker.
        let author = author_re.captures(group).map(|caps| {
            let inner = unescape_html(caps.get(2).map(|m| m.as_str()).unwrap_or(""));
            let title = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let display = if inner.trim().is_empty() { title.to_string() } else { inner };
            (display, title.to_string())
        });
        if author.is_none() {
            continue;
        }
        let (name, title) = author.unwrap();
        let is_bot = group.contains("chatlog__bot-tag");
        let author_id = name.clone();
        let (clean_name, discriminator) = split_discriminator(&name);

        if let Some(user) = users.iter_mut().find(|u| u.id == author_id) {
            user.message_count += 1;
        } else {
            users.push(DiscordUser {
                id: author_id.clone(),
                name: clean_name,
                discriminator,
                nickname: None,
                is_bot,
                message_count: 1,
            });
        }

        for caps in content_re.captures_iter(group) {
            let text = strip_tags(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
            let text = unescape_html(&text);
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.trim().is_empty() {
                continue;
            }
            messages.push(DiscordMessage {
                id: None,
                channel_id: None,
                author_id: author_id.clone(),
                timestamp: None,
                content: text,
            });
        }
        // Title is only used as a fallback discriminator source above.
        let _ = title;
    }

    if messages.is_empty() {
        return Err("HTML parsing produced no messages. Re-export the channel as DiscordChatExporter JSON for full fidelity.".to_string());
    }

    users.sort_by(|a, b| b.message_count.cmp(&a.message_count));
    Ok(DiscordDataset {
        guild: DiscordGuild {
            id: None,
            name: name_hint.trim().to_string(),
        },
        channels: vec![DiscordChannel {
            id: None,
            name: "messages".to_string(),
            category: None,
        }],
        users,
        messages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dce_json() {
        let json = r#"{
          "guild": {"id": "1", "name": "Test Server"},
          "channel": {"id": "2", "name": "general", "category": "Text"},
          "messages": [
            {"id": "m1", "timestamp": "2026-01-01T10:00:00Z", "content": "hello", "author": {"id": "u1", "name": "alice", "discriminator": "1234", "nickname": "Alice", "isBot": false}},
            {"id": "m2", "timestamp": "2026-01-01T10:01:00Z", "content": "beep", "author": {"id": "b1", "name": "botty", "isBot": true}}
          ]
        }"#;
        let ds = parse_json(json).unwrap();
        assert_eq!(ds.guild.name, "Test Server");
        assert_eq!(ds.messages.len(), 2);
        assert_eq!(ds.users.len(), 2);
        let alice = ds.users.iter().find(|u| u.id == "u1").unwrap();
        assert!(!alice.is_bot);
        assert_eq!(alice.name, "Alice");
        let bot = ds.users.iter().find(|u| u.id == "b1").unwrap();
        assert!(bot.is_bot);
    }

    #[test]
    fn parses_dc_plaintext() {
        let text = "[2026-01-01 10:00:00] Alice#1234: hello world\n[2026-01-01 10:01:00] Alice#1234: second message";
        let ds = parse_plaintext(text, "log.txt").unwrap();
        assert_eq!(ds.messages.len(), 2);
        assert_eq!(ds.users.len(), 1);
        assert_eq!(ds.users[0].name, "Alice");
        assert_eq!(ds.users[0].discriminator.as_deref(), Some("1234"));
    }

    #[test]
    fn parses_simple_author() {
        let text = "Alice: first line\nBob: another line\nAlice: third";
        let ds = parse_plaintext(text, "log").unwrap();
        assert_eq!(ds.messages.len(), 3);
        assert_eq!(ds.users.len(), 2);
    }

    #[test]
    fn rejects_empty_input() {
        assert!(parse_plaintext("", "x").is_err());
    }

    #[test]
    fn parses_html_best_effort() {
        let html = r#"<html><head><style>x{}</style></head><body>
          <div class="chatlog__message-group">
            <div class="chatlog__author-avatar-container">
              <span class="chatlog__author-name" title="Alice#1234" data-user-id="1">Alice</span>
              <span class="chatlog__bot-tag">BOT</span>
            </div>
            <div class="chatlog__messages">
              <div class="chatlog__message">
                <div class="chatlog__content"><div class="markdown"><span class="preserve-whitespace">beep boop</span></div></div>
              </div>
            </div>
          </div>
        </body></html>"#;
        let ds = parse_html(html, "export.html").unwrap();
        assert_eq!(ds.messages.len(), 1);
        assert_eq!(ds.messages[0].content, "beep boop");
        assert_eq!(ds.users[0].name, "Alice");
        assert!(ds.users[0].is_bot);
    }
}
