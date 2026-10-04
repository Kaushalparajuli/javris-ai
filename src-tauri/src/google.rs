//! Google Calendar and Gmail, through the user's own Google account.
//!
//! Sign-in is Google's desktop-app flow: the system browser opens Google's consent page, Google
//! sends the browser back to a one-time address on this computer, and Jarvis swaps the code for
//! tokens (with PKCE, so an intercepted code is useless). Tokens are kept in a file only this
//! user can read. The OAuth client is the user's own, made once in Google Cloud Console.
//!
//! Nothing here sends email or invites people by itself: the UI asks the user to confirm first.

use crate::settings;
use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const AUTH: &str = "https://www.googleapis.com/auth";

/// The four things Jarvis can connect to, each signed in separately so any one can be connected
/// or removed alone (and Google never sees an odd mix of permissions in one request).
const SERVICES: &[&str] = &["mail", "calendar", "drive", "youtube"];

fn label(svc: &str) -> &'static str {
    match svc {
        "mail" => "Gmail",
        "calendar" => "Google Calendar",
        "drive" => "Google Drive, Docs and Sheets",
        _ => "YouTube",
    }
}

/// The permissions a service needs, space separated.
fn scopes_for(svc: &str) -> Option<String> {
    let list: &[&str] = match svc {
        "mail" => &["gmail.readonly", "gmail.compose"],
        "calendar" => &["calendar.events"],
        // drive.file: files Jarvis itself creates in Drive (slide decks it uploads), nothing else.
        "drive" => &["drive.readonly", "drive.file", "documents", "spreadsheets"],
        "youtube" => &["youtube.readonly"],
        _ => return None,
    };
    Some(std::iter::once("openid email".to_string()).chain(list.iter().map(|s| format!("{AUTH}/{s}"))).collect::<Vec<_>>().join(" "))
}

/// Which service an API address belongs to, so each request uses that service's sign-in.
fn service_for_url(url: &str) -> &'static str {
    if url.contains("gmail.googleapis.com") || url.contains("/gmail/") {
        "mail"
    } else if url.contains("/calendar/") {
        "calendar"
    } else if url.contains("youtube") {
        "youtube"
    } else {
        "drive"
    }
}

// ---------- tokens ----------

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Tokens {
    refresh_token: String,
    access_token: String,
    /// Seconds since the epoch.
    expires_at: u64,
    email: String,
    /// Space-separated scopes Google actually granted at sign-in.
    scope: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn token_file(app: &AppHandle, svc: &str) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(format!("google-{svc}.json")))
}

/// Before services were separate, one `google.json` held a single sign-in. Hand it to each
/// service it covered, once.
fn migrate_legacy(app: &AppHandle) {
    let Some(dir) = app.path().app_config_dir().ok() else { return };
    let legacy = dir.join("google.json");
    let Some(t) = std::fs::read_to_string(&legacy).ok().and_then(|x| serde_json::from_str::<Tokens>(&x).ok()) else { return };
    if !t.refresh_token.is_empty() {
        for svc in SERVICES {
            let covered = if t.scope.is_empty() {
                matches!(*svc, "mail" | "calendar")
            } else {
                scopes_for(svc).is_some_and(|need| need.split_whitespace().skip(2).all(|n| t.scope.contains(n)))
            };
            if covered && load_tokens(app, svc).is_none() {
                let _ = save_tokens(app, svc, &t);
            }
        }
    }
    let _ = std::fs::remove_file(legacy);
}

fn load_tokens(app: &AppHandle, svc: &str) -> Option<Tokens> {
    let t: Tokens = serde_json::from_str(&std::fs::read_to_string(token_file(app, svc)?).ok()?).ok()?;
    (!t.refresh_token.is_empty()).then_some(t)
}

fn save_tokens(app: &AppHandle, svc: &str, t: &Tokens) -> Result<(), String> {
    let path = token_file(app, svc).ok_or("No config folder available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(t).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// The Google sign-in details compiled in from .env (see build.rs). Empty when the build had none.
const BUILT_IN_ID: &str = env!("JARVIS_GOOGLE_CLIENT_ID");
const BUILT_IN_SECRET: &str = env!("JARVIS_GOOGLE_CLIENT_SECRET");

/// The OAuth client to sign in with: the one built in from .env first, then one saved by an
/// older version's Settings form.
fn client_credentials(app: &AppHandle) -> Result<(String, String), String> {
    if !BUILT_IN_ID.is_empty() {
        return Ok((BUILT_IN_ID.to_string(), BUILT_IN_SECRET.to_string()));
    }
    let s = settings::load(app);
    let id = s.google_client_id.trim().to_string();
    if id.is_empty() {
        return Err("This build of Jarvis has no Google sign-in details. Put GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET in the .env file next to package.json and rebuild.".into());
    }
    Ok((id, s.google_client_secret.trim().to_string()))
}

pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap_or_default()
}

/// Google's error message from a failed response, or the status.
pub(crate) async fn failure(res: reqwest::Response) -> String {
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    let msg = body["error"]["message"].as_str().or(body["error_description"].as_str()).or(body["error"].as_str()).unwrap_or("");
    if msg.is_empty() { format!("Google returned {status}") } else { format!("Google: {msg}") }
}

/// A valid access token, refreshed when it's about to expire.
pub(crate) async fn access_token(app: &AppHandle, svc: &str) -> Result<String, String> {
    migrate_legacy(app);
    let mut t = load_tokens(app, svc).ok_or_else(|| format!("{} isn't connected. Connect it in Settings.", label(svc)))?;
    if t.expires_at > now() + 60 && !t.access_token.is_empty() {
        return Ok(t.access_token);
    }
    let (id, secret) = client_credentials(app)?;
    let res = http()
        .post(TOKEN_URL)
        .form(&[("client_id", id.as_str()), ("client_secret", secret.as_str()), ("refresh_token", t.refresh_token.as_str()), ("grant_type", "refresh_token")])
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        let why = failure(res).await;
        return Err(format!("{} sign-in has expired or was revoked. Connect it again in Settings. ({why})", label(svc)));
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    t.access_token = v["access_token"].as_str().unwrap_or("").to_string();
    t.expires_at = now() + v["expires_in"].as_u64().unwrap_or(3600);
    save_tokens(app, svc, &t)?;
    Ok(t.access_token)
}

pub(crate) async fn api(app: &AppHandle, method: reqwest::Method, url: &str, body: Option<Value>) -> Result<Value, String> {
    let token = access_token(app, service_for_url(url)).await?;
    let mut req = http().request(method, url).bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await.map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        return Err(failure(res).await);
    }
    Ok(res.json().await.unwrap_or(Value::Null))
}

/// A GET whose answer is plain text (Drive exports), cut to `max_bytes` on a character boundary.
pub(crate) async fn api_text(app: &AppHandle, url: &str, max_bytes: usize) -> Result<(String, bool), String> {
    let token = access_token(app, service_for_url(url)).await?;
    let res = http().get(url).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        return Err(failure(res).await);
    }
    let mut text = res.text().await.map_err(|e| e.to_string())?;
    let cut = text.len() > max_bytes;
    if cut {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok((text, cut))
}

/// A URL with its query parameters percent-encoded.
pub(crate) fn url_with(base: &str, params: &[(&str, &str)]) -> String {
    let mut url = tauri::Url::parse(base).expect("valid base url");
    url.query_pairs_mut().extend_pairs(params.iter().copied());
    url.to_string()
}

/// Turns Google's "not enough permission" answers into a plain next step.
pub(crate) fn with_scope_hint(e: String, service: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("insufficient") || l.contains("scope") || l.contains("has not been used") || l.contains("is disabled") {
        format!("{e} Connect {service} again in Settings, Apps, and make sure the {service} API is enabled in your Google Cloud project.")
    } else {
        e
    }
}

// ---------- sign-in ----------

/// PKCE: a random verifier, and the SHA-256 challenge Google checks it against.
fn pkce() -> (String, String) {
    let mut bytes = [0u8; 32];
    let _ = getrandom::fill(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn random_state() -> String {
    let mut bytes = [0u8; 16];
    let _ = getrandom::fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn auth_url(client_id: &str, redirect: &str, challenge: &str, state: &str, scopes: &str) -> String {
    let mut url = tauri::Url::parse(AUTH_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", scopes)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", state);
    url.to_string()
}

/// The `code` and `state` (or Google's `error`) from the browser's request line.
fn parse_redirect(request_line: &str) -> Result<(String, String), String> {
    let path = request_line.split_whitespace().nth(1).ok_or("The browser sent an empty request.")?;
    let url = tauri::Url::parse(&format!("http://localhost{path}")).map_err(|e| e.to_string())?;
    let get = |k: &str| url.query_pairs().find(|(key, _)| key == k).map(|(_, v)| v.to_string());
    if let Some(err) = get("error") {
        return Err(if err == "access_denied" { "You cancelled the Google sign-in.".into() } else { format!("Google sign-in failed: {err}") });
    }
    Ok((get("code").ok_or("Google didn't send a sign-in code.")?, get("state").unwrap_or_default()))
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Jarvis</title>\
    <body style=\"font:16px -apple-system,Segoe UI,sans-serif;display:grid;place-items:center;height:90vh;color:#1f2329\">\
    <div style=\"text-align:center\"><h2>Jarvis is connected to Google</h2><p>You can close this tab and go back to Jarvis.</p></div>";
const FAIL_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Jarvis</title>\
    <body style=\"font:16px -apple-system,Segoe UI,sans-serif;display:grid;place-items:center;height:90vh;color:#1f2329\">\
    <div style=\"text-align:center\"><h2>Jarvis couldn't connect</h2><p>Go back to Jarvis to see what happened.</p></div>";

/// Wait for the browser to come back from Google to our one-time local address.
async fn wait_for_redirect(listener: tokio::net::TcpListener, expected_state: &str) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let accepted = tokio::time::timeout_at(deadline, listener.accept())
            .await
            .map_err(|_| "Google sign-in wasn't finished within five minutes.".to_string())?
            .map_err(|e| e.to_string())?;
        let (mut stream, _) = accepted;
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf)).await.unwrap_or(Ok(0)).unwrap_or(0);
        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        let first = text.lines().next().unwrap_or("");
        // Browsers also ask for /favicon.ico; ignore anything that isn't the redirect.
        if !first.contains("code=") && !first.contains("error=") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        }
        let result = parse_redirect(first).and_then(|(code, state)| {
            if state == expected_state { Ok(code) } else { Err("The sign-in response didn't match. Try connecting again.".into()) }
        });
        let page = if result.is_ok() { DONE_PAGE } else { FAIL_PAGE };
        let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        let _ = stream.write_all(reply.as_bytes()).await;
        let _ = stream.shutdown().await;
        return result;
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    id: String,
    label: String,
    connected: bool,
    /// The account this service is signed in with.
    email: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleStatus {
    configured: bool,
    /// Gmail or Calendar is connected (what routines and the morning briefing need).
    connected: bool,
    /// The account of the first connected service, for "email me" steps.
    email: String,
    services: Vec<ServiceStatus>,
}

#[tauri::command]
pub fn google_status(app: AppHandle) -> GoogleStatus {
    let configured = client_credentials(&app).is_ok();
    migrate_legacy(&app);
    let services: Vec<ServiceStatus> = SERVICES
        .iter()
        .map(|svc| {
            let t = load_tokens(&app, svc);
            ServiceStatus { id: svc.to_string(), label: label(svc).into(), connected: t.is_some(), email: t.map(|t| t.email).unwrap_or_default() }
        })
        .collect();
    let email = services.iter().filter(|s| s.connected && (s.id == "mail" || s.id == "calendar")).map(|s| s.email.clone()).find(|e| !e.is_empty()).unwrap_or_default();
    let connected = services.iter().any(|s| s.connected && (s.id == "mail" || s.id == "calendar"));
    GoogleStatus { configured, connected, email, services }
}

/// Sign in to one Google service ("mail", "calendar", "drive" or "youtube") in the system
/// browser and keep its tokens. Returns the account's email.
#[tauri::command]
pub async fn google_connect(app: AppHandle, service: String) -> Result<String, String> {
    let scopes = scopes_for(&service).ok_or("Unknown Google service.")?;
    let (client_id, secret) = client_credentials(&app)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let (verifier, challenge) = pkce();
    let state = random_state();
    app.opener().open_url(auth_url(&client_id, &redirect, &challenge, &state, &scopes), None::<&str>).map_err(|e| format!("Couldn't open the browser: {e}"))?;

    let code = wait_for_redirect(listener, &state).await?;
    let res = http()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        return Err(failure(res).await);
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    let mut tokens = Tokens {
        refresh_token: v["refresh_token"].as_str().unwrap_or("").to_string(),
        access_token: v["access_token"].as_str().unwrap_or("").to_string(),
        expires_at: now() + v["expires_in"].as_u64().unwrap_or(3600),
        email: String::new(),
        scope: v["scope"].as_str().unwrap_or("").to_string(),
    };
    if tokens.refresh_token.is_empty() {
        return Err("Google didn't grant lasting access. Remove Jarvis at myaccount.google.com/permissions and connect again.".into());
    }
    // The email is only for showing which account is connected.
    let who = http().get("https://openidconnect.googleapis.com/v1/userinfo").bearer_auth(&tokens.access_token).send().await;
    if let Ok(r) = who {
        let v: Value = r.json().await.unwrap_or(Value::Null);
        tokens.email = v["email"].as_str().unwrap_or("").to_string();
    }
    save_tokens(&app, &service, &tokens)?;
    Ok(tokens.email)
}

/// Remove one service's access: revoke it at Google (unless another service still uses the same
/// grant, as after an upgrade from the single sign-in) and forget its tokens.
#[tauri::command]
pub async fn google_disconnect(app: AppHandle, service: String) -> Result<(), String> {
    if scopes_for(&service).is_none() {
        return Err("Unknown Google service.".into());
    }
    if let Some(t) = load_tokens(&app, &service) {
        let shared = SERVICES.iter().filter(|s| **s != service).any(|s| load_tokens(&app, s).is_some_and(|o| o.refresh_token == t.refresh_token));
        if !shared {
            let _ = http().post(REVOKE_URL).form(&[("token", t.refresh_token.as_str())]).send().await;
        }
    }
    if let Some(path) = token_file(&app, &service) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

// ---------- calendar ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    id: String,
    title: String,
    /// RFC 3339 date-time, or YYYY-MM-DD for all-day events.
    start: String,
    end: String,
    all_day: bool,
    location: String,
    attendees: usize,
    /// Guests' email addresses, for editing.
    attendee_emails: Vec<String>,
    description: String,
    link: String,
    meet: String,
    /// One occurrence of a repeating event: changes and deletes apply to this one only.
    recurring: bool,
}

fn event_from(v: &Value) -> Event {
    let when = |k: &str| v[k]["dateTime"].as_str().or(v[k]["date"].as_str()).unwrap_or("").to_string();
    Event {
        id: v["id"].as_str().unwrap_or("").into(),
        title: v["summary"].as_str().unwrap_or("(no title)").into(),
        start: when("start"),
        end: when("end"),
        all_day: v["start"]["date"].is_string(),
        location: v["location"].as_str().unwrap_or("").into(),
        attendees: v["attendees"].as_array().map(|a| a.len()).unwrap_or(0),
        attendee_emails: v["attendees"]
            .as_array()
            .map(|a| a.iter().filter(|x| x["self"] != true && x["resource"] != true).filter_map(|x| x["email"].as_str().map(String::from)).collect())
            .unwrap_or_default(),
        description: v["description"].as_str().unwrap_or("").into(),
        recurring: v["recurringEventId"].is_string(),
        link: v["htmlLink"].as_str().unwrap_or("").into(),
        meet: v["hangoutLink"].as_str().unwrap_or("").into(),
    }
}

/// Events on the primary calendar between two RFC 3339 times, in order.
#[tauri::command]
pub async fn calendar_list(app: AppHandle, from: String, to: String) -> Result<Vec<Event>, String> {
    let mut url = tauri::Url::parse("https://www.googleapis.com/calendar/v3/calendars/primary/events").unwrap();
    url.query_pairs_mut()
        .append_pair("timeMin", &from)
        .append_pair("timeMax", &to)
        .append_pair("singleEvents", "true")
        .append_pair("orderBy", "startTime")
        .append_pair("maxResults", "50");
    let v = api(&app, reqwest::Method::GET, url.as_str(), None).await?;
    Ok(v["items"].as_array().map(|a| a.iter().map(event_from).collect()).unwrap_or_default())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEvent {
    title: String,
    /// Local date-time, "YYYY-MM-DDTHH:MM", in `time_zone`.
    start: String,
    end: String,
    time_zone: String,
    #[serde(default)]
    attendees: Vec<String>,
    #[serde(default)]
    location: String,
    #[serde(default)]
    description: String,
    /// A whole-day event: `start` and `end` are dates ("YYYY-MM-DD"), and `end` is the day after.
    #[serde(default)]
    all_day: bool,
}

/// Google's start or end object for a time, whole-day or not.
fn when_json(t: &str, all_day: bool, zone: &str) -> Value {
    if all_day {
        json!({ "date": &t[..t.len().min(10)], "dateTime": null })
    } else {
        let t = if t.len() == 16 { format!("{t}:00") } else { t.to_string() };
        json!({ "dateTime": t, "timeZone": zone, "date": null })
    }
}

/// Create an event. When it has attendees Google emails them invitations, so the UI confirms
/// with the user before calling this.
#[tauri::command]
pub async fn calendar_create(app: AppHandle, event: NewEvent) -> Result<Event, String> {
    let mut body = json!({
        "summary": event.title,
        "start": when_json(&event.start, event.all_day, &event.time_zone),
        "end": when_json(&event.end, event.all_day, &event.time_zone),
    });
    // A new event has nothing to clear, so drop the nulls that only matter when editing.
    for k in ["start", "end"] {
        body[k].as_object_mut().map(|o| o.retain(|_, v| !v.is_null()));
    }
    if !event.location.is_empty() {
        body["location"] = json!(event.location);
    }
    if !event.description.is_empty() {
        body["description"] = json!(event.description);
    }
    let invite = !event.attendees.is_empty();
    if invite {
        body["attendees"] = json!(event.attendees.iter().map(|e| json!({ "email": e })).collect::<Vec<_>>());
    }
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?sendUpdates={}",
        if invite { "all" } else { "none" }
    );
    Ok(event_from(&api(&app, reqwest::Method::POST, &url, Some(body)).await?))
}

/// Event ids go into the request path, so only the characters Google uses are allowed.
fn event_url(id: &str) -> Result<String, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || "_-.@".contains(c)) {
        return Err("That isn't a valid event id.".into());
    }
    Ok(format!("https://www.googleapis.com/calendar/v3/calendars/primary/events/{id}"))
}

/// One event, in full.
#[tauri::command]
pub async fn calendar_get(app: AppHandle, id: String) -> Result<Event, String> {
    let url = event_url(&id)?;
    Ok(event_from(&api(&app, reqwest::Method::GET, &url, None).await?))
}

/// Changes to an event. Anything left out stays as it is.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPatch {
    title: Option<String>,
    /// New start and end go together, as local date-times (or dates when `all_day`).
    start: Option<String>,
    end: Option<String>,
    all_day: Option<bool>,
    time_zone: Option<String>,
    location: Option<String>,
    description: Option<String>,
    /// The full guest list after the change. Guests who stay keep their replies.
    attendees: Option<Vec<String>>,
    /// Email the guests about the change.
    #[serde(default)]
    notify: bool,
}

/// Change an event. Guests are emailed only when `notify` is set, which the UI confirms first.
#[tauri::command]
pub async fn calendar_update(app: AppHandle, id: String, patch: EventPatch) -> Result<Event, String> {
    let url = event_url(&id)?;
    let mut body = json!({});
    if let Some(t) = &patch.title {
        body["summary"] = json!(t);
    }
    if let Some(l) = &patch.location {
        body["location"] = json!(l);
    }
    if let Some(d) = &patch.description {
        body["description"] = json!(d);
    }
    if let (Some(start), Some(end)) = (&patch.start, &patch.end) {
        let all_day = patch.all_day.unwrap_or(false);
        let zone = patch.time_zone.clone().unwrap_or_default();
        body["start"] = when_json(start, all_day, &zone);
        body["end"] = when_json(end, all_day, &zone);
    } else if patch.start.is_some() || patch.end.is_some() {
        return Err("Change the start and the end together.".into());
    }
    if let Some(wanted) = &patch.attendees {
        // Keep the people who are already invited as they are; add or drop the rest.
        let current = api(&app, reqwest::Method::GET, &url, None).await?;
        let existing = current["attendees"].as_array().cloned().unwrap_or_default();
        let mut list: Vec<Value> = existing
            .into_iter()
            .filter(|a| a["self"] == true || a["resource"] == true || a["email"].as_str().is_some_and(|e| wanted.iter().any(|w| w.eq_ignore_ascii_case(e))))
            .collect();
        for email in wanted {
            if !list.iter().any(|a| a["email"].as_str().is_some_and(|e| e.eq_ignore_ascii_case(email))) {
                list.push(json!({ "email": email }));
            }
        }
        body["attendees"] = json!(list);
    }
    let url = format!("{url}?sendUpdates={}", if patch.notify { "all" } else { "none" });
    Ok(event_from(&api(&app, reqwest::Method::PATCH, &url, Some(body)).await?))
}

/// Delete an event. Guests are emailed the cancellation only when `notify` is set.
#[tauri::command]
pub async fn calendar_delete(app: AppHandle, id: String, notify: bool) -> Result<(), String> {
    let url = format!("{}?sendUpdates={}", event_url(&id)?, if notify { "all" } else { "none" });
    api(&app, reqwest::Method::DELETE, &url, None).await?;
    Ok(())
}

// ---------- mail ----------

const GMAIL: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSummary {
    id: String,
    thread_id: String,
    from: String,
    subject: String,
    date: String,
    snippet: String,
    unread: bool,
}

fn header(v: &Value, name: &str) -> String {
    v["payload"]["headers"]
        .as_array()
        .and_then(|h| h.iter().find(|x| x["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))))
        .and_then(|x| x["value"].as_str())
        .unwrap_or("")
        .to_string()
}

/// Gmail search, same syntax as the Gmail search box ("is:unread newer_than:2d", "from:sita").
#[tauri::command]
pub async fn mail_search(app: AppHandle, query: String, max: Option<u32>) -> Result<Vec<MailSummary>, String> {
    let mut url = tauri::Url::parse(&format!("{GMAIL}/messages")).unwrap();
    url.query_pairs_mut().append_pair("q", &query).append_pair("maxResults", &max.unwrap_or(8).clamp(1, 50).to_string());
    let list = api(&app, reqwest::Method::GET, url.as_str(), None).await?;
    let ids: Vec<String> = list["messages"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default();
    let fetches = ids.iter().map(|id| {
        let url = format!("{GMAIL}/messages/{id}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date");
        let app = app.clone();
        async move { api(&app, reqwest::Method::GET, &url, None).await }
    });
    let mut out = vec![];
    for v in futures_util::future::join_all(fetches).await.into_iter().flatten() {
        out.push(MailSummary {
            id: v["id"].as_str().unwrap_or("").into(),
            thread_id: v["threadId"].as_str().unwrap_or("").into(),
            from: header(&v, "From"),
            subject: header(&v, "Subject"),
            date: header(&v, "Date"),
            snippet: v["snippet"].as_str().unwrap_or("").into(),
            unread: v["labelIds"].as_array().is_some_and(|l| l.iter().any(|x| x == "UNREAD")),
        });
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mail {
    id: String,
    thread_id: String,
    from: String,
    to: String,
    subject: String,
    date: String,
    message_id: String,
    body: String,
}

/// Gmail's message parts are URL-safe base64, with or without trailing padding.
fn decode_part(data: &str) -> String {
    let bytes = URL_SAFE_NO_PAD.decode(data.trim_end_matches('=')).unwrap_or_default();
    String::from_utf8_lossy(&bytes).to_string()
}

/// The readable text of a message: its plain-text part, or its HTML with the tags stripped.
fn body_text(payload: &Value) -> String {
    fn find(p: &Value, mime: &str) -> Option<String> {
        if p["mimeType"].as_str() == Some(mime) {
            if let Some(d) = p["body"]["data"].as_str() {
                return Some(decode_part(d));
            }
        }
        p["parts"].as_array()?.iter().find_map(|c| find(c, mime))
    }
    if let Some(t) = find(payload, "text/plain") {
        return t.trim().to_string();
    }
    let html = find(payload, "text/html").unwrap_or_default();
    strip_html(&html)
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let (mut in_tag, mut skip) = (false, false);
    let lower = html.to_lowercase();
    let mut i = 0;
    let chars: Vec<(usize, char)> = html.char_indices().collect();
    while i < chars.len() {
        let (at, c) = chars[i];
        if c == '<' {
            in_tag = true;
            let rest = &lower[at..];
            if rest.starts_with("<style") || rest.starts_with("<script") {
                skip = true;
            } else if rest.starts_with("</style") || rest.starts_with("</script") {
                skip = false;
            } else if rest.starts_with("<br") || rest.starts_with("<p") || rest.starts_with("</p") || rest.starts_with("<div") || rest.starts_with("<li") || rest.starts_with("<tr") {
                out.push('\n');
            }
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag && !skip {
            out.push(c);
        }
        i += 1;
    }
    let text = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.join("\n").split("\n\n\n").collect::<Vec<_>>().join("\n\n").trim().to_string()
}

/// One message in full, as text. Long messages are cut to keep voice answers sensible.
#[tauri::command]
pub async fn mail_read(app: AppHandle, id: String) -> Result<Mail, String> {
    let v = api(&app, reqwest::Method::GET, &format!("{GMAIL}/messages/{id}?format=full"), None).await?;
    let mut body = body_text(&v["payload"]);
    if body.chars().count() > 8000 {
        body = format!("{}…", body.chars().take(8000).collect::<String>());
    }
    Ok(Mail {
        id: v["id"].as_str().unwrap_or("").into(),
        thread_id: v["threadId"].as_str().unwrap_or("").into(),
        from: header(&v, "From"),
        to: header(&v, "To"),
        subject: header(&v, "Subject"),
        date: header(&v, "Date"),
        message_id: header(&v, "Message-ID"),
        body,
    })
}

/// A header value, encoded for non-ASCII text (RFC 2047) so names and subjects in any language
/// arrive intact.
fn encode_header(value: &str) -> String {
    if value.is_ascii() { value.to_string() } else { format!("=?UTF-8?B?{}?=", STANDARD.encode(value)) }
}

/// An RFC 2822 message, ready for Gmail's `raw` field.
fn build_message(to: &str, subject: &str, body: &str, reply: Option<(&str, &str)>) -> String {
    let mut msg = format!("To: {}\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n", to.trim(), encode_header(subject.trim()));
    if let Some((message_id, _)) = reply.filter(|(m, _)| !m.is_empty()) {
        msg.push_str(&format!("In-Reply-To: {message_id}\r\nReferences: {message_id}\r\n"));
    }
    msg.push_str("\r\n");
    let encoded = STANDARD.encode(body.replace('\n', "\r\n"));
    for line in encoded.as_bytes().chunks(76) {
        msg.push_str(std::str::from_utf8(line).unwrap());
        msg.push_str("\r\n");
    }
    URL_SAFE.encode(msg)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    id: String,
    to: String,
    subject: String,
    body: String,
}

/// Save a draft in Gmail. Replying keeps it in the original conversation. Nothing is sent.
#[tauri::command]
pub async fn mail_draft(app: AppHandle, to: String, subject: String, body: String, reply_to: Option<String>) -> Result<Draft, String> {
    let original = match reply_to.filter(|r| !r.is_empty()) {
        Some(id) => Some(mail_read(app.clone(), id).await?),
        None => None,
    };
    let subject = match &original {
        Some(o) if subject.trim().is_empty() => {
            if o.subject.to_lowercase().starts_with("re:") { o.subject.clone() } else { format!("Re: {}", o.subject) }
        }
        _ => subject,
    };
    let raw = build_message(&to, &subject, &body, original.as_ref().map(|o| (o.message_id.as_str(), o.thread_id.as_str())));
    let mut message = json!({ "raw": raw });
    if let Some(o) = &original {
        message["threadId"] = json!(o.thread_id);
    }
    let v = api(&app, reqwest::Method::POST, &format!("{GMAIL}/drafts"), Some(json!({ "message": message }))).await?;
    Ok(Draft { id: v["id"].as_str().unwrap_or("").into(), to, subject, body })
}

/// Send a draft. The UI shows the user who it goes to and what it says, and only calls this
/// after they confirm.
#[tauri::command]
pub async fn mail_send_draft(app: AppHandle, draft_id: String) -> Result<(), String> {
    api(&app, reqwest::Method::POST, &format!("{GMAIL}/drafts/send"), Some(json!({ "id": draft_id }))).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn times_and_whole_days_become_google_json() {
        let t = super::when_json("2026-10-05T15:00", false, "Asia/Kathmandu");
        assert_eq!(t["dateTime"], "2026-10-05T15:00:00");
        assert_eq!(t["timeZone"], "Asia/Kathmandu");
        assert!(t["date"].is_null());
        let d = super::when_json("2026-10-05", true, "Asia/Kathmandu");
        assert_eq!(d["date"], "2026-10-05");
        assert!(d["dateTime"].is_null());
    }

    #[test]
    fn event_ids_cannot_escape_the_path() {
        assert!(super::event_url("abc123_20261001T090000Z").is_ok());
        assert!(super::event_url("../calendars").is_err());
        assert!(super::event_url("a/b").is_err());
        assert!(super::event_url("a?x=1").is_err());
        assert!(super::event_url("").is_err());
    }

    use super::*;

    #[test]
    fn pkce_challenge_matches_the_verifier() {
        let (verifier, challenge) = pkce();
        assert!(verifier.len() >= 43, "PKCE verifiers need at least 43 characters");
        assert_eq!(challenge, URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())));
        assert_ne!(pkce().0, verifier, "each sign-in gets a fresh verifier");
    }

    #[test]
    fn the_consent_page_asks_for_offline_access_with_pkce() {
        let url = auth_url("abc.apps.googleusercontent.com", "http://127.0.0.1:5555", "CHAL", "ST", &scopes_for("mail").unwrap());
        for part in ["client_id=abc.apps", "redirect_uri=http%3A%2F%2F127.0.0.1%3A5555", "code_challenge=CHAL", "code_challenge_method=S256", "access_type=offline", "state=ST", "gmail.compose"] {
            assert!(url.contains(part), "missing {part} in {url}");
        }
    }

    #[test]
    fn each_service_asks_only_for_its_own_permissions() {
        assert!(scopes_for("youtube").unwrap().contains("youtube.readonly"));
        assert!(!scopes_for("youtube").unwrap().contains("drive"));
        assert!(scopes_for("drive").unwrap().contains("spreadsheets"));
        assert!(scopes_for("drive").unwrap().split_whitespace().any(|s| s == "https://www.googleapis.com/auth/drive.file"), "uploading slides needs drive.file");
        assert!(!scopes_for("drive").unwrap().split_whitespace().any(|s| s == "https://www.googleapis.com/auth/drive"), "never the whole of Drive");
        assert!(!scopes_for("calendar").unwrap().contains("gmail"));
        assert!(scopes_for("nope").is_none());
        for svc in SERVICES {
            assert!(scopes_for(svc).unwrap().starts_with("openid email "), "{svc}");
        }
    }

    #[test]
    fn requests_use_the_matching_services_sign_in() {
        assert_eq!(service_for_url("https://gmail.googleapis.com/gmail/v1/users/me/messages"), "mail");
        assert_eq!(service_for_url("https://www.googleapis.com/calendar/v3/calendars/primary/events"), "calendar");
        assert_eq!(service_for_url("https://www.googleapis.com/youtube/v3/search?q=x"), "youtube");
        assert_eq!(service_for_url("https://docs.googleapis.com/v1/documents"), "drive");
        assert_eq!(service_for_url("https://sheets.googleapis.com/v4/spreadsheets"), "drive");
        assert_eq!(service_for_url("https://www.googleapis.com/drive/v3/files"), "drive");
    }

    #[test]
    fn reads_the_redirect() {
        assert_eq!(parse_redirect("GET /?state=ST&code=4%2F0Ab&scope=x HTTP/1.1").unwrap(), ("4/0Ab".to_string(), "ST".to_string()));
        assert!(parse_redirect("GET /?error=access_denied&state=ST HTTP/1.1").unwrap_err().contains("cancelled"));
    }

    #[tokio::test]
    async fn the_local_sign_in_page_catches_the_code() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(async move { wait_for_redirect(listener, "ST").await });
        // A browser asks for the favicon too; that must not end the wait.
        let mut favicon = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        favicon.write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut browser = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        browser.write_all(b"GET /?state=ST&code=THE-CODE HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut page = String::new();
        browser.read_to_string(&mut page).await.unwrap();
        assert!(page.contains("Jarvis is connected"));
        assert_eq!(waiting.await.unwrap().unwrap(), "THE-CODE");

        // A mismatched state is refused.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(async move { wait_for_redirect(listener, "ST").await });
        let mut forged = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        forged.write_all(b"GET /?state=OTHER&code=X HTTP/1.1\r\n\r\n").await.unwrap();
        assert!(waiting.await.unwrap().is_err());
    }

    #[test]
    fn events_read_timed_and_all_day() {
        let timed = event_from(&json!({ "id": "1", "summary": "Standup", "start": { "dateTime": "2026-10-01T09:00:00+05:45" }, "end": { "dateTime": "2026-10-01T09:15:00+05:45" }, "attendees": [{}, {}], "hangoutLink": "https://meet.google.com/x" }));
        assert_eq!((timed.title.as_str(), timed.all_day, timed.attendees), ("Standup", false, 2));
        let all_day = event_from(&json!({ "id": "2", "start": { "date": "2026-10-02" }, "end": { "date": "2026-10-03" } }));
        assert_eq!((all_day.title.as_str(), all_day.all_day, all_day.start.as_str()), ("(no title)", true, "2026-10-02"));
    }

    #[test]
    fn mail_bodies_come_out_as_text() {
        let plain = URL_SAFE.encode("Hi Kaushal,\nThe deck is attached.\nनमस्ते");
        assert!(plain.ends_with('='), "this sample should carry padding");
        assert_eq!(decode_part(&plain), decode_part(plain.trim_end_matches('=')), "padded and unpadded decode the same");
        let html = URL_SAFE.encode("<html><style>p{color:red}</style><p>Hello&nbsp;there</p><p>Second &amp; last</p></html>");
        let multipart = json!({ "mimeType": "multipart/alternative", "parts": [
            { "mimeType": "text/html", "body": { "data": html } },
            { "mimeType": "text/plain", "body": { "data": plain } } ] });
        assert!(body_text(&multipart).contains("The deck is attached.\nनमस्ते"), "plain text preferred");
        let html_only = json!({ "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>Hello&nbsp;there</p><p>Second &amp; last</p><style>x{}</style>") } });
        let text = body_text(&html_only);
        assert!(text.contains("Hello there") && text.contains("Second & last") && !text.contains("x{}"), "{text:?}");
    }

    #[test]
    fn drafts_are_valid_mime_in_any_language() {
        let raw = build_message("sita@example.com", "भेला योजना", "Hi Sita,\nSee you Friday.\nधन्यवाद", Some(("<abc@mail.gmail.com>", "t1")));
        let msg = String::from_utf8(URL_SAFE.decode(raw).unwrap()).unwrap();
        assert!(msg.contains("To: sita@example.com\r\n"));
        assert!(msg.contains(&format!("Subject: =?UTF-8?B?{}?=", STANDARD.encode("भेला योजना"))));
        assert!(msg.contains("In-Reply-To: <abc@mail.gmail.com>"));
        let body = msg.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
        assert_eq!(String::from_utf8(STANDARD.decode(body).unwrap()).unwrap(), "Hi Sita,\r\nSee you Friday.\r\nधन्यवाद");
        let plain = String::from_utf8(URL_SAFE.decode(build_message("a@b.c", "Plain", "x", None)).unwrap()).unwrap();
        assert!(plain.contains("Subject: Plain\r\n") && !plain.contains("In-Reply-To"));
    }
}
