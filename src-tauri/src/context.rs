//! What the user is looking at: the app in front, its window title, and the text they selected.
//!
//! Read through the macOS Accessibility API, so Jarvis needs the Accessibility permission
//! (System Settings → Privacy & Security → Accessibility). Nothing is captured in the background:
//! a snapshot is taken when the user presses the talk shortcut or says "hey Jarvis" (before
//! Jarvis's own window can take focus), and again when Jarvis asks for it with `get_context`.
//!
//! Text read from other apps is written by whoever wrote it. Callers treat it as data.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

/// Longest selection passed on, so a select-all in a huge file doesn't flood the prompt.
const MAX_SELECTION: usize = 20_000;
/// How long a snapshot from the shortcut stays useful.
const SNAPSHOT_MS: u64 = 10 * 60 * 1000;

#[derive(Clone, Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    /// The app's name, e.g. "Terminal".
    pub app: String,
    pub pid: i32,
    /// Title of its front window, often the file or folder ("SettingsPanel.tsx — Jarvis").
    pub window: String,
    /// The selected text; empty when nothing is selected or the app doesn't expose it.
    pub selection: String,
    /// True when the selection was cut short at MAX_SELECTION characters.
    pub truncated: bool,
    /// When this was read, in ms since the epoch.
    pub at: u64,
}

#[derive(Default)]
pub struct ContextState {
    last: Mutex<Option<Context>>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn clip(mut c: Context) -> Context {
    if c.selection.chars().count() > MAX_SELECTION {
        c.selection = c.selection.chars().take(MAX_SELECTION).collect();
        c.truncated = true;
    }
    c
}

#[cfg(target_os = "macos")]
mod mac {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use std::ffi::c_void;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    type AXUIElementRef = *const c_void;
    const AX_OK: i32 = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: *mut CFTypeRef) -> i32;
        fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
        fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut i32) -> i32;
        fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, seconds: f32) -> i32;
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    }

    /// An owned Accessibility element, released on drop.
    struct El(CFType);

    impl El {
        fn wrap(r: CFTypeRef) -> Option<El> {
            if r.is_null() {
                None
            } else {
                Some(El(unsafe { CFType::wrap_under_create_rule(r) }))
            }
        }
        fn raw(&self) -> AXUIElementRef {
            self.0.as_CFTypeRef()
        }
        fn attr(&self, name: &str) -> Option<CFType> {
            let key = CFString::new(name);
            let mut out: CFTypeRef = std::ptr::null();
            let err = unsafe { AXUIElementCopyAttributeValue(self.raw(), key.as_concrete_TypeRef(), &mut out) };
            if err != AX_OK || out.is_null() {
                return None;
            }
            Some(unsafe { CFType::wrap_under_create_rule(out) })
        }
        fn child(&self, name: &str) -> Option<El> {
            self.attr(name).map(El)
        }
        fn text(&self, name: &str) -> String {
            self.attr(name).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string()).unwrap_or_default()
        }
        fn pid(&self) -> i32 {
            let mut pid = 0;
            unsafe { AXUIElementGetPid(self.raw(), &mut pid) };
            pid
        }
        fn timeout(&self, seconds: f32) {
            unsafe { AXUIElementSetMessagingTimeout(self.raw(), seconds) };
        }
    }

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn screen_allowed() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    pub fn screen_prompt() -> bool {
        unsafe { CGRequestScreenCaptureAccess() }
    }

    /// The id of the front-most normal window of an app, found in the on-screen window list
    /// (which macOS returns front to back).
    pub fn front_window_id(pid: i32) -> Option<u32> {
        use core_foundation::array::CFArray;
        use core_foundation::number::CFNumber;
        use core_graphics::window::{kCGNullWindowID, kCGWindowLayer, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowNumber, kCGWindowOwnerPID, CGWindowListCopyWindowInfo};
        let raw = unsafe { CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID) };
        if raw.is_null() {
            return None;
        }
        let list: CFArray<CFDictionary<CFString, CFType>> = unsafe { CFArray::wrap_under_create_rule(raw) };
        let num = |d: &CFDictionary<CFString, CFType>, key: CFStringRef| -> Option<i64> {
            let k = unsafe { CFString::wrap_under_get_rule(key) };
            let v = d.find(k)?;
            (*v).clone().downcast::<CFNumber>()?.to_i64()
        };
        for d in list.iter() {
            if num(&d, unsafe { kCGWindowOwnerPID }) == Some(pid as i64) && num(&d, unsafe { kCGWindowLayer }) == Some(0) {
                return num(&d, unsafe { kCGWindowNumber }).map(|n| n as u32);
            }
        }
        None
    }

    /// Photograph one window to a PNG file (no shadow, no sound).
    pub fn capture_window(id: u32, path: &std::path::Path) -> Result<(), String> {
        let out = Command::new("/usr/sbin/screencapture")
            .args(["-x", "-o", "-t", "png", &format!("-l{id}")])
            .arg(path)
            .output()
            .map_err(|e| format!("Couldn't take the screenshot: {e}"))?;
        if out.status.success() && path.is_file() {
            Ok(())
        } else {
            Err("Couldn't take the screenshot.".into())
        }
    }

    /// Ask macOS to list Jarvis under Accessibility (shows the system prompt once).
    pub fn prompt() -> bool {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let opts = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
        unsafe { AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef() as *const c_void) }
    }

    /// The app in front: (pid, name, window title, selected text).
    pub fn front() -> Option<(i32, String, String, String)> {
        let system = El::wrap(unsafe { AXUIElementCreateSystemWide() })?;
        system.timeout(0.5);
        let app = system.child("AXFocusedApplication")?;
        app.timeout(0.5);
        let pid = app.pid();
        let name = app.text("AXTitle");
        let window = app.child("AXFocusedWindow").map(|w| w.text("AXTitle")).unwrap_or_default();
        let selection = app.child("AXFocusedUIElement").map(|f| f.text("AXSelectedText")).unwrap_or_default();
        Some((pid, name, window, selection))
    }

    /// Bring an app to the front.
    pub fn activate(pid: i32) -> bool {
        let Some(app) = El::wrap(unsafe { AXUIElementCreateApplication(pid) }) else { return false };
        app.timeout(0.5);
        let key = CFString::new("AXFrontmost");
        let err = unsafe { AXUIElementSetAttributeValue(app.raw(), key.as_concrete_TypeRef(), CFBoolean::true_value().as_CFTypeRef()) };
        err == AX_OK
    }

    /// Press ⌘ + a key (8 = C, 9 = V on every layout macOS maps to ANSI codes).
    fn command_key(code: u16) -> Result<(), String> {
        let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| "Couldn't create a keyboard event.".to_string())?;
        for down in [true, false] {
            let ev = CGEvent::new_keyboard_event(src.clone(), code, down).map_err(|_| "Couldn't create a keyboard event.".to_string())?;
            ev.set_flags(CGEventFlags::CGEventFlagCommand);
            ev.post(CGEventTapLocation::HID);
            std::thread::sleep(Duration::from_millis(15));
        }
        Ok(())
    }

    pub fn clipboard() -> String {
        Command::new("/usr/bin/pbpaste").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    }

    pub fn set_clipboard(text: &str) {
        use std::io::Write;
        if let Ok(mut child) = Command::new("/usr/bin/pbcopy").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }

    /// For apps that don't expose the selection (VS Code, many Electron apps): copy it with ⌘C,
    /// read the clipboard, then put back what was there. Only text on the clipboard survives.
    pub fn copy_selection() -> String {
        let before = clipboard();
        let marker = format!("\u{2063}jarvis-{}\u{2063}", super::now_ms());
        set_clipboard(&marker);
        if command_key(8).is_err() {
            set_clipboard(&before);
            return String::new();
        }
        let mut got = String::new();
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(40));
            let now = clipboard();
            if now != marker {
                got = now;
                break;
            }
        }
        set_clipboard(&before);
        got
    }

    /// Type text into the app in front by pasting it, then restore the clipboard.
    pub fn paste(text: &str) -> Result<(), String> {
        let before = clipboard();
        set_clipboard(text);
        std::thread::sleep(Duration::from_millis(30));
        let r = command_key(9);
        // The target app reads the clipboard asynchronously; give it time before restoring.
        std::thread::sleep(Duration::from_millis(400));
        set_clipboard(&before);
        r
    }
}

const NEEDS_PERMISSION: &str =
    "Jarvis needs the Accessibility permission to see the app in front and its selected text. Open Settings → Your screen to turn it on.";

/// Read the app in front now. Fails when Jarvis itself is in front.
fn read_front(copy_fallback: bool) -> Result<Context, String> {
    #[cfg(target_os = "macos")]
    {
        if !mac::trusted() {
            return Err(NEEDS_PERMISSION.into());
        }
        let (pid, app, window, mut selection) = mac::front().ok_or("Couldn't tell which app is in front.")?;
        if pid as u32 == std::process::id() {
            return Err("jarvis-in-front".into());
        }
        if selection.trim().is_empty() && copy_fallback {
            selection = mac::copy_selection();
        }
        Ok(clip(Context { app, pid, window, selection, truncated: false, at: now_ms() }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = copy_fallback;
        Err("Reading the screen only works on macOS.".into())
    }
}

/// Remember what's in front, called when the user starts talking. Runs off the main thread
/// because a busy app can take up to half a second to answer.
pub fn snapshot(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Ok(c) = read_front(false) {
            *app.state::<ContextState>().last.lock().unwrap() = Some(c);
        }
    });
}

/// Like `snapshot`, but waits, for when Jarvis is about to take focus itself (the command palette).
pub fn snapshot_blocking(app: &AppHandle) {
    // The user asked for the palette on purpose, so copying the selection with ⌘C is fine here.
    if let Ok(c) = read_front(true) {
        *app.state::<ContextState>().last.lock().unwrap() = Some(c);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStatus {
    /// Accessibility: the app in front and its selected text.
    pub trusted: bool,
    /// Screen Recording: photographing a window.
    pub screen: bool,
}

#[tauri::command]
pub fn context_status() -> ContextStatus {
    #[cfg(target_os = "macos")]
    return ContextStatus { trusted: mac::trusted(), screen: mac::screen_allowed() };
    #[cfg(not(target_os = "macos"))]
    ContextStatus { trusted: false, screen: false }
}

/// Show the macOS prompt for Screen Recording and open its settings page.
#[tauri::command]
pub fn request_screen_recording() -> bool {
    #[cfg(target_os = "macos")]
    {
        let ok = mac::screen_prompt();
        if !ok {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
                .spawn();
        }
        ok
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// A photo of the window the user was in (or `pid`'s window), as a PNG data URL, taken now.
/// Only called when the user asks Jarvis to look; nothing is captured in the background.
#[tauri::command]
pub async fn look_at_screen(pid: Option<i32>) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        use base64::Engine;
        if !mac::screen_allowed() {
            return Err("Jarvis needs the Screen Recording permission to look at a window. Open Settings → Your screen to turn it on, then quit and reopen Jarvis.".into());
        }
        tauri::async_runtime::spawn_blocking(move || {
            let pid = match pid {
                Some(p) if p as u32 != std::process::id() => p,
                _ => match mac::front() {
                    Some((p, ..)) if p as u32 != std::process::id() => p,
                    _ => return Err("Jarvis is the app in front. Ask the user to switch to the window they mean and try again.".to_string()),
                },
            };
            let id = mac::front_window_id(pid).ok_or("That app has no open window to look at.")?;
            let path = std::env::temp_dir().join(format!("jarvis-look-{}.png", now_ms()));
            let shot = mac::capture_window(id, &path).and_then(|_| std::fs::read(&path).map_err(|e| e.to_string()));
            let _ = std::fs::remove_file(&path);
            let bytes = shot?;
            Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        Err("Looking at windows only works on macOS.".into())
    }
}

/// Show the macOS prompt and open the Accessibility settings page.
#[tauri::command]
pub fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        let ok = mac::prompt();
        if !ok {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .spawn();
        }
        ok
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// What the user is looking at. Reads the app in front now; when that's Jarvis itself (the user
/// clicked into Jarvis), or it shows no selection, falls back to the snapshot taken when they
/// started talking.
#[tauri::command]
pub async fn get_context(state: State<'_, ContextState>) -> Result<Context, String> {
    let live = tauri::async_runtime::spawn_blocking(|| read_front(true)).await.map_err(|e| e.to_string())?;
    let last = state.last.lock().unwrap().clone().filter(|c| now_ms().saturating_sub(c.at) < SNAPSHOT_MS);
    match (live, last) {
        (Ok(now), Some(before)) if now.selection.trim().is_empty() && now.pid == before.pid && !before.selection.trim().is_empty() => {
            Ok(Context { selection: before.selection, truncated: before.truncated, ..now })
        }
        (Ok(now), _) => Ok(now),
        (Err(e), _) if e == NEEDS_PERMISSION => Err(e),
        (Err(_), Some(before)) => Ok(before),
        (Err(e), None) if e == "jarvis-in-front" => {
            Err("Jarvis is the app in front, so there's nothing else to look at. Ask the user to switch to the app they mean and select the text, then try again.".into())
        }
        (Err(e), None) => Err(e),
    }
}

/// Put text where the selection is in an app (replacing it), by pasting.
#[tauri::command]
pub async fn replace_selection(pid: i32, text: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if !mac::trusted() {
            return Err(NEEDS_PERMISSION.into());
        }
        tauri::async_runtime::spawn_blocking(move || {
            if pid as u32 != std::process::id() && !mac::activate(pid) {
                return Err("That app isn't open any more.".to_string());
            }
            // Let the app come forward and restore its focus before typing into it.
            std::thread::sleep(std::time::Duration::from_millis(350));
            match mac::front() {
                Some((front, ..)) if front == pid => mac::paste(&text),
                _ => Err("Couldn't bring that app to the front to paste into it.".into()),
            }
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, text);
        Err("Pasting into other apps only works on macOS.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_selections_are_cut() {
        let c = clip(Context { selection: "x".repeat(MAX_SELECTION + 5), ..Default::default() });
        assert_eq!(c.selection.len(), MAX_SELECTION);
        assert!(c.truncated);
        let c = clip(Context { selection: "short".into(), ..Default::default() });
        assert!(!c.truncated);
    }
}
