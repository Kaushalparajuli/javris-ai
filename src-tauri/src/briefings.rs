//! Scheduled briefings: research that runs by itself on a schedule ("every weekday at 9, brief me
//! on AI news"). A background loop checks every 30 seconds, so briefings run while Jarvis sits in
//! the menu bar. A run missed while Jarvis was closed happens once, the next time it's open.

use crate::settings;
use crate::tasks::{self, Task};
use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Briefing {
    pub id: String,
    pub title: String,
    /// What to research each time.
    pub request: String,
    /// "daily", "weekdays" or "weekly".
    pub repeat: String,
    /// For weekly briefings: 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    /// Local time of day, "HH:MM" (24-hour).
    pub time: String,
    /// "quick" or "deep".
    pub depth: String,
    /// The conversation it belongs to; its runs show in that chat's task list.
    pub chat_id: String,
    pub enabled: bool,
    pub created_at: u64,
    pub last_run: Option<u64>,
    pub last_task: Option<u32>,
    /// When it runs next (ms since the epoch). Worked out here, never trusted from the UI.
    pub next_run: Option<u64>,
    /// Why the last scheduled run couldn't start (say Codex wasn't found); empty once one starts.
    pub last_error: String,
}

/// One writer at a time for briefings.json (the scheduler and the UI both save it).
static LOCK: Mutex<()> = Mutex::new(());

fn file(app: &AppHandle) -> PathBuf {
    settings::research_root(app).join("briefings.json")
}

fn load(app: &AppHandle) -> Vec<Briefing> {
    std::fs::read_to_string(file(app)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn store(app: &AppHandle, list: &[Briefing]) -> Result<(), String> {
    let path = file(app);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let _ = app.emit("briefings-changed", ());
    Ok(())
}

fn now_ms() -> u64 {
    Local::now().timestamp_millis().max(0) as u64
}

/// The first scheduled moment strictly after `after`, in local time.
pub fn next_run(b: &Briefing, after: DateTime<Local>) -> Option<DateTime<Local>> {
    next_time(&b.repeat, b.weekday, &b.time, after)
}

/// The first moment strictly after `after` on a "daily", "weekdays" or "weekly" schedule at
/// `time` ("HH:MM", local). Routines use the same schedules as briefings.
pub fn next_time(repeat: &str, on_weekday: u8, time: &str, after: DateTime<Local>) -> Option<DateTime<Local>> {
    let at = NaiveTime::parse_from_str(time.trim(), "%H:%M").ok()?;
    let mut day = after.date_naive();
    for _ in 0..9 {
        let weekday = day.weekday().num_days_from_monday() as u8;
        let wanted = match repeat {
            "weekdays" => weekday < 5,
            "weekly" => weekday == on_weekday,
            _ => true,
        };
        // A time that doesn't exist on a clock-change day moves to the next day.
        if let Some(when) = Local.from_local_datetime(&day.and_time(at)).earliest() {
            if wanted && when > after {
                return Some(when);
            }
        }
        day = day.succ_opt()?;
    }
    None
}

fn schedule(b: &mut Briefing, after: DateTime<Local>) {
    b.next_run = if b.enabled { next_run(b, after).map(|t| t.timestamp_millis().max(0) as u64) } else { None };
}

fn validate(b: &Briefing) -> Result<(), String> {
    if b.request.trim().is_empty() {
        return Err("Say what the briefing should cover.".into());
    }
    if NaiveTime::parse_from_str(b.time.trim(), "%H:%M").is_err() {
        return Err("The time should look like 09:00.".into());
    }
    if !["daily", "weekdays", "weekly"].contains(&b.repeat.as_str()) {
        return Err("Repeat must be daily, weekdays or weekly.".into());
    }
    if b.weekday > 6 {
        return Err("The day of the week is out of range.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_briefings(app: AppHandle) -> Vec<Briefing> {
    let mut list = load(&app);
    list.sort_by_key(|b| (!b.enabled, b.next_run.unwrap_or(u64::MAX)));
    list
}

/// Create a briefing (empty id) or update one. Returns it with its next run worked out.
#[tauri::command]
pub fn save_briefing(app: AppHandle, briefing: Briefing) -> Result<Briefing, String> {
    validate(&briefing)?;
    let _guard = LOCK.lock().unwrap();
    let mut list = load(&app);
    let mut b = briefing;
    b.title = if b.title.trim().is_empty() { tasks_title(&b.request) } else { b.title.trim().to_string() };
    b.time = b.time.trim().to_string();
    if b.depth != "deep" {
        b.depth = "quick".into();
    }
    if let Some(old) = list.iter().find(|x| x.id == b.id && !b.id.is_empty()) {
        b.created_at = old.created_at;
        b.last_run = old.last_run;
        b.last_task = old.last_task;
        b.last_error = old.last_error.clone();
    } else {
        b.id = format!("b{}", now_ms());
        b.created_at = now_ms();
    }
    schedule(&mut b, Local::now());
    list.retain(|x| x.id != b.id);
    list.push(b.clone());
    store(&app, &list)?;
    Ok(b)
}

#[tauri::command]
pub fn delete_briefing(app: AppHandle, id: String) -> Result<(), String> {
    let _guard = LOCK.lock().unwrap();
    let mut list = load(&app);
    let before = list.len();
    list.retain(|b| b.id != id);
    if list.len() == before {
        return Err("There's no such briefing.".into());
    }
    store(&app, &list)
}

fn tasks_title(request: &str) -> String {
    let words: Vec<&str> = request.split_whitespace().take(6).collect();
    words.join(" ")
}

/// Start one run of a briefing as a research task.
fn start(app: &AppHandle, b: &Briefing) -> Result<Task, String> {
    let since = b
        .last_run
        .and_then(|ms| Local.timestamp_millis_opt(ms as i64).single())
        .map(|t| format!(" Focus on what's new since {}.", t.format("%A %-d %B at %H:%M")))
        .unwrap_or_default();
    let request = format!(
        "This is a scheduled briefing: {}.{since} Lead with the three to five most important developments, then the details, with sources.",
        b.request.trim().trim_end_matches('.')
    );
    let title = format!("Briefing: {} · {}", b.title, Local::now().format("%-d %b"));
    tasks::start_task(app.clone(), title, request, b.depth.clone(), Some(b.chat_id.clone()), None)
}

#[tauri::command]
pub fn run_briefing_now(app: AppHandle, id: String) -> Result<Task, String> {
    let _guard = LOCK.lock().unwrap();
    let mut list = load(&app);
    let b = list.iter_mut().find(|b| b.id == id).ok_or("There's no such briefing.")?;
    let task = start(&app, b)?;
    b.last_run = Some(now_ms());
    b.last_task = Some(task.id);
    b.last_error.clear();
    store(&app, &list)?;
    Ok(task)
}

/// Run briefings that are due, then work out when each runs next.
fn tick(app: &AppHandle) {
    let _guard = LOCK.lock().unwrap();
    let mut list = load(app);
    let now = Local::now();
    let mut changed = false;
    for b in list.iter_mut() {
        if !b.enabled {
            continue;
        }
        // Older files may not have a next run yet.
        if b.next_run.is_none() {
            schedule(b, now);
            changed = true;
            continue;
        }
        if b.next_run.is_some_and(|t| t <= now.timestamp_millis().max(0) as u64) {
            match start(app, b) {
                Ok(task) => {
                    b.last_run = Some(now_ms());
                    b.last_task = Some(task.id);
                    b.last_error.clear();
                }
                // Kept on the briefing so the Briefings page can say so (saving tells it to reload).
                Err(e) => {
                    eprintln!("Briefing {:?} couldn't start: {e}", b.title);
                    b.last_error = e;
                }
            }
            // Missed runs collapse into this one: the next run is worked out from now.
            schedule(b, now);
            changed = true;
        }
    }
    if changed {
        let _ = store(app, &list);
    }
}

/// The background loop, started when the app launches.
pub async fn run_scheduler(app: AppHandle) {
    // Give the window a moment to load before any catch-up run starts.
    tokio::time::sleep(Duration::from_secs(8)).await;
    loop {
        let handle = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || tick(&handle)).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest().unwrap()
    }
    fn b(repeat: &str, time: &str, weekday: u8) -> Briefing {
        Briefing { repeat: repeat.into(), time: time.into(), weekday, enabled: true, ..Default::default() }
    }

    #[test]
    fn works_out_the_next_run() {
        // 2026-09-29 is a Tuesday.
        let tue_8am = at(2026, 9, 29, 8, 0);
        assert_eq!(next_run(&b("daily", "09:00", 0), tue_8am), Some(at(2026, 9, 29, 9, 0)), "later today");
        assert_eq!(next_run(&b("daily", "07:30", 0), tue_8am), Some(at(2026, 9, 30, 7, 30)), "tomorrow");
        assert_eq!(next_run(&b("weekly", "09:00", 0), tue_8am), Some(at(2026, 10, 5, 9, 0)), "next Monday");
        let fri_10am = at(2026, 10, 2, 10, 0);
        assert_eq!(next_run(&b("weekdays", "09:00", 0), fri_10am), Some(at(2026, 10, 5, 9, 0)), "skips the weekend");
        assert_eq!(next_run(&b("daily", "9am", 0), tue_8am), None, "bad time");
    }

    #[test]
    fn rejects_bad_briefings() {
        let mut x = b("daily", "09:00", 0);
        assert!(validate(&x).is_err(), "no request");
        x.request = "AI news".into();
        assert!(validate(&x).is_ok());
        x.repeat = "hourly".into();
        assert!(validate(&x).is_err());
    }
}
