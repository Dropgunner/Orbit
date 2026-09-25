//! A local, token-protected HTTP server that serves the embedded frontend and
//! bridges `POST /api/invoke` to the same Tauri command functions the desktop
//! webview calls. The same React app therefore runs in a browser, against the
//! same Rust/TypeSafe backend, without a second API implementation.
//!
//! Security: bound to `127.0.0.1` only; `/api/*` requires a per-run token; the
//! `Origin` header, when present, must match this server exactly. Assets carry
//! no token (the shell is not sensitive; the data behind `/api` is).

use std::convert::Infallible;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::net::TcpListener;

use crate::state::AppState;

/// A running local web server: its port and the token the browser must send.
#[derive(Debug, Clone, Serialize)]
pub struct WebServer {
    pub port: u16,
    pub token: String,
}

impl WebServer {
    /// The URL to open in a browser, with the token attached.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/?token={}", self.port, self.token)
    }
}

/// The server plus its accept-loop task, so it can be stopped.
pub struct Running {
    pub info: WebServer,
    pub handle: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Deserialize)]
struct InvokeRequest {
    command: String,
    #[serde(default)]
    args: Value,
}

/// Generate a per-run token (splitmix64 over time, pid, and stack address).
fn random_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = nanos ^ ((std::process::id() as u64) << 32) ^ (&nanos as *const _ as u64);
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    (0..4).map(|_| format!("{:016x}", next())).collect()
}

/// A running server for this app, if one is up.
pub fn current(app: &AppHandle) -> Option<WebServer> {
    app.state::<AppState>()
        .web
        .lock()
        .unwrap()
        .as_ref()
        .map(|running| running.info.clone())
}

/// Start the server if it is not already running.
pub async fn start(app: &AppHandle) -> Result<WebServer, String> {
    if let Some(existing) = current(app) {
        return Ok(existing);
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = random_token();
    let info = WebServer {
        port,
        token: token.clone(),
    };

    let app_for_task = app.clone();
    let token_for_task = token.clone();
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let io = TokioIo::new(stream);
            let app = app_for_task.clone();
            let token = token_for_task.clone();
            tauri::async_runtime::spawn(async move {
                let service = service_fn(move |request| {
                    let app = app.clone();
                    let token = token.clone();
                    async move { handle_request(app, token, request).await }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    *app.state::<AppState>().web.lock().unwrap() = Some(Running { info: info.clone(), handle });
    Ok(info)
}

/// Stop the server, if running.
pub fn stop(app: &AppHandle) {
    if let Some(running) = app.state::<AppState>().web.lock().unwrap().take() {
        running.handle.abort();
    }
}

fn json_response(status: StatusCode, value: &Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(value.to_string())))
        .expect("static response")
}

fn ok(value: Value) -> Response<Full<Bytes>> {
    json_response(StatusCode::OK, &json!({ "ok": true, "value": value }))
}

fn fail(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    json_response(status, &json!({ "ok": false, "error": message }))
}

async fn handle_request(
    app: AppHandle,
    token: String,
    request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(route(app, token, request).await.unwrap_or_else(|e| {
        fail(StatusCode::BAD_REQUEST, &e)
    }))
}

async fn route(
    app: AppHandle,
    token: String,
    request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, String> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    if path == "/api/invoke" {
        if method != Method::POST {
            return Ok(fail(StatusCode::METHOD_NOT_ALLOWED, "POST required"));
        }
        if !same_origin(&request) {
            return Ok(fail(StatusCode::FORBIDDEN, "cross-origin request rejected"));
        }
        if !authorized(&request, &token) {
            return Ok(fail(StatusCode::UNAUTHORIZED, "missing or invalid token"));
        }
        let body = request
            .collect()
            .await
            .map_err(|e| e.to_string())?
            .to_bytes();
        let payload: InvokeRequest =
            serde_json::from_slice(&body).map_err(|e| format!("bad request body: {e}"))?;
        return Ok(match dispatch(&app, &payload.command, payload.args).await {
            Ok(value) => ok(value),
            Err(error) => fail(StatusCode::BAD_REQUEST, &error),
        });
    }

    if method == Method::GET {
        return Ok(serve_asset(&app, &path));
    }

    Ok(fail(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"))
}

/// Serve an embedded frontend asset, falling back to `index.html` for routes.
fn serve_asset(app: &AppHandle, path: &str) -> Response<Full<Bytes>> {
    let requested = if path == "/" {
        "index.html".to_string()
    } else {
        path.trim_start_matches('/').to_string()
    };
    let resolver = app.asset_resolver();
    let asset = resolver
        .get(requested)
        .or_else(|| resolver.get("index.html".to_string()));
    match asset {
        Some(asset) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", asset.mime_type)
            .body(Full::new(Bytes::from(asset.bytes)))
            .expect("static response"),
        None => fail(
            StatusCode::NOT_FOUND,
            "Not found. The local web UI serves the packaged frontend; run the built app.",
        ),
    }
}

/// Reject a request whose `Origin` is not this server.
fn same_origin(request: &Request<Incoming>) -> bool {
    let Some(origin) = request.headers().get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    origin == format!("http://{host}")
}

fn authorized(request: &Request<Incoming>, token: &str) -> bool {
    if let Some(header) = request
        .headers()
        .get("x-orbit-token")
        .and_then(|v| v.to_str().ok())
    {
        if header == token {
            return true;
        }
    }
    request
        .uri()
        .query()
        .map(|query| {
            query.split('&').any(|pair| {
                pair.strip_prefix("token=")
                    .map(|value| value == token)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Command bridge
// ---------------------------------------------------------------------------

fn arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    let value = args
        .get(key)
        .ok_or_else(|| format!("missing argument \"{key}\""))?;
    serde_json::from_value(value.clone()).map_err(|e| format!("argument \"{key}\": {e}"))
}

fn opt_arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| format!("argument \"{key}\": {e}")),
    }
}

fn to_value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

/// Whether the browser bridge may reach the coding surface (filesystem writes,
/// shell commands, and the agent).
///
/// The Discord analysis surface is always available, because it only touches
/// data the user imported. Coding commands are different in kind: they are
/// arbitrary file writes and shell execution inside the workspace, so they stay
/// off unless the user opts in with `ORBIT_WEB_ALLOW_AGENT=1`.
fn agent_surface_enabled() -> bool {
    matches!(
        std::env::var("ORBIT_WEB_ALLOW_AGENT").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// Rejects a coding command when the opt-in is missing.
fn require_agent_surface() -> Result<(), String> {
    if agent_surface_enabled() {
        return Ok(());
    }
    Err(
        "Coding commands are disabled over the local web UI. Restart Orbit with ORBIT_WEB_ALLOW_AGENT=1 to allow file edits, shell commands, and the agent from the browser."
            .to_string(),
    )
}

/// Dispatch a browser request to the same command function the webview calls.
async fn dispatch(app: &AppHandle, command: &str, args: Value) -> Result<Value, String> {
    use crate::commands::{agent, chain, config, discord, discord_live, dsh, fs, javtools};

    match command {
        // --- settings ---
        // Redacted: the bridge is reachable by anything holding the run token,
        // so it must not be able to read API keys or the Discord token back.
        "get_settings" => to_value(config::resolve_redacted(&app)?),
        "set_settings" => {
            config::set_settings_preserving_secrets(&app, arg(&args, "settings")?)?;
            Ok(Value::Null)
        }

        // --- workspace / self-hosting ---
        "workspace_mode" => to_value(agent::workspace_mode(arg(&args, "workspace")?)),

        // --- coding surface (opt-in, see `require_agent_surface`) ---
        "list_dir" => {
            require_agent_surface()?;
            to_value(fs::list_dir(arg(&args, "path")?)?)
        }
        "read_file" => {
            require_agent_surface()?;
            to_value(fs::read_file(arg(&args, "path")?)?)
        }
        "write_file" => {
            require_agent_surface()?;
            fs::write_file(arg(&args, "path")?, arg(&args, "content")?)?;
            Ok(Value::Null)
        }
        "search" => {
            require_agent_surface()?;
            to_value(fs::search(arg(&args, "root")?, arg(&args, "query")?)?)
        }
        "chat" => {
            require_agent_surface()?;
            to_value(
                agent::chat(
                    app.clone(),
                    arg(&args, "history")?,
                    arg(&args, "prompt")?,
                    arg(&args, "workspace")?,
                )
                .await?,
            )
        }
        "stop_chat" => {
            require_agent_surface()?;
            agent::stop_chat(app.clone(), arg(&args, "runId")?)?;
            Ok(Value::Null)
        }
        "list_pending_edits" => {
            require_agent_surface()?;
            to_value(agent::list_pending_edits(app.state::<AppState>()))
        }
        "apply_pending_edit" => {
            require_agent_surface()?;
            agent::apply_pending_edit(app.state::<AppState>(), arg(&args, "id")?)?;
            Ok(Value::Null)
        }
        "discard_pending_edit" => {
            require_agent_surface()?;
            agent::discard_pending_edit(app.state::<AppState>(), arg(&args, "id")?)?;
            Ok(Value::Null)
        }
        "apply_all_pending_edits" => {
            require_agent_surface()?;
            to_value(agent::apply_all_pending_edits(app.state::<AppState>())?)
        }

        // --- autonomous chains ---
        "start_chain" => {
            require_agent_surface()?;
            to_value(
                chain::start_chain(
                    app.clone(),
                    arg(&args, "goal")?,
                    arg(&args, "workspace")?,
                    opt_arg(&args, "maxCycles")?,
                )
                .await?,
            )
        }
        "stop_chain" => {
            require_agent_surface()?;
            chain::stop_chain(app.clone(), arg(&args, "chainId")?)?;
            Ok(Value::Null)
        }
        "get_chain" => {
            require_agent_surface()?;
            to_value(chain::get_chain(
                app.state::<AppState>(),
                opt_arg(&args, "chainId")?,
            ))
        }

        // --- DeepSeek Harness ---
        "dsh_status" => to_value(dsh::dsh_status(app.clone())),
        "dsh_run" => {
            require_agent_surface()?;
            to_value(
                dsh::dsh_run(
                    app.clone(),
                    arg(&args, "prompt")?,
                    arg(&args, "workspace")?,
                )
                .await?,
            )
        }
        "stop_dsh" => {
            require_agent_surface()?;
            dsh::stop_dsh(app.clone(), arg(&args, "runId")?)?;
            Ok(Value::Null)
        }

        // --- import / browse ---
        "import_discord_text" => to_value(discord::import_discord_text(
            app.clone(),
            arg(&args, "name")?,
            arg(&args, "content")?,
        )?),
        "discord_summary" => to_value(discord::discord_summary(app.clone())?),
        "discord_messages" => to_value(discord::discord_messages(
            app.clone(),
            opt_arg(&args, "userId")?,
            opt_arg(&args, "channelId")?,
            opt_arg(&args, "query")?,
            opt_arg(&args, "limit")?,
        )?),

        // --- analysis ---
        "analyze_profile" => to_value(
            discord::analyze_profile(
                app.clone(),
                arg(&args, "userId")?,
                opt_arg(&args, "limit")?,
                opt_arg(&args, "dryRun")?,
            )
            .await?,
        ),
        "analyze_bot" => to_value(
            discord::analyze_bot(
                app.clone(),
                arg(&args, "userId")?,
                opt_arg(&args, "limit")?,
                opt_arg(&args, "dryRun")?,
            )
            .await?,
        ),
        "analyze_guard" => to_value(
            discord::analyze_guard(
                app.clone(),
                arg(&args, "userId")?,
                opt_arg(&args, "policy")?,
                opt_arg(&args, "limit")?,
                opt_arg(&args, "dryRun")?,
            )
            .await?,
        ),
        "screen_message_cmd" => to_value(
            discord::screen_message_cmd(
                app.clone(),
                arg(&args, "content")?,
                opt_arg(&args, "side")?,
                opt_arg(&args, "policy")?,
                opt_arg(&args, "dryRun")?,
            )
            .await?,
        ),
        "guard_policies" => to_value(discord::guard_policies()),
        "discuss_once" => to_value(
            discord::discuss_once(
                app.clone(),
                arg(&args, "prompt")?,
                arg(&args, "context")?,
            )
            .await?,
        ),

        // --- patterns ---
        "list_patterns" => to_value(discord::list_patterns(app.clone())),
        "flag_pattern" => to_value(discord::flag_pattern(app.clone(), arg(&args, "input")?)?),
        "remove_pattern" => to_value(discord::remove_pattern(app.clone(), arg(&args, "id")?)?),
        "match_patterns" => to_value(discord::match_patterns(
            app.clone(),
            opt_arg(&args, "userId")?,
            opt_arg(&args, "channelId")?,
        )?),

        // --- flags ---
        "list_flags" => to_value(discord::list_flags(app.clone())),
        "set_flag" => to_value(discord::set_flag(
            app.clone(),
            arg(&args, "entityType")?,
            arg(&args, "entityId")?,
            arg(&args, "label")?,
            arg(&args, "action")?,
            arg(&args, "note")?,
        )?),
        "clear_flag" => to_value(discord::clear_flag(
            app.clone(),
            arg(&args, "entityType")?,
            arg(&args, "entityId")?,
        )?),

        // --- export ---
        "discord_export_json_text" => to_value(discord_live::discord_export_json_text(app.clone())?),
        "discord_dce_status" => to_value(discord_live::discord_dce_status(app.clone())),

        // --- live Discord ---
        "discord_whoami" => to_value(discord_live::discord_whoami(app.clone()).await?),
        "discord_guilds" => to_value(discord_live::discord_guilds(app.clone()).await?),
        "discord_channels" => {
            to_value(discord_live::discord_channels(app.clone(), arg(&args, "guildId")?).await?)
        }
        "discord_members" => to_value(
            discord_live::discord_members(
                app.clone(),
                arg(&args, "guildId")?,
                opt_arg(&args, "limit")?,
            )
            .await?,
        ),
        "discord_fetch_channel" => to_value(
            discord_live::discord_fetch_channel(
                app.clone(),
                opt_arg(&args, "guildId")?,
                arg(&args, "guildName")?,
                arg(&args, "channelId")?,
                arg(&args, "channelName")?,
                opt_arg(&args, "limit")?,
            )
            .await?,
        ),
        // The browser UI renders these controls too, so they must be reachable
        // here as well; DCE exports to the app config dir, which the browser
        // then loads back through the summary.
        "discord_dce_export" => to_value(
            discord_live::discord_dce_export(
                app.clone(),
                arg(&args, "channelId")?,
                opt_arg(&args, "format")?,
                opt_arg(&args, "loadIntoApp")?,
            )
            .await?,
        ),

        // --- Jev (TypeSafe) toolkit: the Tools tab, also rendered in a browser ---
        "classify_cmd" => to_value(
            javtools::classify_cmd(
                app.clone(),
                arg(&args, "text")?,
                arg(&args, "instructions")?,
                arg(&args, "labels")?,
            )
            .await?,
        ),
        "score_cmd" => to_value(
            javtools::score_cmd(
                app.clone(),
                arg(&args, "text")?,
                arg(&args, "instructions")?,
                arg(&args, "rubric")?,
            )
            .await?,
        ),
        "psych_profile_cmd" => to_value(
            javtools::psych_profile_cmd(app.clone(), arg(&args, "text")?).await?,
        ),
        "noul_cmd" => to_value(
            javtools::noul_cmd(
                app.clone(),
                arg(&args, "text")?,
                arg(&args, "instructions")?,
                opt_arg(&args, "yes")?,
                opt_arg(&args, "no")?,
            )
            .await?,
        ),
        "prune_cmd" => to_value(
            javtools::prune_cmd(
                app.clone(),
                arg(&args, "text")?,
                opt_arg(&args, "goal")?,
                opt_arg(&args, "chunkLines")?,
                opt_arg(&args, "keepThreshold")?,
            )
            .await?,
        ),
        "compact_cmd" => to_value(
            javtools::compact_cmd(
                app.clone(),
                arg(&args, "entries")?,
                opt_arg(&args, "goal")?,
                opt_arg(&args, "preserveRecent")?,
                opt_arg(&args, "keepThreshold")?,
            )
            .await?,
        ),
        "generate_cmd" => to_value(
            javtools::generate_cmd(
                app.clone(),
                arg(&args, "prompt")?,
                arg(&args, "candidates")?,
                opt_arg(&args, "maxTokens")?,
                opt_arg(&args, "stop")?,
            )
            .await?,
        ),

        other => Err(format!("\"{other}\" is not available in the local web UI.")),
    }
}
