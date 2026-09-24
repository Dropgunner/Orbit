//! Minimal OpenAI-compatible chat-completions client with SSE streaming and
//! tool-call delta accumulation. Works against DeepSeek and any other
//! OpenAI-compatible endpoint by changing the base URL.

use std::collections::HashMap;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::types::{ChatMessage, FunctionCall, ToolCall};

/// Connection settings for an OpenAI-compatible endpoint.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
}

/// A tool definition sent to the model.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub def_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [ToolDef]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    stream: bool,
    temperature: f64,
}

/// The accumulated result of one streamed assistant turn.
#[derive(Debug, Default)]
pub struct StreamedResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    #[allow(dead_code)]
    index: usize,
    delta: ChunkDelta,
}

#[derive(Debug, Default, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Debug, Default, Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Streams one assistant turn. `on_delta` is invoked for each text token as
/// it arrives so callers can forward it to the UI in real time.
pub async fn stream_chat<F>(
    config: &ModelConfig,
    messages: &[ChatMessage],
    tools: &[ToolDef],
    mut on_delta: F,
) -> Result<StreamedResponse, String>
where
    F: FnMut(&str),
{
    let url = format!(
        "{}/chat/completions",
        config.base_url.trim_end_matches('/')
    );

    let request = ChatRequest {
        model: &config.model,
        messages,
        tools: if tools.is_empty() { None } else { Some(tools) },
        tool_choice: if tools.is_empty() {
            None
        } else {
            Some(serde_json::json!("auto"))
        },
        stream: true,
        temperature: config.temperature,
    };

    let client = reqwest::Client::new();
    let mut builder = client.post(&url).json(&request);
    if !config.api_key.is_empty() {
        builder = builder.bearer_auth(&config.api_key);
    }
    let response = builder
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {text}"));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    let mut content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    // Maps a streamed tool-call index to its position in `tool_calls`.
    let mut tool_index: HashMap<usize, usize> = HashMap::new();

    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        loop {
            let Some(newline) = buffer.find('\n') else { break };
            let line = buffer[..newline].trim_end_matches('\r').to_string();
            buffer = buffer[newline + 1..].to_string();

            let data = match line.strip_prefix("data:") {
                Some(d) => d.trim(),
                None => continue,
            };
            if data.is_empty() || data == "[DONE]" {
                continue;
            }

            let chunk: Chunk =
                serde_json::from_str(data).map_err(|e| format!("bad chunk: {e}"))?;

            let Some(choice) = chunk.choices.into_iter().next() else {
                continue;
            };

            if let Some(c) = choice.delta.content {
                if !c.is_empty() {
                    content.push_str(&c);
                    on_delta(&c);
                }
            }

            if let Some(deltas) = choice.delta.tool_calls {
                for delta in deltas {
                    let pos = match tool_index.get(&delta.index) {
                        Some(&p) => p,
                        None => {
                            let p = tool_calls.len();
                            tool_calls.push(ToolCall {
                                id: String::new(),
                                call_type: "function".to_string(),
                                function: FunctionCall {
                                    name: String::new(),
                                    arguments: String::new(),
                                },
                            });
                            tool_index.insert(delta.index, p);
                            p
                        }
                    };

                    let call = &mut tool_calls[pos];
                    if let Some(id) = delta.id {
                        if !id.is_empty() {
                            call.id = id;
                        }
                    }
                    if let Some(function) = delta.function {
                        if let Some(name) = function.name {
                            if !name.is_empty() {
                                call.function.name.push_str(&name);
                            }
                        }
                        if let Some(arguments) = function.arguments {
                            call.function.arguments.push_str(&arguments);
                        }
                    }
                }
            }

        }
    }

    Ok(StreamedResponse {
        content,
        tool_calls,
    })
}
