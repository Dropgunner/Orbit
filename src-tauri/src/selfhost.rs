//! Self-hosting support: detect when the agent's workspace *is* the repository
//! that builds Orbit, and give the agent the project-specific knowledge it needs
//! to work on Orbit itself ("code Orbit directly into Orbit").
//!
//! Detection is pure (filesystem probes only) so it is unit-testable and can
//! never mutate the workspace. Nothing here is model-visible unless the
//! workspace genuinely looks like the Orbit checkout.

use std::path::Path;

/// A workspace that builds Orbit, plus how to verify a change in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfHost {
    /// Human-readable name of the detected project.
    pub project: String,
    /// Whether the Tauri desktop app (Rust + React) lives in this workspace.
    pub has_desktop_app: bool,
    /// Whether the TypeSafe profiler CLI (Node/TS) lives in this workspace.
    pub has_profiler_cli: bool,
    /// Whether the Harness plugin bundle lives in this workspace.
    pub has_plugin: bool,
    /// The command that proves a change is good, in priority order.
    pub verify_commands: Vec<VerifyCommand>,
}

/// One verification command the chain runner can execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCommand {
    /// Short label for progress events ("typecheck", "rust tests").
    pub label: String,
    /// Shell command, run in the workspace root.
    pub command: String,
}

/// The `src-tauri` directory that identifies a Tauri app, given a workspace.
fn desktop_manifest(workspace: &Path) -> Option<std::path::PathBuf> {
    let direct = workspace.join("src-tauri").join("tauri.conf.json");
    if direct.is_file() {
        return Some(direct);
    }
    // The meta-repo layout: the Tauri app sits one level down in `orbit/`.
    let nested = workspace
        .join("orbit")
        .join("src-tauri")
        .join("tauri.conf.json");
    if nested.is_file() {
        return Some(nested);
    }
    None
}

/// Detects a self-hosting workspace.
///
/// Returns `Some` only when the workspace contains the Tauri desktop app, which
/// is what makes "the agent can change Orbit" true. The profiler CLI and plugin
/// flags refine the verification plan but are not required.
pub fn detect(workspace: &Path) -> Option<SelfHost> {
    let manifest = desktop_manifest(workspace)?;
    let tauri_dir = manifest.parent()?.to_path_buf();
    let app_dir = tauri_dir.parent()?.to_path_buf();
    // The repository root owns `orbit/` when the app is nested inside it.
    let repo_root = if app_dir.ends_with("orbit") {
        app_dir.parent().unwrap_or(&app_dir).to_path_buf()
    } else {
        app_dir.clone()
    };
    // Where the app's own package.json lives, and how to point a command at it
    // from the repository root the verifier runs in.
    let nested_app = repo_root != app_dir;
    let app_prefix = if nested_app { "orbit/" } else { "" };
    let manifest_path = format!("{app_prefix}src-tauri/Cargo.toml");

    let has_profiler_cli = repo_root.join("src").join("index.ts").is_file()
        && repo_root.join("package.json").is_file();
    let has_plugin = repo_root.join("plugin").join("index.ts").is_file();

    let mut verify_commands = Vec::new();
    if app_dir.join("package.json").is_file() {
        // Self-contained: each command sets its own directory, so the verifier
        // can run any of them straight from the repository root.
        verify_commands.push(VerifyCommand {
            label: "frontend typecheck".to_string(),
            command: if nested_app {
                "cd orbit && npm run typecheck".to_string()
            } else {
                "npm run typecheck".to_string()
            },
        });
    }
    if has_profiler_cli {
        verify_commands.push(VerifyCommand {
            label: "cli typecheck".to_string(),
            command: "npm run typecheck".to_string(),
        });
    }
    verify_commands.push(VerifyCommand {
        label: "rust check".to_string(),
        command: format!("cargo check --manifest-path {manifest_path}"),
    });
    verify_commands.push(VerifyCommand {
        label: "rust tests".to_string(),
        command: format!("cargo test --manifest-path {manifest_path} --lib"),
    });

    Some(SelfHost {
        project: "Orbit".to_string(),
        has_desktop_app: true,
        has_profiler_cli,
        has_plugin,
        verify_commands,
    })
}

impl SelfHost {
    /// The system-prompt section that turns a generic coding agent into one that
    /// knows this repository's layout and verification commands.
    pub fn prompt_section(&self) -> String {
        let mut out = String::new();
        out.push_str("This workspace IS the source of Orbit itself. You are editing the very app you are running in.\n");
        out.push_str("Layout:\n");
        out.push_str("- orbit/            Tauri 2 + React 18 + Vite desktop app (the UI you are looking at)\n");
        out.push_str("- orbit/src/        React frontend (components/, ipc/, state/)\n");
        out.push_str("- orbit/src-tauri/  Rust backend (agent.rs, tools.rs, model.rs, web.rs)\n");
        if self.has_profiler_cli {
            out.push_str("- src/              TypeSafe profiler CLI (Node/TS), built with `npm run build`\n");
        }
        if self.has_plugin {
            out.push_str("- plugin/           DeepSeek Harness plugin bundle wrapping the same pipeline\n");
        }
        out.push_str("Rules for self-hosting changes:\n");
        out.push_str("- The frontend is embedded at build time: a React/CSS change is invisible until `orbit && npm run build` runs AND the Rust app is rebuilt/starts from the new `orbit/dist`.\n");
        out.push_str("- Keep the Rust and TypeScript command surfaces in sync: a new `#[tauri::command]` must also be registered in `lib.rs` and, if the browser UI needs it, dispatched in `web.rs`.\n");
        out.push_str("- Verify with the project's own commands rather than reasoning about correctness:\n");
        for command in &self.verify_commands {
            out.push_str(&format!("    {}  ->  {}\n", command.label, command.command));
        }
        out.push_str("- Prefer incremental, compiling changes over sweeping rewrites; a broken build blocks every later step.\n");
        out
    }
}

/// The system prompt for an ordinary (non-self-hosting) workspace.
pub fn default_prompt_section(workspace: &Path) -> String {
    format!(
        "You are working in the project at: {}\n",
        workspace.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Creates a unique scratch directory for a test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orbit-selfhost-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, "{}").unwrap();
    }

    #[test]
    fn detects_nested_orbit_layout() {
        let dir = scratch("nested");
        touch(&dir.join("orbit/src-tauri/tauri.conf.json"));
        touch(&dir.join("orbit/package.json"));
        touch(&dir.join("package.json"));
        touch(&dir.join("src/index.ts"));
        touch(&dir.join("plugin/index.ts"));

        let detected = detect(&dir).expect("should detect self-hosting");
        assert_eq!(detected.project, "Orbit");
        assert!(detected.has_desktop_app);
        assert!(detected.has_profiler_cli);
        assert!(detected.has_plugin);
        // The app's own typecheck runs inside orbit/, and cargo is pointed at
        // the nested manifest, so both work from the repository root.
        assert!(detected
            .verify_commands
            .iter()
            .any(|c| c.command == "cd orbit && npm run typecheck"));
        assert!(detected
            .verify_commands
            .iter()
            .any(|c| c.command.contains("cargo check --manifest-path orbit/src-tauri")));
    }

    #[test]
    fn detects_direct_tauri_layout() {
        let dir = scratch("direct");
        touch(&dir.join("src-tauri/tauri.conf.json"));
        touch(&dir.join("package.json"));

        let detected = detect(&dir).expect("should detect self-hosting");
        assert!(detected.has_desktop_app);
        assert!(!detected.has_profiler_cli);
        // Not nested: the app typecheck runs bare, and there is no separate
        // profiler-CLI typecheck to add.
        assert_eq!(
            detected
                .verify_commands
                .iter()
                .filter(|c| c.command.contains("npm run typecheck"))
                .count(),
            1
        );
        assert!(detected
            .verify_commands
            .iter()
            .any(|c| c.command.contains("cargo check --manifest-path src-tauri")));
    }

    #[test]
    fn ignores_plain_workspace() {
        let dir = scratch("plain");
        touch(&dir.join("package.json"));
        touch(&dir.join("src/index.ts"));
        assert!(detect(&dir).is_none());
    }

    #[test]
    fn prompt_mentions_embedded_frontend_trap() {
        let dir = scratch("prompt");
        touch(&dir.join("orbit/src-tauri/tauri.conf.json"));
        let detected = detect(&dir).unwrap();
        let prompt = detected.prompt_section();
        assert!(prompt.contains("source of Orbit"));
        assert!(prompt.contains("embedded at build time"));
        assert!(prompt.contains("cargo check"));
    }
}
