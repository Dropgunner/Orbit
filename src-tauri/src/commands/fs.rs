//! Filesystem commands used by the file tree, editor, and search UI.

use std::fs;
use std::path::Path;

use crate::types::{FileEntry, SearchMatch};

/// Lists the immediate children of a directory (directories first).
#[tauri::command]
pub fn list_dir(path: String) -> Result<Vec<FileEntry>, String> {
    let mut entries = fs::read_dir(&path)
        .map_err(|e| format!("read_dir {path}: {e}"))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let entry_path = entry.path();
            let metadata = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            Some(FileEntry {
                is_dir: metadata.is_dir(),
                name,
                path: entry_path.to_string_lossy().to_string(),
                size: metadata.len(),
                modified_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64),
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

/// Reads a UTF-8 text file.
#[tauri::command]
pub fn read_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))
}

/// Writes a UTF-8 text file, creating parent directories as needed.
#[tauri::command]
pub fn write_file(path: String, content: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
    }
    fs::write(&path, content).map_err(|e| format!("write {path}: {e}"))
}

/// Returns whether a path exists.
#[tauri::command]
pub fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

/// Searches file contents under a root directory (case-insensitive).
#[tauri::command]
pub fn search(root: String, query: String) -> Result<Vec<SearchMatch>, String> {
    crate::tools::search_sync(Path::new(&root), &query)
}
