//! Native microphone permission. WKWebView only gets audio devices after the app itself
//! has been granted microphone access by macOS, so we ask AVFoundation directly.

#[cfg(target_os = "macos")]
mod imp {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    pub fn status() -> &'static str {
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else { return "unknown" };
        let st = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
        match st {
            AVAuthorizationStatus::Authorized => "granted",
            AVAuthorizationStatus::Denied => "denied",
            AVAuthorizationStatus::Restricted => "restricted",
            _ => "undetermined",
        }
    }

    pub async fn request() -> bool {
        match status() {
            "granted" => return true,
            "denied" | "restricted" => return false,
            _ => {}
        }
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else { return false };
        let (tx, rx) = oneshot::channel::<bool>();
        {
            // AVFoundation copies the block, so it can be dropped before we await.
            let tx = Mutex::new(Some(tx));
            let block = RcBlock::new(move |granted: Bool| {
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(granted.as_bool());
                }
            });
            unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &block) };
        }
        rx.await.unwrap_or(false)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn status() -> &'static str {
        "granted"
    }
    pub async fn request() -> bool {
        true
    }
}

#[tauri::command]
pub fn mic_status() -> String {
    imp::status().to_string()
}

/// Shows the macOS "allow microphone" prompt the first time; afterwards returns the saved answer.
#[tauri::command]
pub async fn request_mic() -> bool {
    imp::request().await
}

#[tauri::command]
pub fn open_mic_settings() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .spawn();
}
