//! After a code task finishes, run the project's own checks (tests, type check, build) so "done"
//! means "done and passing". The commands are the ones the user wrote in the workspace's settings,
//! run in its folder; nothing Codex or a web page suggests is ever run here.

use crate::tasks::{self, Status};
use crate::workspaces;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Longest a single check may run.
const LIMIT: Duration = Duration::from_secs(300);
/// How much of a check's output is kept, from the end (where the errors are).
const KEEP: usize = 3000;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Verified {
    pub task: tasks::Task,
    pub ok: bool,
    /// One line per check, then the output of the ones that failed.
    pub text: String,
}

/// The check commands of the workspace whose folder is `project`, if there is one.
pub fn commands_for(app: &AppHandle, project: &str) -> Vec<String> {
    let want = PathBuf::from(project).canonicalize().unwrap_or_else(|_| PathBuf::from(project));
    workspaces::list_workspaces(app.clone())
        .into_iter()
        .find(|w| !w.folder.is_empty() && PathBuf::from(&w.folder).canonicalize().map(|f| f == want).unwrap_or(false))
        .map(|w| w.verify)
        .unwrap_or_default()
}

fn tail(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= KEEP {
        return s.to_string();
    }
    let skip = s.chars().count() - KEEP;
    format!("…{}", s.chars().skip(skip).collect::<String>())
}

/// Run one command through the user's login shell (so node, cargo and friends are on the PATH).
async fn run_one(cmd: &str, dir: &Path) -> (bool, String) {
    let child = Command::new("/bin/zsh")
        .args(["-lc", cmd])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (false, format!("Couldn't start: {e}")),
    };
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let read = async {
        let (mut a, mut b) = (String::new(), String::new());
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut a).await;
        }
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut b).await;
        }
        let status = child.wait().await;
        (status, format!("{a}\n{b}"))
    };
    match tokio::time::timeout(LIMIT, read).await {
        Ok((Ok(status), text)) => (status.success(), tail(&text)),
        Ok((Err(e), text)) => (false, format!("{}\n{e}", tail(&text))),
        Err(_) => (false, format!("Took longer than {} minutes, so it was stopped.", LIMIT.as_secs() / 60)),
    }
}

/// Whether this just-finished task should be checked, and with what.
pub fn plan(app: &AppHandle, task: &tasks::Task) -> Vec<String> {
    if task.kind != "code" || task.status != Status::Done || task.project.is_empty() {
        return vec![];
    }
    commands_for(app, &task.project)
}

/// Run the checks and report. Called after the task is marked finished with `verify = "running"`.
pub async fn run(app: AppHandle, id: u32, commands: Vec<String>) {
    let Some(task) = tasks::get_task(&app, id) else { return };
    let dir = PathBuf::from(&task.project);
    let mut lines = vec![];
    let mut details = vec![];
    let mut ok = true;
    for cmd in &commands {
        let (passed, output) = run_one(cmd, &dir).await;
        lines.push(format!("{} {cmd}", if passed { "passed:" } else { "FAILED:" }));
        if !passed {
            ok = false;
            details.push(format!("$ {cmd}\n{output}"));
            // Later checks usually depend on this one (build before test), so stop at the first failure.
            break;
        }
    }
    let skipped = commands.len().saturating_sub(lines.len());
    if skipped > 0 {
        lines.push(format!("(not run: {skipped} more after the failure)"));
    }
    let text = if details.is_empty() { lines.join("\n") } else { format!("{}\n\n{}", lines.join("\n"), details.join("\n\n")) };

    let done = tasks::set_verification(&app, id, if ok { "passed" } else { "failed" }, &text);
    if let Some(task) = done {
        let _ = app.emit("task-update", &task);
        let _ = app.emit("task-verified", &Verified { task, ok, text });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passing_and_failing_commands_are_told_apart() {
        let dir = std::env::temp_dir();
        let (ok, _) = run_one("echo fine", &dir).await;
        assert!(ok);
        let (ok, out) = run_one("echo broken >&2; exit 3", &dir).await;
        assert!(!ok);
        assert!(out.contains("broken"));
    }

    #[test]
    fn long_output_keeps_its_end() {
        let long = format!("{}END", "x".repeat(10_000));
        let t = tail(&long);
        assert!(t.ends_with("END"));
        assert!(t.chars().count() <= KEEP + 1);
    }
}
