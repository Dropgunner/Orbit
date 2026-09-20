//! Shared application state managed by Tauri.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::commands::terminal::Terminal;
use crate::types::PendingEdit;

/// Process-wide state. Each field is independently locked so unrelated
/// operations never contend on a single global lock.
pub struct AppState {
    /// Live terminal sessions keyed by id.
    pub terminals: Mutex<HashMap<String, Arc<Mutex<Terminal>>>>,
    /// Proposed file changes awaiting user review.
    pub pending_edits: Mutex<HashMap<String, PendingEdit>>,
    /// Monotonic counter for pending-edit ids.
    pub next_edit_id: Mutex<u64>,
    /// Cancellation flags for in-flight agent runs, keyed by run id.
    pub active_runs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            terminals: Mutex::new(HashMap::new()),
            pending_edits: Mutex::new(HashMap::new()),
            next_edit_id: Mutex::new(0),
            active_runs: Mutex::new(HashMap::new()),
        }
    }
}
