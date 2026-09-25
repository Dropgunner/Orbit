//! Integration with the DiscordChatExporter CLI (https://github.com/Tyrrrz/DiscordChatExporter):
//! resolve the single-file binary (downloading the macOS arm64 release on
//! demand) and run it to export a channel to JSON/HTML/CSV/plain text. The JSON
//! output is then imported through the existing [`crate::discord::import`] path.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

const DCE_VERSION: &str = "2.48";
const DCE_BINARY: &str = "DiscordChatExporter.Cli";
const DCE_ARCHIVE_URL: &str = "https://github.com/Tyrrrz/DiscordChatExporter/releases/download/2.48/DiscordChatExporter.Cli.osx-arm64.zip";

/// The four DCE export formats we expose, in display order.
pub const DCE_FORMATS: &[&str] = &["Json", "HtmlDark", "HtmlLight", "PlainText", "Csv"];

#[derive(Debug, Clone, serde::Serialize)]
pub struct DceStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

fn dce_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| format!("config dir: {e}"))?;
    let dir = dir.join("dce");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create dce dir: {e}"))?;
    Ok(dir)
}

fn dce_binary(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(dce_dir(app)?.join(DCE_BINARY))
}

/// Map a DCE format name to its file extension.
pub fn format_extension(format: &str) -> Result<&'static str, String> {
    match format {
        "Json" => Ok("json"),
        "HtmlDark" | "HtmlLight" => Ok("html"),
        "PlainText" => Ok("txt"),
        "Csv" => Ok("csv"),
        other => Err(format!(
            "Unknown export format \"{other}\"; expected one of: {}.",
            DCE_FORMATS.join(", ")
        )),
    }
}

/// Build the argument list for a single-channel export (pure, testable).
/// The token is deliberately absent: it is passed through the environment so it
/// never appears in the process table.
pub fn build_export_args(channel_id: &str, format: &str, out_file: &str) -> Vec<String> {
    vec![
        "export".to_string(),
        "-c".to_string(),
        channel_id.to_string(),
        "-f".to_string(),
        format.to_string(),
        "-o".to_string(),
        out_file.to_string(),
    ]
}

/// Report whether the DCE CLI is installed.
pub fn status(app: &AppHandle) -> DceStatus {
    let path = dce_binary(app).ok();
    let installed = path.as_ref().map(|p| p.exists()).unwrap_or(false);
    DceStatus {
        installed,
        version: if installed { Some(DCE_VERSION.to_string()) } else { None },
        path: if installed {
            path.map(|p| p.to_string_lossy().to_string())
        } else {
            None
        },
    }
}

/// Resolve the DCE binary, downloading and extracting it on first use.
pub async fn ensure_binary(app: &AppHandle) -> Result<PathBuf, String> {
    let binary = dce_binary(app)?;
    if binary.exists() {
        return Ok(binary);
    }

    let dir = dce_dir(app)?;
    let zip_path = dir.join("dce.zip");

    let bytes = reqwest::get(DCE_ARCHIVE_URL)
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    std::fs::write(&zip_path, &bytes).map_err(|e| format!("write zip: {e}"))?;

    let status = std::process::Command::new("unzip")
        .args(["-o", zip_path.to_str().unwrap_or("dce.zip"), "-d", dir.to_str().unwrap_or(".")])
        .output()
        .map_err(|e| format!("unzip failed (is `unzip` installed?): {e}"))?;
    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(format!("unzip failed: {}", stderr.trim()));
    }

    let _ = std::fs::remove_file(&zip_path);
    if !binary.exists() {
        return Err(format!("Archive did not contain {DCE_BINARY}."));
    }
    make_executable(&binary)?;
    Ok(binary)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| format!("chmod: {e}"))
}

/// Run a single-channel export and return the output file path.
pub async fn export_channel(
    app: &AppHandle,
    token: &str,
    channel_id: &str,
    format: &str,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let ext = format_extension(format)?;
    let binary = ensure_binary(app).await?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create out dir: {e}"))?;
    let out_file = out_dir.join(format!("{channel_id}.{ext}"));
    let args = build_export_args(channel_id, format, out_file.to_str().unwrap_or("export.out"));

    let output = tokio::process::Command::new(&binary)
        .args(&args)
        // DCE declares DISCORD_TOKEN as the environment fallback for `-t`, so
        // passing it here keeps the user's token out of `ps` and /proc.
        .env("DISCORD_TOKEN", token)
        .output()
        .await
        .map_err(|e| format!("failed to run DiscordChatExporter: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("DiscordChatExporter failed: {}", stderr.trim()));
    }
    if !out_file.exists() {
        return Err("DiscordChatExporter produced no output file.".to_string());
    }
    Ok(out_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_extensions() {
        assert_eq!(format_extension("Json").unwrap(), "json");
        assert_eq!(format_extension("HtmlDark").unwrap(), "html");
        assert_eq!(format_extension("PlainText").unwrap(), "txt");
        assert_eq!(format_extension("Csv").unwrap(), "csv");
        assert!(format_extension("bogus").is_err());
    }

    #[test]
    fn export_args_are_ordered_and_carry_no_token() {
        let args = build_export_args("123456", "Json", "/tmp/123456.json");
        assert_eq!(args[0], "export");
        assert_eq!(args[1], "-c");
        assert_eq!(args[2], "123456");
        assert_eq!(args[3], "-f");
        assert_eq!(args[4], "Json");
        assert_eq!(args[5], "-o");
        assert_eq!(args[6], "/tmp/123456.json");
        // The token is supplied through the environment, never as an argument.
        assert!(!args.iter().any(|a| a == "-t"), "{args:?}");
    }
}
