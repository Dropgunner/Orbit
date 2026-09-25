//! Native Discord REST client (user token) for live browsing: list guilds,
//! channels, members, and fetch channel messages into a [`DiscordDataset`].
//! Uses `Authorization: <token>` (raw user token, like DiscordChatExporter's
//! "auto" mode). Requests are rate-limit aware (retries on HTTP 429).

use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::discord::{DiscordChannel, DiscordDataset, DiscordGuild, DiscordMessage, DiscordUser};

const API_BASE: &str = "https://discord.com/api/v10";
const USER_AGENT: &str = "orbit-discord-analysis/0.1 (https://github.com/Tyrrrz/DiscordChatExporter)";
const MAX_ATTEMPTS: usize = 5;
const MESSAGE_PAGE: u16 = 100;
/// Appended to a 403, where Discord accepted the token and refused the read.
const FORBIDDEN_HINT: &str = "the token is valid but this account cannot read that resource; a bot token needs the bot invited to the server with permissions, and a user account needs to be a member with View Channel on the server and channel";

/// A failed Discord REST call. `status` is kept so callers can tell a refused
/// read (403) from a rejected token (401) or an unreachable host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordError {
    /// HTTP status, or `None` when the request never completed.
    pub status: Option<StatusCode>,
    /// A complete, user-facing description of the failure.
    pub message: String,
}

impl DiscordError {
    /// Whether Discord accepted the token and refused access to the resource.
    pub fn is_forbidden(&self) -> bool {
        self.status == Some(StatusCode::FORBIDDEN)
    }
}

impl std::fmt::Display for DiscordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(f, "Discord HTTP {status}: {}", self.message)?,
            None => write!(f, "{}", self.message)?,
        }
        if self.is_forbidden() {
            write!(f, ". {FORBIDDEN_HINT}")?;
        }
        Ok(())
    }
}

impl From<DiscordError> for String {
    fn from(error: DiscordError) -> Self {
        error.to_string()
    }
}

/// A Discord user object (subset of the API's `User`).
#[derive(Debug, Clone, Deserialize)]
pub struct ApiUser {
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub discriminator: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub bot: Option<bool>,
}

/// A Discord guild (server).
#[derive(Debug, Clone, Deserialize)]
pub struct ApiGuild {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// A Discord channel.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiChannel {
    pub id: String,
    #[serde(rename = "type")]
    pub channel_type: u8,
    #[serde(default)]
    pub name: String,
}

/// A guild member (subset).
#[derive(Debug, Clone, Deserialize)]
pub struct ApiMember {
    #[serde(default)]
    pub user: Option<ApiUser>,
    #[serde(default)]
    pub nick: Option<String>,
}

/// A Discord message (subset).
#[derive(Debug, Clone, Deserialize)]
pub struct ApiMessage {
    pub id: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub author: Option<ApiUser>,
}

/// The current authenticated account.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiCurrentUser {
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
}

impl ApiUser {
    /// Display name: global_name (nickname-equivalent) else username.
    pub fn display_name(&self) -> String {
        let fallback = if self.username.is_empty() { "Unknown" } else { &self.username };
        self.global_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| fallback.to_string())
    }
}

/// A minimal, rate-limit-aware client for the Discord REST API.
#[derive(Clone)]
pub struct DiscordClient {
    http: reqwest::Client,
    token: String,
}

impl DiscordClient {
    pub fn new(token: String) -> Result<Self, String> {
        if token.trim().is_empty() {
            return Err("No Discord token configured.".to_string());
        }
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("client: {e}"))?;
        Ok(Self { http, token })
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, DiscordError> {
        let url = format!("{API_BASE}{path}");
        let mut attempt = 0;
        loop {
            let response = self
                .http
                .get(&url)
                .header("Authorization", &self.token)
                .send()
                .await
                .map_err(|e| DiscordError {
                    status: None,
                    message: format!("Discord request failed: {e}"),
                })?;

            if response.status().as_u16() == 429 {
                attempt += 1;
                if attempt >= MAX_ATTEMPTS {
                    return Err(DiscordError {
                        status: Some(StatusCode::TOO_MANY_REQUESTS),
                        message: "rate limited; try again later.".to_string(),
                    });
                }
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(1.0);
                tokio::time::sleep(Duration::from_secs_f64(delay.max(0.25))).await;
                continue;
            }

            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(DiscordError {
                    status: Some(status),
                    message: discord_error_message(&text),
                });
            }
            return serde_json::from_str(&text).map_err(|e| DiscordError {
                status: None,
                message: format!("Discord bad response: {e}"),
            });
        }
    }

    /// The authenticated account, used to validate the token.
    pub async fn current_user(&self) -> Result<ApiCurrentUser, DiscordError> {
        self.get_json("/users/@me").await
    }

    /// Servers the account can access.
    pub async fn list_guilds(&self) -> Result<Vec<ApiGuild>, DiscordError> {
        self.get_json("/users/@me/guilds").await
    }

    /// Text/announcement channels in a server.
    pub async fn list_channels(&self, guild_id: &str) -> Result<Vec<ApiChannel>, DiscordError> {
        let channels: Vec<ApiChannel> = self
            .get_json(&format!("/guilds/{guild_id}/channels"))
            .await?;
        Ok(channels.into_iter().filter(|c| matches!(c.channel_type, 0 | 5)).collect())
    }

    /// Members of a server (up to `limit`).
    pub async fn list_members(&self, guild_id: &str, limit: usize) -> Result<Vec<ApiMember>, DiscordError> {
        self.get_json(&format!("/guilds/{guild_id}/members?limit={}", limit.min(1000))).await
    }

    /// Messages in a channel (newest first), up to `limit`, paginating by `before`.
    pub async fn fetch_messages(&self, channel_id: &str, limit: usize) -> Result<Vec<ApiMessage>, DiscordError> {
        let mut out: Vec<ApiMessage> = Vec::new();
        let mut before: Option<String> = None;
        while out.len() < limit {
            let remaining = (limit - out.len()).min(MESSAGE_PAGE as usize);
            let mut path = format!("/channels/{channel_id}/messages?limit={remaining}");
            if let Some(before_id) = &before {
                path.push_str(&format!("&before={before_id}"));
            }
            let batch: Vec<ApiMessage> = self.get_json(&path).await?;
            if batch.is_empty() {
                break;
            }
            let last_id = batch.last().map(|m| m.id.clone());
            out.extend(batch);
            if last_id.as_deref() == before.as_deref() || out.len() >= limit {
                break;
            }
            before = last_id;
        }
        out.truncate(limit);
        Ok(out)
    }
}

fn discord_error_message(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| {
            // Truncate on a character boundary: a byte slice panics on any
            // non-ASCII body (proxy or localised Discord responses).
            text.trim().chars().take(200).collect()
        })
}

/// Build a [`DiscordDataset`] from a channel's messages, with members used to
/// fill in nicknames. Users are the distinct message authors (with the bot
/// flag); every member also contributes a user entry so the roster is browsable
/// even when a member has not posted in the fetched window.
pub fn build_dataset(
    guild_id: Option<String>,
    guild_name: &str,
    channel_id: Option<String>,
    channel_name: &str,
    messages: Vec<ApiMessage>,
    members: Vec<ApiMember>,
) -> DiscordDataset {
    let mut users: Vec<DiscordUser> = Vec::new();
    let mut by_id = std::collections::HashMap::<String, usize>::new();

    let nick_by_id: std::collections::HashMap<String, String> = members
        .iter()
        .filter_map(|m| {
            let id = m.user.as_ref().map(|u| u.id.clone())?;
            m.nick.clone().map(|n| (id, n))
        })
        .collect();

    let mut upsert = |users: &mut Vec<DiscordUser>, id: String, user: DiscordUser| {
        if let Some(&idx) = by_id.get(&id) {
            let existing = &mut users[idx];
            existing.message_count += user.message_count;
            if existing.nickname.is_none() && user.nickname.is_some() {
                existing.nickname = user.nickname;
            }
            if user.is_bot {
                existing.is_bot = true;
            }
        } else {
            by_id.insert(id.clone(), users.len());
            users.push(user);
        }
    };

    // Roster first (members who may not have posted), with no message count.
    for member in &members {
        if let Some(api_user) = &member.user {
            let id = api_user.id.clone();
            upsert(
                &mut users,
                id.clone(),
                DiscordUser {
                    id,
                    name: api_user.display_name(),
                    discriminator: nonempty(api_user.discriminator.clone()),
                    nickname: member.nick.clone().filter(|n| !n.trim().is_empty()),
                    is_bot: api_user.bot.unwrap_or(false),
                    message_count: 0,
                },
            );
        }
    }

    let mut out: Vec<DiscordMessage> = Vec::with_capacity(messages.len());
    for message in messages {
        let author = message.author.unwrap_or(ApiUser {
            id: "unknown".to_string(),
            username: "Unknown".to_string(),
            discriminator: String::new(),
            global_name: None,
            bot: None,
        });
        let author_id = author.id.clone();
        upsert(
            &mut users,
            author_id.clone(),
            DiscordUser {
                id: author_id.clone(),
                name: author.display_name(),
                discriminator: nonempty(author.discriminator.clone()),
                nickname: nick_by_id.get(&author_id).cloned(),
                is_bot: author.bot.unwrap_or(false),
                message_count: 1,
            },
        );
        out.push(DiscordMessage {
            id: Some(message.id),
            channel_id: channel_id.clone(),
            author_id,
            timestamp: message.timestamp,
            content: message.content,
        });
    }

    users.sort_by(|a, b| b.message_count.cmp(&a.message_count).then_with(|| a.name.cmp(&b.name)));
    let channels = if channel_id.is_some() || !channel_name.is_empty() {
        vec![DiscordChannel {
            id: channel_id.clone(),
            name: channel_name.to_string(),
            category: None,
        }]
    } else {
        Vec::new()
    };

    DiscordDataset {
        guild: DiscordGuild {
            id: guild_id,
            name: guild_name.to_string(),
        },
        channels,
        users,
        messages: out,
    }
}

fn nonempty(value: String) -> Option<String> {
    let t = value.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_user(id: &str, username: &str, bot: bool) -> ApiUser {
        ApiUser {
            id: id.to_string(),
            username: username.to_string(),
            discriminator: "0".to_string(),
            global_name: None,
            bot: Some(bot),
        }
    }

    #[test]
    fn build_dataset_groups_authors_and_flags_bots() {
        let messages = vec![
            ApiMessage {
                id: "m1".to_string(),
                timestamp: Some("2026-01-01T10:00:00Z".to_string()),
                content: "hello".to_string(),
                author: Some(api_user("u1", "alice", false)),
            },
            ApiMessage {
                id: "m2".to_string(),
                timestamp: None,
                content: "beep".to_string(),
                author: Some(api_user("b1", "botty", true)),
            },
            ApiMessage {
                id: "m3".to_string(),
                timestamp: None,
                content: "again".to_string(),
                author: Some(api_user("u1", "alice", false)),
            },
        ];
        let ds = build_dataset(Some("g1".to_string()), "Server", Some("c1".to_string()), "general", messages, vec![]);
        assert_eq!(ds.messages.len(), 3);
        assert_eq!(ds.users.len(), 2);
        let alice = ds.users.iter().find(|u| u.id == "u1").unwrap();
        assert_eq!(alice.message_count, 2);
        assert!(!alice.is_bot);
        let bot = ds.users.iter().find(|u| u.id == "b1").unwrap();
        assert!(bot.is_bot);
    }

    #[test]
    fn member_nicknames_enrich_roster() {
        let members = vec![ApiMember {
            user: Some(api_user("u1", "alice", false)),
            nick: Some("Alice The Great".to_string()),
        }];
        let ds = build_dataset(None, "S", None, "", vec![], members);
        assert_eq!(ds.users.len(), 1);
        assert_eq!(ds.users[0].nickname.as_deref(), Some("Alice The Great"));
        assert_eq!(ds.users[0].message_count, 0);
    }

    #[test]
    fn channel_filter_keeps_text_and_news() {
        let channels = vec![
            ApiChannel { id: "1".to_string(), channel_type: 0, name: "general".to_string() },
            ApiChannel { id: "2".to_string(), channel_type: 2, name: "voice".to_string() },
            ApiChannel { id: "3".to_string(), channel_type: 5, name: "news".to_string() },
        ];
        // list_channels does the filtering via the API; assert the predicate inline.
        let kept: Vec<&ApiChannel> = channels.iter().filter(|c| matches!(c.channel_type, 0 | 5)).collect();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "general");
        assert_eq!(kept[1].name, "news");
    }

    #[test]
    fn forbidden_error_names_the_likely_cause() {
        let error = DiscordError {
            status: Some(StatusCode::FORBIDDEN),
            message: "Missing Access".to_string(),
        };
        assert!(error.is_forbidden());
        let text = error.to_string();
        assert!(text.starts_with("Discord HTTP 403 Forbidden: Missing Access."), "{text}");
        assert!(text.contains("bot token"), "{text}");
        assert_eq!(String::from(error.clone()), text);
    }

    #[test]
    fn rejected_token_is_not_reported_as_forbidden() {
        let error = DiscordError {
            status: Some(StatusCode::UNAUTHORIZED),
            message: "401: Unauthorized".to_string(),
        };
        assert!(!error.is_forbidden());
        assert_eq!(error.to_string(), "Discord HTTP 401 Unauthorized: 401: Unauthorized");
    }

    #[test]
    fn transport_failure_keeps_its_message() {
        let error = DiscordError {
            status: None,
            message: "Discord request failed: connection closed".to_string(),
        };
        assert!(!error.is_forbidden());
        assert_eq!(error.to_string(), "Discord request failed: connection closed");
    }
}
