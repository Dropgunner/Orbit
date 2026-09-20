# Orbit

Orbit is an agentic AI coding IDE for macOS, built with [Tauri 2](https://tauri.app)
(Rust backend + React/TypeScript frontend). It pairs a chat-driven agent with a
real editor, terminal, and git, so the agent can read, edit, and run your
codebase and you review every change before it lands on disk.

![stack](https://img.shields.io/badge/Tauri-2-6c8cff) ![rust](https://img.shields.io/badge/Rust-stable-orange) ![ts](https://img.shields.io/badge/TypeScript-strict-3178c6)

## Features

- **Agent chat** — streamed, multi-turn conversation with an
  OpenAI-compatible model (DeepSeek by default; any compatible provider works).
- **Agent tools** — the model can list directories, read files, search the
  codebase, propose file edits, and run shell commands.
- **Reviewable edits** — file changes are *staged*, not applied. Review a
  unified diff and accept or reject each one (or accept all).
- **Editor** — multi-tab CodeMirror editor with syntax highlighting for
  JS/TS, JSON, Markdown, Python, Rust, CSS, and HTML, plus Cmd+S save.
- **File explorer** — lazy-loading directory tree.
- **Integrated terminal** — a real PTY (login shell) with multiple terminals.
- **Git** — status, stage/unstage, commit, unified diff, and log.

## Architecture

- `src-tauri/` — Rust backend.
  - `commands/` — Tauri commands for fs, terminal (PTY), git, agent, config.
  - `model.rs` — OpenAI-compatible streaming client with tool-call deltas.
  - `tools.rs` — the agent tool schemas and executors (workspace-scoped).
  - `agent.rs` — the tool-calling agent loop.
- `src/` — React/TypeScript frontend.
  - `ipc/contract.ts` — the typed mirror of the Rust serde types and events.
  - `ipc/invoke.ts` — typed `invoke` wrappers (the type-safe IPC boundary).
  - `state/store.ts` — zustand store; `state/events.ts` — event wiring.
  - `components/` — Header, Sidebar, FileTree, EditorPane, Terminal, ChatPanel,
    GitPanel, DiffView, StatusBar, SettingsModal.

The frontend and backend share one contract: every command and event is typed in
TypeScript and mirrored by serde structs in Rust, so the IPC boundary is
type-checked on both sides.

## Prerequisites

- macOS 10.15+ (Apple Silicon or Intel)
- [Node.js](https://nodejs.org) 22+ and [pnpm](https://pnpm.io) 10+
- [Rust](https://rustup.rs) stable
- Xcode Command Line Tools (`xcode-select --install`)

## Build & run (development)

```sh
pnpm install
pnpm tauri dev        # launch the app with the Vite dev server
```

## Build a release .app / .dmg

```sh
pnpm install
pnpm tauri build
```

Outputs land in `src-tauri/target/release/bundle/`:

- `macos/Orbit.app`
- `dmg/Orbit_<version>_aarch64.dmg`

The local build is ad-hoc signed. To distribute outside your machine you must
sign and notarize with an Apple Developer certificate (see "Distribution").

## Configuration

Orbit reads an OpenAI-compatible endpoint from Settings (the gear icon) or from
environment variables. Environment variables override the settings file.

| Variable | Default | Description |
| --- | --- | --- |
| `ORBIT_API_KEY` | — | API key (also read: `DEEPSEEK_API_KEY`) |
| `ORBIT_BASE_URL` | `https://api.deepseek.com` | Endpoint base URL |
| `ORBIT_MODEL` | `deepseek-chat` | Model name |
| `ORBIT_TEMPERATURE` | `0.2` | Sampling temperature |

The API key entered in Settings is stored in the app config directory
(`~/Library/Application Support/com.orbit.ide/`). Prefer the environment
variable if you do not want the key on disk.

## GitHub Actions

On every push, pull request, and `v*` tag, the
[`.github/workflows/build.yml`](.github/workflows/build.yml) workflow builds the
`.app` and `.dmg` on a macOS runner and uploads them as artifacts. Tagged
pushes also publish a GitHub release with the `.dmg` attached.

## Distribution (signing + notarization)

To produce a Gatekeeper-friendly app, add these secrets to the repository and
extend the workflow's build step:

- `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` — your Developer ID
  Application certificate (base64 of the `.p12`).
- `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` — App Store Connect
  credentials for `notarytool`.

Tauri's [macOS distribution guide](https://tauri.app/distribute/sign/macos/)
covers the full signing and notarization flow.

## Security notes

- File edits are confined to the opened project directory.
- The agent's `run_command` runs with your user privileges; review tool
  activity in the chat before accepting changes, and do not run Orbit on
  untrusted code without a sandbox.
- `html` rendering in chat is escaped; Markdown is rendered with raw HTML
  disabled.

## Roadmap

- Provider picker and per-model settings.
- Keychain storage for the API key.
- Streaming cancellation mid-token and edit-apply model (`edit_file` diff
  granularity per-hunk accept).
- Universal (arm64 + x86_64) builds and auto-updates.

## License

[MIT](LICENSE)
