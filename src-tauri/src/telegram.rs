//! Telegram remote: the owner talks to Jarvis from their phone through their own Telegram bot.
//!
//! The owner makes a bot with @BotFather and pastes its token in Settings. Pairing ties the bot to
//! one chat: Jarvis shows a six-digit code, the owner sends "/start <code>" to the bot, and that
//! chat's id is saved. From then on only that chat is listened to; every other chat is told once
//! that the bot is private and otherwise ignored. That chat-id check is the security boundary.
//!
//! Messages are fetched with long polling (getUpdates), so nothing on this computer is exposed to
//! the internet. Text from the owner goes to the UI as `telegram-message`, button presses as
//! `telegram-callback`; the UI decides what Jarvis does with them.

use crate::settings::{self, Settings};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const API: &str = "https://api.telegram.org";
/// Telegram's longest message, in UTF-16 units.
const MAX_MESSAGE: usize = 4096;
/// Telegram's longest caption on a file.
const MAX_CAPTION: usize = 1024;
/// Bots may send files up to 50 MB.
const MAX_FILE: u64 = 50 * 1024 * 1024;
/// A pairing code is good for ten minutes and a handful of guesses.
const PAIR_TTL: Duration = Duration::from_secs(600);
const PAIR_TRIES: u32 = 5;
/// Messages older than this when Jarvis sees them were sent while it was off; they aren't acted on.
const STALE_SECS: u64 = 600;

struct Pairing {
    code: String,
    until: Instant,
    tries: u32,
}

static PAIRING: Mutex<Option<Pairing>> = Mutex::new(None);
static LOOP: Mutex<Option<tauri::async_runtime::JoinHandle<()>>> = Mutex::new(None);
/// The bot's display name and @username, from getMe.
static BOT: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));
/// Strangers already told the bot is private, so they're told only once.
static TOLD: Mutex<Option<HashSet<i64>>> = Mutex::new(None);
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

// ---------- plain helpers (tested) ----------

/// Text made safe for Telegram's HTML mode.
pub(crate) fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escaped_len(c: char) -> usize {
    match c {
        '&' => 5,
        '<' | '>' => 4,
        c => c.len_utf16(),
    }
}

/// Split text into pieces that each fit one message once escaped, breaking at a line end or a space
/// where one is reasonably close. Pieces are returned unescaped.
pub(crate) fn split_message(text: &str, limit: usize) -> Vec<String> {
    let mut out = vec![];
    let mut rest: &str = text.trim();
    while !rest.is_empty() {
        let mut used = 0;
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if used + escaped_len(c) > limit {
                end = i;
                break;
            }
            used += escaped_len(c);
        }
        if end < rest.len() {
            // Prefer a line break, then a space, in the last half of the piece.
            let head = &rest[..end];
            let floor = end / 2;
            if let Some(at) = head.rfind('\n').filter(|&a| a > floor).or_else(|| head.rfind(' ').filter(|&a| a > floor)) {
                end = at + 1;
            }
        }
        let piece = rest[..end].trim_end();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        rest = rest[end..].trim_start();
    }
    out
}

/// Cut text to `limit` UTF-16 units once escaped, adding an ellipsis when cut.
fn clip(text: &str, limit: usize) -> String {
    match split_message(text, limit.saturating_sub(1)).into_iter().next() {
        Some(first) if first.len() < text.trim().len() => format!("{first}…"),
        Some(first) => first,
        None => String::new(),
    }
}

/// A fresh six-digit code.
fn new_code() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    format!("{:06}", u32::from_le_bytes(b) % 1_000_000)
}

/// The code in "/start 123456" (or "/start@MyBot 123456"), if the text is a start command.
fn start_code(text: &str) -> Option<&str> {
    let mut words = text.split_whitespace();
    let cmd = words.next()?;
    if cmd != "/start" && !cmd.starts_with("/start@") {
        return None;
    }
    Some(words.next().unwrap_or(""))
}

/// Same length and same bytes, compared without stopping early.
fn same_code(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, PartialEq)]
enum PairCheck {
    Paired,
    Wrong,
    /// No code is waiting (none was made, it expired, or it was used up by wrong guesses).
    NoCode,
}

/// Check a code someone sent against the one waiting. Wrong guesses use up the code.
fn check_pairing(slot: &mut Option<Pairing>, sent: &str, now: Instant) -> PairCheck {
    let Some(p) = slot.as_mut() else { return PairCheck::NoCode };
    if now >= p.until {
        *slot = None;
        return PairCheck::NoCode;
    }
    if same_code(&p.code, sent.trim()) {
        *slot = None;
        return PairCheck::Paired;
    }
    p.tries += 1;
    if p.tries >= PAIR_TRIES {
        *slot = None;
    }
    PairCheck::Wrong
}

/// What one update from Telegram asks of Jarvis.
#[derive(Debug, PartialEq)]
enum Incoming {
    /// "/start <code>" in a private chat while not paired.
    PairAttempt { chat: i64, code: String },
    Text { chat: i64, message_id: i64, text: String, date: u64 },
    /// A photo, voice note, sticker… from the owner.
    Unsupported { chat: i64 },
    Callback { query_id: String, chat: i64, message_id: i64, data: String },
    /// Someone who isn't the owner.
    Stranger { chat: i64, callback: Option<String> },
    Ignore,
}

/// Sort an update by who sent it. Anything not from `paired` (0 = not paired yet) is a stranger,
/// except a start command in a private chat, which may be the owner pairing.
fn classify(update: &Value, paired: i64) -> Incoming {
    if let Some(q) = update.get("callback_query") {
        let chat = q["message"]["chat"]["id"].as_i64().unwrap_or(0);
        let from = q["from"]["id"].as_i64().unwrap_or(0);
        let query_id = q["id"].as_str().unwrap_or("").to_string();
        if paired == 0 || chat != paired || from != paired {
            return Incoming::Stranger { chat: if chat != 0 { chat } else { from }, callback: Some(query_id) };
        }
        return Incoming::Callback { query_id, chat, message_id: q["message"]["message_id"].as_i64().unwrap_or(0), data: q["data"].as_str().unwrap_or("").to_string() };
    }
    let Some(m) = update.get("message") else { return Incoming::Ignore };
    let chat = m["chat"]["id"].as_i64().unwrap_or(0);
    if chat == 0 {
        return Incoming::Ignore;
    }
    let text = m["text"].as_str();
    if paired == 0 || chat != paired {
        let private = m["chat"]["type"] == "private";
        if let Some(code) = text.and_then(start_code).filter(|_| private && paired == 0) {
            return Incoming::PairAttempt { chat, code: code.to_string() };
        }
        return Incoming::Stranger { chat, callback: None };
    }
    match text {
        Some(t) => Incoming::Text { chat, message_id: m["message_id"].as_i64().unwrap_or(0), text: t.to_string(), date: m["date"].as_u64().unwrap_or(0) },
        None => Incoming::Unsupported { chat },
    }
}

/// The inline keyboard for an approval: approve or decline, tagged with the approval's number.
fn ask_keyboard(id: u32, approve_label: &str) -> Value {
    let label = if approve_label.trim().is_empty() { "Approve" } else { approve_label.trim() };
    json!({ "inline_keyboard": [[
        { "text": format!("✓ {label}"), "callback_data": format!("ok:{id}") },
        { "text": "✕ Decline", "callback_data": format!("no:{id}") }
    ]] })
}

// ---------- talking to Telegram ----------

struct TgError {
    message: String,
    code: i64,
    retry_after: Option<u64>,
}

fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder().timeout(timeout).build().unwrap_or_default()
}

fn method_url(token: &str, method: &str) -> String {
    format!("{API}/bot{token}/{method}")
}

/// Telegram's answer, or its error. Network errors never include the address, which holds the token.
async fn read_answer(res: Result<reqwest::Response, reqwest::Error>) -> Result<Value, TgError> {
    let res = res.map_err(|e| TgError { message: format!("Couldn't reach Telegram: {}", e.without_url()), code: 0, retry_after: None })?;
    let status = res.status().as_u16() as i64;
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if v["ok"] == true {
        return Ok(v["result"].clone());
    }
    let code = v["error_code"].as_i64().unwrap_or(status);
    let desc = v["description"].as_str().unwrap_or("").to_string();
    let message = match code {
        401 | 404 => "Telegram didn't accept the bot token. Copy it again from @BotFather.".to_string(),
        _ if desc.is_empty() => format!("Telegram returned {code}."),
        _ => format!("Telegram: {desc}"),
    };
    Err(TgError { message, code, retry_after: v["parameters"]["retry_after"].as_u64() })
}

async fn call(c: &reqwest::Client, token: &str, method: &str, body: Value) -> Result<Value, TgError> {
    read_answer(c.post(method_url(token, method)).json(&body).send().await).await
}

/// One call with a short timeout, for the commands the UI makes.
async fn quick(token: &str, method: &str, body: Value) -> Result<Value, String> {
    call(&client(Duration::from_secs(20)), token, method, body).await.map_err(|e| e.message)
}

/// The token and the paired chat, or why messages can't be sent yet.
fn target(app: &AppHandle) -> Result<(String, i64), String> {
    let s = settings::load(app);
    let token = s.telegram_token.trim().to_string();
    if token.is_empty() {
        return Err("Telegram isn't set up. Add a bot token in Settings.".into());
    }
    if s.telegram_chat_id == 0 {
        return Err("Telegram isn't paired yet. Pair it in Settings.".into());
    }
    Ok((token, s.telegram_chat_id))
}

async fn send_text(c: &reqwest::Client, token: &str, chat: i64, text: &str) -> Result<(), String> {
    for piece in split_message(text, MAX_MESSAGE) {
        call(c, token, "sendMessage", json!({ "chat_id": chat, "text": escape_html(&piece), "parse_mode": "HTML", "link_preview_options": { "is_disabled": true } }))
            .await
            .map_err(|e| e.message)?;
    }
    Ok(())
}

// ---------- the loop ----------

fn pairing_active() -> bool {
    PAIRING.lock().unwrap().as_ref().is_some_and(|p| Instant::now() < p.until)
}

/// Listen when there's a token and either a paired, enabled chat or a pairing code waiting.
fn should_run(s: &Settings) -> bool {
    !s.telegram_token.trim().is_empty() && ((s.telegram_enabled && s.telegram_chat_id != 0) || pairing_active())
}

fn set_error(e: &str) {
    *LAST_ERROR.lock().unwrap() = e.to_string();
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Start listening if it should, replacing any loop already running. Called at startup and
/// after Telegram's settings change.
pub fn restart(app: &AppHandle) {
    let mut slot = LOOP.lock().unwrap();
    if let Some(old) = slot.take() {
        old.abort();
    }
    if should_run(&settings::load(app)) {
        *slot = Some(tauri::async_runtime::spawn(poll(app.clone())));
    }
}

async fn poll(app: AppHandle) {
    // Long polls wait up to 30 seconds for news, so the client allows a little more.
    let c = client(Duration::from_secs(40));
    let mut offset: i64 = 0;
    let mut backoff = 1u64;
    loop {
        let s = settings::load(&app);
        if !should_run(&s) {
            break;
        }
        let token = s.telegram_token.trim().to_string();
        let res = call(&c, &token, "getUpdates", json!({ "offset": offset, "timeout": 30, "allowed_updates": ["message", "callback_query"] })).await;
        match res {
            Ok(updates) => {
                backoff = 1;
                set_error("");
                for u in updates.as_array().cloned().unwrap_or_default() {
                    if let Some(id) = u["update_id"].as_i64() {
                        offset = offset.max(id + 1);
                    }
                    handle(&app, &c, &token, &u).await;
                }
            }
            Err(e) => {
                set_error(&e.message);
                if matches!(e.code, 401 | 404) {
                    // A bad token won't fix itself; wait for the settings to change.
                    break;
                }
                let wait = match (e.code, e.retry_after) {
                    (_, Some(secs)) => secs,
                    // Another copy of Jarvis (or an old loop) is polling the same bot.
                    (409, _) => 5,
                    _ => backoff,
                };
                backoff = (backoff * 2).min(60);
                tokio::time::sleep(Duration::from_secs(wait.max(1))).await;
            }
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MessageEvent {
    id: i64,
    text: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CallbackEvent {
    id: String,
    data: String,
    message_id: i64,
}

async fn handle(app: &AppHandle, c: &reqwest::Client, token: &str, update: &Value) {
    let paired = settings::load(app).telegram_chat_id;
    match classify(update, paired) {
        Incoming::PairAttempt { chat, code } => {
            let check = check_pairing(&mut PAIRING.lock().unwrap(), &code, Instant::now());
            match check {
                PairCheck::Paired => {
                    let mut s = settings::load(app);
                    s.telegram_chat_id = chat;
                    s.telegram_enabled = true;
                    if let Err(e) = settings::store(app, &s) {
                        set_error(&format!("Couldn't save the pairing: {e}"));
                        return;
                    }
                    *TOLD.lock().unwrap() = None;
                    let _ = send_text(c, token, chat, "Paired with Jarvis. Send a message here any time and Jarvis will answer.").await;
                    let _ = app.emit("telegram-paired", json!({ "chatId": chat }));
                }
                PairCheck::Wrong => {
                    let _ = send_text(c, token, chat, "That code didn't match. Press Pair in Jarvis's Settings for a new one.").await;
                }
                PairCheck::NoCode => tell_stranger(c, token, chat).await,
            }
        }
        Incoming::Text { chat, message_id, text, date } => {
            if start_code(&text).is_some() {
                let _ = send_text(c, token, chat, "Jarvis is listening. Just send a message.").await;
            } else if date + STALE_SECS < now_secs() {
                let _ = send_text(c, token, chat, &format!("Jarvis was offline when you sent “{}”. Send it again if you still want it.", clip(&text, 200))).await;
            } else {
                let _ = app.emit("telegram-message", MessageEvent { id: message_id, text });
            }
        }
        Incoming::Unsupported { chat } => {
            let _ = send_text(c, token, chat, "Only text messages work for now.").await;
        }
        Incoming::Callback { query_id, message_id, data, .. } => {
            let _ = call(c, token, "answerCallbackQuery", json!({ "callback_query_id": query_id })).await;
            let _ = app.emit("telegram-callback", CallbackEvent { id: query_id, data, message_id });
        }
        Incoming::Stranger { chat, callback } => {
            if let Some(q) = callback {
                let _ = call(c, token, "answerCallbackQuery", json!({ "callback_query_id": q, "text": "This bot is private." })).await;
            } else {
                tell_stranger(c, token, chat).await;
            }
        }
        Incoming::Ignore => {}
    }
}

async fn tell_stranger(c: &reqwest::Client, token: &str, chat: i64) {
    let first = TOLD.lock().unwrap().get_or_insert_with(HashSet::new).insert(chat);
    if first {
        let _ = send_text(c, token, chat, "This bot is private.").await;
    }
}

// ---------- commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairStart {
    code: String,
    bot_name: String,
    /// The bot's @username, for a t.me link that sends the code by itself.
    bot_username: String,
}

/// Check the saved token and make a pairing code, valid for ten minutes. Listening starts so the
/// "/start <code>" message can be seen.
#[tauri::command]
pub async fn telegram_pair_start(app: AppHandle) -> Result<PairStart, String> {
    let token = settings::load(&app).telegram_token.trim().to_string();
    if token.is_empty() {
        return Err("Paste the bot token from @BotFather first.".into());
    }
    let me = quick(&token, "getMe", json!({})).await?;
    let name = me["first_name"].as_str().unwrap_or("").to_string();
    let username = me["username"].as_str().unwrap_or("").to_string();
    *BOT.lock().unwrap() = (name.clone(), username.clone());
    let code = new_code();
    *PAIRING.lock().unwrap() = Some(Pairing { code: code.clone(), until: Instant::now() + PAIR_TTL, tries: 0 });
    set_error("");
    restart(&app);
    Ok(PairStart { code, bot_name: name, bot_username: username })
}

/// Stop waiting for a pairing code.
#[tauri::command]
pub fn telegram_pair_cancel(app: AppHandle) {
    *PAIRING.lock().unwrap() = None;
    restart(&app);
}

/// Pick up changed Telegram settings: start, stop or restart listening.
#[tauri::command]
pub fn telegram_restart(app: AppHandle) {
    set_error("");
    restart(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStatus {
    enabled: bool,
    paired: bool,
    /// A token is saved.
    configured: bool,
    /// Jarvis is listening for messages right now.
    running: bool,
    /// A pairing code is waiting to be sent.
    pairing: bool,
    bot_name: String,
    bot_username: String,
    /// The last problem talking to Telegram, or empty.
    error: String,
}

#[tauri::command]
pub async fn telegram_status(app: AppHandle) -> TelegramStatus {
    let s = settings::load(&app);
    let token = s.telegram_token.trim().to_string();
    if !token.is_empty() && BOT.lock().unwrap().1.is_empty() {
        if let Ok(me) = call(&client(Duration::from_secs(8)), &token, "getMe", json!({})).await {
            *BOT.lock().unwrap() = (me["first_name"].as_str().unwrap_or("").into(), me["username"].as_str().unwrap_or("").into());
        }
    }
    let (bot_name, bot_username) = BOT.lock().unwrap().clone();
    let running = LOOP.lock().unwrap().as_ref().is_some_and(|h| !h.inner().is_finished());
    TelegramStatus {
        enabled: s.telegram_enabled,
        paired: s.telegram_chat_id != 0,
        configured: !token.is_empty(),
        running,
        pairing: pairing_active(),
        bot_name,
        bot_username,
        error: LAST_ERROR.lock().unwrap().clone(),
    }
}

/// Send text to the owner's chat, split into as many messages as it needs.
#[tauri::command]
pub async fn telegram_send(app: AppHandle, text: String) -> Result<(), String> {
    let (token, chat) = target(&app)?;
    send_text(&client(Duration::from_secs(20)), &token, chat, &text).await
}

/// Show "typing…" in the owner's chat for a few seconds.
#[tauri::command]
pub async fn telegram_typing(app: AppHandle) -> Result<(), String> {
    let (token, chat) = target(&app)?;
    quick(&token, "sendChatAction", json!({ "chat_id": chat, "action": "typing" })).await.map(|_| ())
}

/// Send one of Jarvis's own files (inside the research folder) to the owner's chat.
#[tauri::command]
pub async fn telegram_send_file(app: AppHandle, path: String, caption: Option<String>) -> Result<(), String> {
    let (token, chat) = target(&app)?;
    let file = crate::slides::inside_research_root(&app, Path::new(&path))?;
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if size > MAX_FILE {
        return Err(format!("That file is {} MB, more than Telegram's 50 MB limit.", size / 1_048_576));
    }
    let bytes = tokio::fs::read(&file).await.map_err(|e| format!("Couldn't read the file: {e}"))?;
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let mut form = reqwest::multipart::Form::new().text("chat_id", chat.to_string()).part("document", reqwest::multipart::Part::bytes(bytes).file_name(name));
    if let Some(c) = caption.filter(|c| !c.trim().is_empty()) {
        form = form.text("caption", escape_html(&clip(&c, MAX_CAPTION))).text("parse_mode", "HTML");
    }
    let res = client(Duration::from_secs(300)).post(method_url(&token, "sendDocument")).multipart(form).send().await;
    read_answer(res).await.map(|_| ()).map_err(|e| e.message)
}

/// Ask the owner to approve something, with Approve and Decline buttons. The press comes back as
/// `telegram-callback` with data "ok:<id>" or "no:<id>". Returns the message's id.
#[tauri::command]
pub async fn telegram_ask(app: AppHandle, text: String, approve_label: Option<String>, id: u32) -> Result<i64, String> {
    let (token, chat) = target(&app)?;
    let body = json!({
        "chat_id": chat,
        "text": escape_html(&clip(&text, MAX_MESSAGE)),
        "parse_mode": "HTML",
        "link_preview_options": { "is_disabled": true },
        "reply_markup": ask_keyboard(id, approve_label.as_deref().unwrap_or("")),
    });
    let sent = quick(&token, "sendMessage", body).await?;
    Ok(sent["message_id"].as_i64().unwrap_or(0))
}

/// Take the buttons off an approval message once it's answered, here or on the Mac. With `text`,
/// the message is rewritten (to say how it was answered).
#[tauri::command]
pub async fn telegram_ask_close(app: AppHandle, message_id: i64, text: Option<String>) -> Result<(), String> {
    let (token, chat) = target(&app)?;
    match text {
        Some(t) => quick(&token, "editMessageText", json!({ "chat_id": chat, "message_id": message_id, "text": escape_html(&clip(&t, MAX_MESSAGE)), "parse_mode": "HTML", "link_preview_options": { "is_disabled": true } })).await,
        None => quick(&token, "editMessageReplyMarkup", json!({ "chat_id": chat, "message_id": message_id, "reply_markup": { "inline_keyboard": [] } })).await,
    }
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_len(s: &str) -> usize {
        s.chars().map(char::len_utf16).sum()
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(escape_html("a < b && c > d"), "a &lt; b &amp;&amp; c &gt; d");
        assert_eq!(escape_html("<b>hi</b>"), "&lt;b&gt;hi&lt;/b&gt;");
        assert_eq!(escape_html("plain नमस्ते"), "plain नमस्ते");
    }

    #[test]
    fn long_text_is_split_to_fit_once_escaped() {
        assert_eq!(split_message("short", MAX_MESSAGE), vec!["short"]);
        assert!(split_message("   ", MAX_MESSAGE).is_empty());

        let para = "word ".repeat(2000); // 10 000 characters
        let parts = split_message(&para, MAX_MESSAGE);
        assert!(parts.len() == 3, "{}", parts.len());
        assert!(parts.iter().all(|p| utf16_len(&escape_html(p)) <= MAX_MESSAGE));
        assert!(parts.iter().all(|p| p.ends_with("word")), "breaks at spaces, not inside words");
        assert_eq!(parts.join(" "), para.trim());

        // Escaping makes "&" five long; the limit counts what Telegram sees.
        let amps = "&".repeat(1000);
        let parts = split_message(&amps, MAX_MESSAGE);
        assert!(parts.iter().all(|p| utf16_len(&escape_html(p)) <= MAX_MESSAGE));
        assert_eq!(parts.concat(), amps);

        // No spaces at all: still split, and never inside a character.
        let emoji = "😀".repeat(3000); // 6000 UTF-16 units
        let parts = split_message(&emoji, MAX_MESSAGE);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| utf16_len(p) <= MAX_MESSAGE));

        // Line breaks are preferred.
        let lines = format!("{}\n{}", "a".repeat(3000), "b".repeat(3000));
        assert_eq!(split_message(&lines, MAX_MESSAGE), vec!["a".repeat(3000), "b".repeat(3000)]);
    }

    #[test]
    fn captions_are_clipped() {
        assert_eq!(clip("short", 1024), "short");
        let long = clip(&"x".repeat(2000), 1024);
        assert!(long.ends_with('…') && utf16_len(&long) <= 1024);
    }

    #[test]
    fn pairing_codes_are_checked_and_used_up() {
        let now = Instant::now();
        let fresh = |code: &str| Some(Pairing { code: code.into(), until: now + PAIR_TTL, tries: 0 });

        let mut slot = fresh("123456");
        assert_eq!(check_pairing(&mut slot, "123456", now), PairCheck::Paired);
        assert!(slot.is_none(), "a code works once");
        assert_eq!(check_pairing(&mut slot, "123456", now), PairCheck::NoCode);

        let mut slot = fresh("123456");
        assert_eq!(check_pairing(&mut slot, "12345", now), PairCheck::Wrong);
        assert_eq!(check_pairing(&mut slot, "", now), PairCheck::Wrong);
        assert_eq!(check_pairing(&mut slot, " 123456 ", now), PairCheck::Paired);

        let mut slot = fresh("123456");
        assert_eq!(check_pairing(&mut slot, "123456", now + PAIR_TTL), PairCheck::NoCode, "expired after ten minutes");

        let mut slot = fresh("123456");
        for _ in 0..PAIR_TRIES {
            assert_eq!(check_pairing(&mut slot, "000000", now), PairCheck::Wrong);
        }
        assert_eq!(check_pairing(&mut slot, "123456", now), PairCheck::NoCode, "guessing uses the code up");

        for _ in 0..50 {
            let c = new_code();
            assert!(c.len() == 6 && c.chars().all(|x| x.is_ascii_digit()), "{c}");
        }
        assert_eq!(start_code("/start 123456"), Some("123456"));
        assert_eq!(start_code("/start@JarvisBot 123456"), Some("123456"));
        assert_eq!(start_code("/start"), Some(""));
        assert_eq!(start_code("start 123456"), None);
        assert_eq!(start_code("/stop 1"), None);
    }

    fn msg(chat: i64, kind: &str, text: Option<&str>) -> Value {
        let mut m = json!({ "message_id": 7, "date": 1_700_000_000u64, "chat": { "id": chat, "type": kind } });
        if let Some(t) = text {
            m["text"] = json!(t);
        }
        json!({ "update_id": 1, "message": m })
    }

    #[test]
    fn only_the_paired_chat_is_listened_to() {
        let owner = 42;
        assert_eq!(classify(&msg(owner, "private", Some("hi")), owner), Incoming::Text { chat: owner, message_id: 7, text: "hi".into(), date: 1_700_000_000 });
        assert_eq!(classify(&msg(99, "private", Some("hi")), owner), Incoming::Stranger { chat: 99, callback: None });
        assert_eq!(classify(&msg(owner, "private", None), owner), Incoming::Unsupported { chat: owner });
        assert_eq!(classify(&msg(99, "private", None), owner), Incoming::Stranger { chat: 99, callback: None });
        // Once paired, nobody else can pair, even with a start command.
        assert_eq!(classify(&msg(99, "private", Some("/start 123456")), owner), Incoming::Stranger { chat: 99, callback: None });
        // Before pairing, a start command in a private chat is a pairing attempt; in a group it isn't.
        assert_eq!(classify(&msg(99, "private", Some("/start 123456")), 0), Incoming::PairAttempt { chat: 99, code: "123456".into() });
        assert_eq!(classify(&msg(-100, "group", Some("/start 123456")), 0), Incoming::Stranger { chat: -100, callback: None });
        assert_eq!(classify(&msg(99, "private", Some("hello")), 0), Incoming::Stranger { chat: 99, callback: None });
        assert_eq!(classify(&json!({ "update_id": 3, "edited_message": {} }), owner), Incoming::Ignore);

        let press = |chat: i64, from: i64| json!({ "update_id": 2, "callback_query": { "id": "q1", "from": { "id": from }, "data": "ok:3", "message": { "message_id": 9, "chat": { "id": chat } } } });
        assert_eq!(classify(&press(owner, owner), owner), Incoming::Callback { query_id: "q1".into(), chat: owner, message_id: 9, data: "ok:3".into() });
        assert_eq!(classify(&press(99, 99), owner), Incoming::Stranger { chat: 99, callback: Some("q1".into()) });
        assert_eq!(classify(&press(owner, 99), owner), Incoming::Stranger { chat: owner, callback: Some("q1".into()) }, "someone else pressing in a shared chat");
        assert_eq!(classify(&press(owner, owner), 0), Incoming::Stranger { chat: owner, callback: Some("q1".into()) }, "not paired: nothing is accepted");
    }

    #[test]
    fn approval_buttons_carry_the_approval_number() {
        let k = ask_keyboard(12, "Send it");
        assert_eq!(k["inline_keyboard"][0][0]["callback_data"], "ok:12");
        assert_eq!(k["inline_keyboard"][0][1]["callback_data"], "no:12");
        assert_eq!(k["inline_keyboard"][0][0]["text"], "✓ Send it");
        assert_eq!(ask_keyboard(1, " ")["inline_keyboard"][0][0]["text"], "✓ Approve");
    }

    #[test]
    fn listening_needs_a_token_and_a_paired_enabled_chat() {
        let mut s = Settings::default();
        assert!(!should_run(&s));
        s.telegram_token = "1:abc".into();
        s.telegram_chat_id = 42;
        assert!(!should_run(&s), "paired but switched off");
        s.telegram_enabled = true;
        assert!(should_run(&s));
        s.telegram_token = " ".into();
        assert!(!should_run(&s));
    }
}
