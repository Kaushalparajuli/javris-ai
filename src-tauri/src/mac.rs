//! Mac control: Jarvis reads another app's interface as a list of numbered controls (through the
//! Accessibility API, the same one a screen reader uses), then clicks, types, presses keys and
//! scrolls in it.
//!
//! Every action is gated on the front end by an approval for the app ("Control Safari"), and by
//! `halt`, which the stop shortcut (⌥.) trips: nothing here runs again until the user allows it.
//! Text read from an app was written by someone else; callers treat it as data. Jarvis never types
//! into a password field, and never opens anything but apps and web/mail links.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

static HALTED: AtomicBool = AtomicBool::new(false);

/// Stop controlling apps right now (the ⌥. shortcut).
pub fn halt() {
    HALTED.store(true, Ordering::SeqCst);
}

fn check_running() -> Result<(), String> {
    if HALTED.load(Ordering::SeqCst) {
        Err("Stopped: the user pressed stop. Don't try again until they ask.".into())
    } else {
        Ok(())
    }
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MacView {
    pub app: String,
    pub window: String,
    /// One control per line: `[12] button “Send”`. The number is what mac_click takes.
    pub controls: String,
    /// True when the window had more than fits.
    pub truncated: bool,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::MacView;
    use core_foundation::array::{CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef};
    use core_foundation::base::{CFGetTypeID, CFType, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::number::CFNumber;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::{CGPoint, CGSize};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::Duration;

    type AXUIElementRef = *const c_void;
    const AX_OK: i32 = 0;
    const AX_VALUE_POINT: u32 = 1;
    const AX_VALUE_SIZE: u32 = 2;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventPost(tap: u32, event: *const c_void);
        fn CFRelease(cf: *const c_void);
        fn CGEventCreateScrollWheelEvent2(source: *const c_void, units: u32, wheel_count: u32, w1: i32, w2: i32, w3: i32) -> *const c_void;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: *mut CFTypeRef) -> i32;
        fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
        fn AXUIElementPerformAction(el: AXUIElementRef, action: CFStringRef) -> i32;
        fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut i32) -> i32;
        fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, seconds: f32) -> i32;
        fn AXValueGetValue(value: CFTypeRef, ty: u32, out: *mut c_void) -> bool;
    }

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
            match self.attr(name) {
                Some(v) => {
                    if let Some(s) = v.downcast::<CFString>() {
                        s.to_string()
                    } else if let Some(n) = v.downcast::<CFNumber>() {
                        n.to_i64().map(|n| n.to_string()).unwrap_or_default()
                    } else if let Some(b) = v.downcast::<CFBoolean>() {
                        bool::from(b).to_string()
                    } else {
                        String::new()
                    }
                }
                None => String::new(),
            }
        }
        fn children(&self) -> Vec<El> {
            let Some(v) = self.attr("AXChildren") else { return vec![] };
            let arr = v.as_CFTypeRef() as CFArrayRef;
            if unsafe { CFGetTypeID(v.as_CFTypeRef()) != CFArrayGetTypeID() } {
                return vec![];
            }
            (0..unsafe { CFArrayGetCount(arr) })
                .filter_map(|i| {
                    let p = unsafe { CFArrayGetValueAtIndex(arr, i) };
                    if p.is_null() {
                        None
                    } else {
                        Some(El(unsafe { CFType::wrap_under_get_rule(p) }))
                    }
                })
                .collect()
        }
        fn timeout(&self, seconds: f32) {
            unsafe { AXUIElementSetMessagingTimeout(self.raw(), seconds) };
        }
        fn pid(&self) -> i32 {
            let mut pid = 0;
            unsafe { AXUIElementGetPid(self.raw(), &mut pid) };
            pid
        }
        /// x, y, width, height on screen.
        fn frame(&self) -> Option<(f64, f64, f64, f64)> {
            let pos = self.attr("AXPosition")?;
            let size = self.attr("AXSize")?;
            let mut p = CGPoint { x: 0.0, y: 0.0 };
            let mut s = CGSize { width: 0.0, height: 0.0 };
            let ok = unsafe {
                AXValueGetValue(pos.as_CFTypeRef(), AX_VALUE_POINT, &mut p as *mut _ as *mut c_void)
                    && AXValueGetValue(size.as_CFTypeRef(), AX_VALUE_SIZE, &mut s as *mut _ as *mut c_void)
            };
            ok.then_some((p.x, p.y, s.width, s.height))
        }
        fn perform(&self, action: &str) -> bool {
            let a = CFString::new(action);
            unsafe { AXUIElementPerformAction(self.raw(), a.as_concrete_TypeRef()) == AX_OK }
        }
    }

    /// The controls from the last read, so a click can name one by number.
    struct Held(El);
    // Accessibility references are plain handles; they are only used while this lock is held.
    unsafe impl Send for Held {}

    struct Snapshot {
        pid: i32,
        els: HashMap<u32, Held>,
    }

    static SNAP: Mutex<Option<Snapshot>> = Mutex::new(None);

    const NEEDS_PERMISSION: &str = "Jarvis needs the Accessibility permission to control apps. Open Settings → Your screen to turn it on.";

    fn trusted() -> Result<(), String> {
        if unsafe { AXIsProcessTrusted() } {
            Ok(())
        } else {
            Err(NEEDS_PERMISSION.into())
        }
    }

    /// Apps with a window on screen: (pid, name).
    fn visible_apps() -> Vec<(i32, String)> {
        use core_foundation::array::CFArray;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::number::CFNumber;
        use core_graphics::window::{kCGNullWindowID, kCGWindowLayer, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowOwnerName, kCGWindowOwnerPID, CGWindowListCopyWindowInfo};
        let raw = unsafe { CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID) };
        if raw.is_null() {
            return vec![];
        }
        let list: CFArray<CFDictionary<CFString, CFType>> = unsafe { CFArray::wrap_under_create_rule(raw) };
        let mut out: Vec<(i32, String)> = vec![];
        for d in list.iter() {
            let get = |key: CFStringRef| d.find(unsafe { CFString::wrap_under_get_rule(key) }).map(|v| (*v).clone());
            let layer = get(unsafe { kCGWindowLayer }).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_i64());
            let pid = get(unsafe { kCGWindowOwnerPID }).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_i64());
            let name = get(unsafe { kCGWindowOwnerName }).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string());
            if let (Some(0), Some(pid), Some(name)) = (layer, pid, name) {
                if !out.iter().any(|(p, _)| *p == pid as i32) {
                    out.push((pid as i32, name));
                }
            }
        }
        out
    }

    fn front_app() -> Option<(i32, String)> {
        let system = El::wrap(unsafe { AXUIElementCreateSystemWide() })?;
        system.timeout(0.5);
        let app = system.child("AXFocusedApplication")?;
        app.timeout(0.5);
        Some((app.pid(), app.text("AXTitle")))
    }

    /// Find an app by name (or the one in front for "" / "front").
    fn resolve(name: &str) -> Result<(i32, String), String> {
        let me = std::process::id() as i32;
        let want = name.trim().to_lowercase();
        if want.is_empty() || want == "front" || want == "frontmost" {
            return match front_app() {
                Some((pid, n)) if pid != me => Ok((pid, n)),
                _ => Err("Jarvis is in front. Say which app to use.".into()),
            };
        }
        let apps = visible_apps();
        let exact = apps.iter().find(|(p, n)| *p != me && n.to_lowercase() == want);
        let part = apps.iter().find(|(p, n)| *p != me && n.to_lowercase().contains(&want));
        exact.or(part).cloned().ok_or_else(|| format!("“{name}” isn't open with a window. Open it first with mac_open. Open apps: {}.", apps.iter().filter(|(p, _)| *p != me).map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ")))
    }

    fn activate(pid: i32) {
        if let Some(app) = El::wrap(unsafe { AXUIElementCreateApplication(pid) }) {
            app.timeout(0.5);
            let key = CFString::new("AXFrontmost");
            unsafe { AXUIElementSetAttributeValue(app.raw(), key.as_concrete_TypeRef(), CFBoolean::true_value().as_CFTypeRef()) };
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    const MAX_CONTROLS: usize = 220;
    const MAX_VISITED: usize = 2500;
    const MAX_DEPTH: usize = 14;
    const MAX_CHARS: usize = 9000;

    /// Roles worth listing even without a label.
    const ALWAYS: [&str; 12] = ["button", "textfield", "textarea", "checkbox", "radiobutton", "popupbutton", "combobox", "slider", "link", "menuitem", "tab", "switch"];

    fn short(s: &str, n: usize) -> String {
        let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if one.chars().count() > n {
            format!("{}…", one.chars().take(n).collect::<String>())
        } else {
            one
        }
    }

    struct Walk {
        lines: Vec<String>,
        els: HashMap<u32, Held>,
        visited: usize,
        next: u32,
        chars: usize,
        truncated: bool,
    }

    fn walk(el: El, depth: usize, w: &mut Walk) {
        if w.visited >= MAX_VISITED || depth > MAX_DEPTH {
            w.truncated = true;
            return;
        }
        w.visited += 1;
        el.timeout(0.4);
        let role_raw = el.text("AXRole");
        let role = role_raw.trim_start_matches("AX").to_lowercase();
        let secure = el.text("AXSubrole") == "AXSecureTextField";
        let mut label = el.text("AXTitle");
        if label.is_empty() {
            label = el.text("AXDescription");
        }
        if label.is_empty() {
            label = el.text("AXPlaceholderValue");
        }
        let value = if secure { String::new() } else { el.text("AXValue") };
        let is_text = role == "statictext";
        let listed = !role.is_empty() && (ALWAYS.contains(&role.as_str()) || (!label.is_empty() && !matches!(role.as_str(), "group" | "window" | "scrollarea" | "splitgroup" | "toolbar" | "unknown")) || (is_text && !value.is_empty()));
        if listed && w.lines.len() < MAX_CONTROLS && w.chars < MAX_CHARS {
            let id = w.next;
            w.next += 1;
            let mut line = format!("{}[{id}] {}", "  ".repeat(depth.min(6)), if secure { "password field".to_string() } else { role.clone() });
            let name = if is_text { short(&value, 100) } else { short(&label, 80) };
            if !name.is_empty() {
                line.push_str(&format!(" “{name}”"));
            }
            if !is_text && !value.is_empty() && value != label {
                line.push_str(&format!(" = “{}”", short(&value, 80)));
            }
            if el.text("AXEnabled") == "false" {
                line.push_str(" (disabled)");
            }
            w.chars += line.len();
            w.lines.push(line);
            w.els.insert(id, Held(El(el.0.clone())));
        } else if listed {
            w.truncated = true;
        }
        for c in el.children() {
            walk(c, depth + 1, w);
        }
    }

    pub fn read(app: &str) -> Result<(i32, String, MacView), String> {
        trusted()?;
        let (pid, name) = resolve(app)?;
        let root = El::wrap(unsafe { AXUIElementCreateApplication(pid) }).ok_or("Couldn't reach that app.")?;
        root.timeout(0.5);
        let window = root.child("AXFocusedWindow").or_else(|| root.child("AXMainWindow")).ok_or("That app has no window to read.")?;
        let title = window.text("AXTitle");
        let mut w = Walk { lines: vec![], els: HashMap::new(), visited: 0, next: 1, chars: 0, truncated: false };
        walk(window, 0, &mut w);
        let view = MacView { app: name.clone(), window: title, controls: w.lines.join("\n"), truncated: w.truncated };
        *SNAP.lock().unwrap() = Some(Snapshot { pid, els: w.els });
        Ok((pid, name, view))
    }

    fn source() -> Result<CGEventSource, String> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| "Couldn't create an input event.".to_string())
    }

    fn click_at(x: f64, y: f64, count: i64) -> Result<(), String> {
        let src = source()?;
        let at = CGPoint { x, y };
        for (i, kind) in [CGEventType::MouseMoved, CGEventType::LeftMouseDown, CGEventType::LeftMouseUp].into_iter().enumerate() {
            let down = i == 1;
            let ev = CGEvent::new_mouse_event(src.clone(), kind, at, CGMouseButton::Left).map_err(|_| "Couldn't create a mouse event.".to_string())?;
            if i > 0 {
                ev.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, count);
            }
            ev.post(CGEventTapLocation::HID);
            std::thread::sleep(Duration::from_millis(if down { 40 } else { 20 }));
        }
        Ok(())
    }

    pub fn click(id: u32, double: bool) -> Result<String, String> {
        trusted()?;
        let (pid, el) = {
            let snap = SNAP.lock().unwrap();
            let s = snap.as_ref().ok_or("Read the app first (mac_read), then click a numbered control.")?;
            let held = s.els.get(&id).ok_or("There's no control with that number in the last read. Read the app again.")?;
            (s.pid, El(held.0 .0.clone()))
        };
        activate(pid);
        if !double && el.perform("AXPress") {
            return Ok("pressed".into());
        }
        let (x, y, w, h) = el.frame().ok_or("That control has no position on screen; read the app again.")?;
        click_at(x + w / 2.0, y + h / 2.0, if double { 2 } else { 1 })?;
        Ok("clicked".into())
    }

    /// Whether the focused control of an app is a password field.
    fn focused_is_secure(pid: i32) -> bool {
        El::wrap(unsafe { AXUIElementCreateApplication(pid) })
            .and_then(|a| {
                a.timeout(0.5);
                a.child("AXFocusedUIElement")
            })
            .map(|f| f.text("AXSubrole") == "AXSecureTextField")
            .unwrap_or(false)
    }

    fn key_event(code: u16, down: bool, flags: CGEventFlags) -> Result<(), String> {
        let ev = CGEvent::new_keyboard_event(source()?, code, down).map_err(|_| "Couldn't create a keyboard event.".to_string())?;
        ev.set_flags(flags);
        ev.post(CGEventTapLocation::HID);
        std::thread::sleep(Duration::from_millis(12));
        Ok(())
    }

    pub fn type_text(app: &str, text: &str) -> Result<String, String> {
        trusted()?;
        let (pid, _) = resolve(app)?;
        activate(pid);
        if focused_is_secure(pid) {
            return Err("That's a password field. Jarvis doesn't type passwords; the user has to.".into());
        }
        let mut buf: Vec<char> = vec![];
        let flush = |buf: &mut Vec<char>| -> Result<(), String> {
            for chunk in buf.chunks(12) {
                let s: String = chunk.iter().collect();
                for down in [true, false] {
                    let ev = CGEvent::new_keyboard_event(source()?, 0, down).map_err(|_| "Couldn't create a keyboard event.".to_string())?;
                    ev.set_string(&s);
                    ev.post(CGEventTapLocation::HID);
                    std::thread::sleep(Duration::from_millis(8));
                }
            }
            buf.clear();
            Ok(())
        };
        for c in text.chars().take(4000) {
            if c == '\n' {
                flush(&mut buf)?;
                key_event(36, true, CGEventFlags::empty())?;
                key_event(36, false, CGEventFlags::empty())?;
            } else {
                buf.push(c);
            }
        }
        flush(&mut buf)?;
        Ok(format!("typed {} characters", text.chars().count().min(4000)))
    }

    fn key_code(name: &str) -> Option<u16> {
        Some(match name {
            "a" => 0, "s" => 1, "d" => 2, "f" => 3, "h" => 4, "g" => 5, "z" => 6, "x" => 7, "c" => 8, "v" => 9, "b" => 11,
            "q" => 12, "w" => 13, "e" => 14, "r" => 15, "y" => 16, "t" => 17, "1" => 18, "2" => 19, "3" => 20, "4" => 21,
            "6" => 22, "5" => 23, "=" => 24, "9" => 25, "7" => 26, "-" => 27, "8" => 28, "0" => 29, "]" => 30, "o" => 31,
            "u" => 32, "[" => 33, "i" => 34, "p" => 35, "return" | "enter" => 36, "l" => 37, "j" => 38, "'" => 39, "k" => 40,
            ";" => 41, "\\" => 42, "," => 43, "/" => 44, "n" => 45, "m" => 46, "." => 47, "tab" => 48, "space" => 49, "`" => 50,
            "delete" | "backspace" => 51, "escape" | "esc" => 53, "forwarddelete" => 117, "home" => 115, "end" => 119,
            "pageup" => 116, "pagedown" => 121, "left" => 123, "right" => 124, "down" => 125, "up" => 126,
            "f1" => 122, "f2" => 120, "f3" => 99, "f4" => 118, "f5" => 96, "f6" => 97, "f7" => 98, "f8" => 100, "f9" => 101,
            "f10" => 109, "f11" => 103, "f12" => 111,
            _ => return None,
        })
    }

    /// "cmd+shift+t", "return", "escape".
    pub fn press(app: &str, combo: &str) -> Result<String, String> {
        trusted()?;
        let parts: Vec<String> = combo.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
        let (key, mods) = parts.split_last().ok_or("Say which key to press.")?;
        let code = key_code(key).ok_or_else(|| format!("I don't know the key “{key}”."))?;
        let mut flags = CGEventFlags::empty();
        for m in mods {
            flags |= match m.as_str() {
                "cmd" | "command" | "meta" => CGEventFlags::CGEventFlagCommand,
                "shift" => CGEventFlags::CGEventFlagShift,
                "alt" | "option" | "opt" => CGEventFlags::CGEventFlagAlternate,
                "ctrl" | "control" => CGEventFlags::CGEventFlagControl,
                other => return Err(format!("I don't know the modifier “{other}”.")),
            };
        }
        let (pid, _) = resolve(app)?;
        activate(pid);
        if focused_is_secure(pid) && mods.is_empty() && key.len() == 1 {
            return Err("That's a password field. Jarvis doesn't type passwords; the user has to.".into());
        }
        key_event(code, true, flags)?;
        key_event(code, false, flags)?;
        Ok(format!("pressed {combo}"))
    }

    pub fn scroll(app: &str, direction: &str, amount: i32) -> Result<String, String> {
        trusted()?;
        let (pid, _) = resolve(app)?;
        activate(pid);
        let root = El::wrap(unsafe { AXUIElementCreateApplication(pid) }).ok_or("Couldn't reach that app.")?;
        root.timeout(0.5);
        let win = root.child("AXFocusedWindow").ok_or("That app has no window.")?;
        let (x, y, w, h) = win.frame().ok_or("Couldn't find the window.")?;
        let src = source()?;
        let mv = CGEvent::new_mouse_event(src, CGEventType::MouseMoved, CGPoint { x: x + w / 2.0, y: y + h / 2.0 }, CGMouseButton::Left).map_err(|_| "Couldn't move the pointer.".to_string())?;
        mv.post(CGEventTapLocation::HID);
        std::thread::sleep(Duration::from_millis(40));
        let n = amount.clamp(1, 30);
        let (dy, dx) = match direction {
            "up" => (n, 0),
            "down" => (-n, 0),
            "left" => (0, n),
            "right" => (0, -n),
            _ => return Err("Direction must be up, down, left or right.".into()),
        };
        // core-graphics has no wrapper for scroll events; units 1 = lines. A null source is allowed.
        let raw = unsafe { CGEventCreateScrollWheelEvent2(std::ptr::null(), 1, 2, dy, dx, 0) };
        if raw.is_null() {
            return Err("Couldn't create a scroll event.".into());
        }
        unsafe {
            CGEventPost(0, raw);
            CFRelease(raw);
        }
        Ok(format!("scrolled {direction}"))
    }

    /// Open an app by name, or a web or mail link.
    pub fn open(target: &str) -> Result<String, String> {
        let t = target.trim();
        if t.is_empty() {
            return Err("Say what to open.".into());
        }
        let lower = t.to_lowercase();
        let status = if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
            Command::new("/usr/bin/open").arg(t).status()
        } else if t.chars().all(|c| c.is_alphanumeric() || " ._&+-".contains(c)) {
            Command::new("/usr/bin/open").args(["-a", t]).status()
        } else {
            return Err("I can only open apps by name and http, https or mailto links.".into());
        };
        match status {
            Ok(s) if s.success() => {
                std::thread::sleep(Duration::from_millis(900));
                Ok(format!("opened {t}"))
            }
            _ => Err(format!("Couldn't open “{t}”.")),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::MacView;
    const NO: &str = "Controlling apps is only available on macOS.";
    pub fn read(_: &str) -> Result<(i32, String, MacView), String> {
        Err(NO.into())
    }
    pub fn click(_: u32, _: bool) -> Result<String, String> {
        Err(NO.into())
    }
    pub fn type_text(_: &str, _: &str) -> Result<String, String> {
        Err(NO.into())
    }
    pub fn press(_: &str, _: &str) -> Result<String, String> {
        Err(NO.into())
    }
    pub fn scroll(_: &str, _: &str, _: i32) -> Result<String, String> {
        Err(NO.into())
    }
    pub fn open(_: &str) -> Result<String, String> {
        Err(NO.into())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// The window of an app as numbered controls. Reading doesn't change anything.
#[tauri::command]
pub async fn mac_read(app: Option<String>) -> Result<MacView, String> {
    let app = app.unwrap_or_default();
    blocking(move || imp::read(&app).map(|(_, _, v)| v)).await
}

#[tauri::command]
pub async fn mac_open(target: String) -> Result<String, String> {
    check_running()?;
    blocking(move || imp::open(&target)).await
}

#[tauri::command]
pub async fn mac_click(id: u32, double: Option<bool>) -> Result<String, String> {
    check_running()?;
    blocking(move || imp::click(id, double.unwrap_or(false))).await
}

#[tauri::command]
pub async fn mac_type(app: String, text: String) -> Result<String, String> {
    check_running()?;
    blocking(move || imp::type_text(&app, &text)).await
}

#[tauri::command]
pub async fn mac_key(app: String, combo: String) -> Result<String, String> {
    check_running()?;
    blocking(move || imp::press(&app, &combo)).await
}

#[tauri::command]
pub async fn mac_scroll(app: String, direction: String, amount: Option<i32>) -> Result<String, String> {
    check_running()?;
    blocking(move || imp::scroll(&app, &direction, amount.unwrap_or(5))).await
}

/// The user granted control of an app again after pressing stop.
#[tauri::command]
pub fn mac_resume() {
    HALTED.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub fn mac_halt() {
    halt();
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// Reads the Finder window as a smoke test: `cargo test -- --ignored --nocapture mac_read_finder`.
    /// Needs the Accessibility permission for the program running the test, and Finder open with a window.
    #[test]
    #[ignore]
    fn mac_read_finder() {
        let (_, name, view) = super::imp::read("Finder").unwrap();
        println!("{name} — {}\n{}", view.window, view.controls);
        assert!(!view.controls.is_empty());
    }
}
