//! Drive the real DeepSeek Harness (`dsh`) from Orbit.
//!
//! Orbit has its own Rust agent loop; this module adds the second, heavier
//! option — handing a task to the installed Harness CLI in its one-shot
//! `headless` profile and streaming its output back into the UI. The harness
//! brings its own tool set, plugin bundles, and profile configuration, so the
//! two paths are deliberately independent: nothing here re-enters Orbit's
//! command layer.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::events::{DshEvent, DSH_EVENT};

/// The Harness profile used for one-shot tasks.
pub const HEADLESS_PROFILE: &str = "headless";

/// Where the Harness may live, in PATH order.
const CANDIDATE_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    ".local/bin",
];

/// The located Harness executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Harness {
    pub program: PathBuf,
}

/// A report on whether the Harness is usable from this machine.
#[derive(Debug, Clone, Serialize)]
pub struct DshStatus {
    /// True when a `dsh` executable was found.
    pub available: bool,
    /// Absolute path to the executable, when found.
    pub program: Option<String>,
    /// The profile one-shot tasks are run with.
    pub profile: String,
    /// A message for the UI when the harness is missing.
    pub note: String,
}

/// Finds the `dsh` executable: an explicit override, then `PATH`, then the
/// usual install directories. Returns `None` when the Harness is not installed.
pub fn locate(override_path: Option<&str>) -> Option<Harness> {
    if let Some(raw) = override_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if is_executable_file(&candidate) {
                return Some(Harness { program: candidate });
            }
        }
    }

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("dsh");
            if is_executable_file(&candidate) {
                return Some(Harness { program: candidate });
            }
        }
    }

    for dir in CANDIDATE_DIRS {
        let base = if let Some(rest) = dir.strip_prefix(".local/") {
            match std::env::var_os("HOME") {
                Some(home) => PathBuf::from(home).join(".local").join(rest),
                None => continue,
            }
        } else {
            PathBuf::from(dir)
        };
        let candidate = base.join("dsh");
        if is_executable_file(&candidate) {
            return Some(Harness { program: candidate });
        }
    }

    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The argv for a one-shot harness task.
pub fn headless_args(prompt: &str) -> Vec<String> {
    vec![
        "--profile".to_string(),
        HEADLESS_PROFILE.to_string(),
        prompt.to_string(),
    ]
}

/// Reports whether the Harness can be driven from here.
pub fn status(app: &AppHandle) -> DshStatus {
    let override_path = crate::commands::config::resolve(app)
        .ok()
        .and_then(|settings| settings.dsh_path);
    match locate(override_path.as_deref()) {
        Some(harness) => DshStatus {
            available: true,
            program: Some(harness.program.to_string_lossy().to_string()),
            profile: HEADLESS_PROFILE.to_string(),
            note: String::new(),
        },
        None => DshStatus {
            available: false,
            program: None,
            profile: HEADLESS_PROFILE.to_string(),
            note: "The DeepSeek Harness CLI (`dsh`) was not found on PATH. Install it, or set the harness path in Settings.".to_string(),
        },
    }
}

/// Runs one harness task, streaming output on `orbit://dsh` until it exits.
pub async fn run(
    app: AppHandle,
    run_id: String,
    prompt: String,
    workspace: PathBuf,
    cancel: Arc<AtomicBool>,
) {
    let override_path = crate::commands::config::resolve(&app)
        .ok()
        .and_then(|settings| settings.dsh_path);
    let Some(harness) = locate(override_path.as_deref()) else {
        let _ = app.emit(
            DSH_EVENT,
            DshEvent::Error {
                run_id,
                error: "The DeepSeek Harness CLI (`dsh`) was not found. Install it, or set its path in Settings.".to_string(),
            },
        );
        return;
    };

    let mut command = tokio::process::Command::new(&harness.program);
    command
        .args(headless_args(&prompt))
        .current_dir(&workspace)
        .env("TERM", "dumb")
        .env("CI", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = app.emit(
                DSH_EVENT,
                DshEvent::Error {
                    run_id,
                    error: format!("could not start {}: {error}", harness.program.display()),
                },
            );
            return;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Both pipes are drained on their own tasks so a chatty run cannot deadlock
    // on a full pipe buffer.
    let mut readers = Vec::new();
    if let Some(stdout) = stdout {
        readers.push(spawn_reader(app.clone(), run_id.clone(), stdout));
    }
    if let Some(stderr) = stderr {
        readers.push(spawn_reader(app.clone(), run_id.clone(), stderr));
    }

    let cancel_for_wait = cancel.clone();
    let code = loop {
        if cancel_for_wait.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            break None;
        }
        match tokio::time::timeout(std::time::Duration::from_millis(250), child.wait()).await {
            Ok(Ok(status)) => break status.code(),
            Ok(Err(_)) => break None,
            Err(_) => continue,
        }
    };

    for reader in readers {
        let _ = reader.await;
    }

    let _ = app.emit(DSH_EVENT, DshEvent::Exit { run_id, code });
}

/// Streams a child pipe line by line onto the harness event channel.
fn spawn_reader<R>(app: AppHandle, run_id: String, stream: R) -> tauri::async_runtime::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app.emit(
                DSH_EVENT,
                DshEvent::Line {
                    run_id: run_id.clone(),
                    line,
                },
            );
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_args_use_the_headless_profile() {
        let args = headless_args("fix the failing test");
        assert_eq!(args[0], "--profile");
        assert_eq!(args[1], "headless");
        assert_eq!(args[2], "fix the failing test");
    }

    #[test]
    fn locate_prefers_an_explicit_override() {
        // /bin/sh is executable everywhere this runs; the point is that the
        // override wins over PATH probing.
        let found = locate(Some("/bin/sh"));
        assert_eq!(found.map(|h| h.program), Some(PathBuf::from("/bin/sh")));
    }

    #[test]
    fn locate_ignores_non_executable_override_and_falls_back() {
        // A directory is not an executable, so the override must be skipped.
        // What happens next depends on whether the real harness is installed,
        // so assert only that the directory itself is never returned.
        let dir = std::env::temp_dir().join("orbit-dsh-dir-test");
        let _ = std::fs::create_dir_all(&dir);
        if let Some(harness) = locate(Some(dir.to_string_lossy().as_ref())) {
            assert_ne!(harness.program, dir);
        }
    }

    #[test]
    fn locate_rejects_a_missing_path() {
        let missing = "/definitely/not/here/dsh";
        if let Some(harness) = locate(Some(missing)) {
            assert_ne!(harness.program, PathBuf::from(missing));
        }
    }

    #[test]
    fn is_executable_file_distinguishes_files_from_directories() {
        let dir = std::env::temp_dir().join("orbit-dsh-exec-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_executable_file(&dir), "a directory is not executable");

        let plain = dir.join("plain.txt");
        std::fs::write(&plain, "x").unwrap();
        assert!(
            !is_executable_file(&plain),
            "a non-executable file must be rejected"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exec = dir.join("tool");
            std::fs::write(&exec, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&exec, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(is_executable_file(&exec), "a 0755 file is executable");
        }

        assert!(!is_executable_file(&dir.join("nothing-here")));
    }
}
