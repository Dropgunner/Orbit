//! Integrated terminal backed by a real pseudo-terminal (PTY).

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, State};

use crate::events::{TerminalEvent, TERMINAL_EVENT};
use crate::state::AppState;

/// A live terminal session. `master` owns the PTY for resizing; `writer`
/// is a handle used to feed keystrokes from the UI.
pub struct Terminal {
    pub child: Box<dyn Child + Send + Sync>,
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub alive: Arc<AtomicBool>,
}

fn next_id(state: &State<'_, AppState>) -> String {
    let n = state.terminals.lock().unwrap().len();
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("term-{n}-{millis}")
}

/// Spawns a login shell in a PTY and returns its terminal id.
#[tauri::command]
pub fn spawn_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
) -> Result<String, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    if shell.ends_with("zsh") || shell.ends_with("bash") {
        cmd.arg("-l");
    }
    cmd.env("TERM", "xterm-256color");
    if let Some(dir) = &cwd {
        cmd.cwd(dir.clone());
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn shell: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {e}"))?;

    let id = next_id(&state);
    let alive = Arc::new(AtomicBool::new(true));

    let reader_alive = alive.clone();
    let reader_app = app.clone();
    let reader_id = id.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            if !reader_alive.load(Ordering::Relaxed) {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = reader_app.emit(
                        TERMINAL_EVENT,
                        TerminalEvent::Data {
                            id: reader_id.clone(),
                            data,
                        },
                    );
                }
                Err(_) => break,
            }
        }
        let _ = reader_app.emit(
            TERMINAL_EVENT,
            TerminalEvent::Exit {
                id: reader_id,
                code: None,
            },
        );
    });

    let terminal = Terminal {
        child,
        master: pair.master,
        writer,
        alive,
    };
    state
        .terminals
        .lock()
        .unwrap()
        .insert(id.clone(), Arc::new(Mutex::new(terminal)));

    Ok(id)
}

/// Writes keystrokes to a terminal.
#[tauri::command]
pub fn write_terminal(state: State<'_, AppState>, id: String, data: String) -> Result<(), String> {
    let terminals = state.terminals.lock().unwrap();
    let terminal = terminals
        .get(&id)
        .ok_or_else(|| "terminal not found".to_string())?;
    let mut t = terminal.lock().unwrap();
    t.writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("write pty: {e}"))?;
    t.writer.flush().map_err(|e| format!("flush pty: {e}"))
}

/// Resizes a terminal's PTY.
#[tauri::command]
pub fn resize_terminal(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let terminals = state.terminals.lock().unwrap();
    let terminal = terminals
        .get(&id)
        .ok_or_else(|| "terminal not found".to_string())?;
    let t = terminal.lock().unwrap();
    t.master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize pty: {e}"))
}

/// Kills a terminal session.
#[tauri::command]
pub fn kill_terminal(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let terminal = state.terminals.lock().unwrap().remove(&id);
    if let Some(arc) = terminal {
        let mut t = arc.lock().unwrap();
        t.alive.store(false, Ordering::Relaxed);
        let _ = t.child.kill();
    }
    Ok(())
}
