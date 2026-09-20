//! Git commands implemented on top of the system `git` binary.

use std::process::Command;

use crate::types::{GitCommit, GitStatus, GitStatusEntry};

fn git(repo: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        return Err(if message.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            message.to_string()
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Returns the branch and the porcelain status entries for a repository.
#[tauri::command]
pub fn git_status(repo: String) -> Result<GitStatus, String> {
    let branch = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let raw = git(&repo, &["status", "--porcelain=v1"])?;
    let mut entries = Vec::new();
    for line in raw.lines() {
        if line.len() < 4 {
            continue;
        }
        let index = line[0..1].to_string();
        let worktree = line[1..2].to_string();
        let mut path = line[3..].trim().to_string();
        // Renames appear as "R  old -> new"; keep the destination name.
        if let Some((_old, new)) = path.split_once(" -> ") {
            path = new.to_string();
        }
        entries.push(GitStatusEntry {
            path,
            index,
            worktree,
        });
    }

    Ok(GitStatus { branch, entries })
}

/// Returns a unified diff (staged or unstaged).
#[tauri::command]
pub fn git_diff(repo: String, staged: bool) -> Result<String, String> {
    let args: &[&str] = if staged {
        &["diff", "--cached"]
    } else {
        &["diff"]
    };
    git(&repo, args)
}

/// Stages the given paths.
#[tauri::command]
pub fn git_stage(repo: String, paths: Vec<String>) -> Result<(), String> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    git(&repo, &args).map(|_| ())
}

/// Unstages the given paths.
#[tauri::command]
pub fn git_unstage(repo: String, paths: Vec<String>) -> Result<(), String> {
    let mut args = vec!["reset", "HEAD", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    git(&repo, &args).map(|_| ())
}

/// Commits staged changes with the given message and returns the new HEAD hash.
#[tauri::command]
pub fn git_commit(repo: String, message: String) -> Result<String, String> {
    git(&repo, &["commit", "-m", &message])?;
    git(&repo, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
}

/// Returns recent commits (newest first).
#[tauri::command]
pub fn git_log(repo: String, limit: usize) -> Result<Vec<GitCommit>, String> {
    let n = limit.clamp(1, 100).to_string();
    let raw = git(
        &repo,
        &[
            "log",
            "--pretty=format:%H%x1f%s%x1f%an%x1f%at",
            "-n",
            &n,
        ],
    )?;

    let mut commits = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split('\u{1f}').collect();
        if parts.len() >= 4 {
            commits.push(GitCommit {
                hash: parts[0].to_string(),
                subject: parts[1].to_string(),
                author: parts[2].to_string(),
                date_ms: parts[3].parse::<u64>().ok().map(|s| s * 1000),
            });
        }
    }
    Ok(commits)
}

/// Returns the current branch name.
#[tauri::command]
pub fn git_current_branch(repo: String) -> Result<String, String> {
    git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string())
}
