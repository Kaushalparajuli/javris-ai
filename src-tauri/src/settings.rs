use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// User settings, stored as JSON in the app's config folder
/// (~/Library/Application Support/co.fikraventures.jarvis/settings.json).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub gemini_api_key: String,
    /// Empty means "pick the newest Live model automatically".
    pub gemini_model: String,
    pub voice: String,
    /// Language Jarvis speaks by default, as a BCP-47 code. Empty means
    /// "match whoever is talking".
    pub language: String,
    pub user_name: String,
    /// Empty means "look in the usual install locations".
    pub codex_path: String,
    /// Empty means ~/Jarvis.
    pub research_dir: String,
    /// Empty means Codex's own default model.
    pub codex_model: String,
    /// "auto" (quick → low, deep → high), "default" (Codex's config), or a level like "medium".
    pub codex_reasoning: String,
    pub web_search: bool,
    /// With headphones Jarvis can hear you while it talks, so you can interrupt by voice.
    pub headphones: bool,
    /// Microphone name; empty means the macOS default input.
    pub mic_device: String,
    /// Show a system notification when worker tasks finish while Jarvis isn't in front.
    pub notify: bool,
    /// Listen for "hey Jarvis" on this computer. Off until turned on in Settings.
    pub wake_word: bool,
    /// The user's own Google OAuth client (a "Desktop app" client from Google Cloud Console),
    /// for Calendar and Gmail. Google treats desktop client secrets as not really secret.
    pub google_client_id: String,
    pub google_client_secret: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gemini_api_key: String::new(),
            gemini_model: String::new(),
            voice: "Charon".into(),
            language: String::new(),
            user_name: String::new(),
            codex_path: String::new(),
            research_dir: String::new(),
            codex_model: String::new(),
            codex_reasoning: "auto".into(),
            web_search: true,
            headphones: false,
            mic_device: String::new(),
            notify: true,
            wake_word: false,
            google_client_id: String::new(),
            google_client_secret: String::new(),
        }
    }
}

fn settings_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    settings_file(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn home(app: &AppHandle) -> PathBuf {
    app.path().home_dir().unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Folder that holds research tasks, notes and the task index.
pub fn research_root(app: &AppHandle) -> PathBuf {
    let s = load(app);
    let dir = s.research_dir.trim();
    if dir.is_empty() {
        home(app).join("Jarvis")
    } else if let Some(rest) = dir.strip_prefix("~/") {
        home(app).join(rest)
    } else {
        PathBuf::from(dir)
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    load(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let path = settings_file(&app).ok_or("No config folder available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
