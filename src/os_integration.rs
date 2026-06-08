// macOS-only text injection module.
//
// Injection strategy — no osascript, no Automation permission needed:
//
//   1. Write the refined text to the macOS General Pasteboard via `arboard`.
//      arboard calls NSPasteboard internally; the write is synchronous and
//      immediately visible to all processes.
//
//   2. Synthesize a Cmd+V key-press pair (KeyDown + KeyUp) using the macOS
//      Core Graphics event API (CGEventCreateKeyboardEvent / CGEventPost).
//      The events are posted to CGEventTapLocation::HID — the same point in
//      the event pipeline that real hardware keyboard input enters.
//
// Permission model:
//   • No "Automation" permission (that's only required for Apple Events /
//     osascript `tell application X …` constructs).
//   • Accessibility permission IS required for CGEventPost to reach other
//     processes.  Grant it once in:
//       System Settings → Privacy & Security → Accessibility
//   • The app is NOT sandboxed (entitlements.plist: app-sandbox = false),
//     so CGEventPost works without any special entitlement key.

use anyhow::Result;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// Virtual key-code for the 'V' key on a US-layout keyboard (layout-independent
/// on macOS — CGEventCreateKeyboardEvent uses hardware scan-codes, not characters).
const VKEY_V: u16 = 9;

/// Injects `text` into the currently focused application:
///   1. Writes the text to the macOS General Pasteboard.
///   2. Synthesizes a Cmd+V KeyDown + KeyUp pair via CGEventPost → HID tap.
///
/// Must be called from the **main thread** (Tauri's `run_on_main_thread` callback)
/// so CGEventPost has access to the active window-server session.
#[cfg(target_os = "macos")]
pub fn inject_text(text: &str) -> Result<()> {
    // Write the text to the macOS General Pasteboard via arboard (native, no subprocess)
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| anyhow::anyhow!("Failed to initialize arboard clipboard: {:?}", e))?;
    cb.set_text(text.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to copy text to clipboard: {:?}", e))?;

    // Short delay to ensure system pasteboard registers the write
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Cmd+V simulation using CombinedSessionState (more reliable for targeting other apps)
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), VKEY_V, true)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent KeyDown"))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(50));

    let key_up = CGEvent::new_keyboard_event(source, VKEY_V, false)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent KeyUp"))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Synthesizes a Cmd+C KeyDown + KeyUp pair to copy selected text to clipboard.
#[cfg(target_os = "macos")]
pub fn simulate_copy() -> Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), 8, true)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent KeyDown"))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(50));

    let key_up = CGEvent::new_keyboard_event(source, 8, false)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent KeyUp"))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Compile-time guard — this module is macOS-exclusive.
#[cfg(not(target_os = "macos"))]
pub fn inject_text(_text: &str) -> Result<()> {
    anyhow::bail!("inject_text is only supported on macOS");
}

#[cfg(not(target_os = "macos"))]
pub fn simulate_copy() -> Result<()> {
    anyhow::bail!("simulate_copy is only supported on macOS");
}

/// Plays a short system sound for start, stop, or completion events.
pub fn play_sound(sound_type: &str) {
    let sound_type = sound_type.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let sound_path = match sound_type.as_str() {
                "start" => "/System/Library/Sounds/Tink.aiff",
                "stop" => "/System/Library/Sounds/Pop.aiff",
                "complete" => "/System/Library/Sounds/Glass.aiff",
                "processing" => "/System/Library/Sounds/Ping.aiff",
                _ => return,
            };
            let _ = std::process::Command::new("afplay")
                .arg(sound_path)
                .status();
        }

        #[cfg(target_os = "windows")]
        {
            let ps_command = match sound_type.as_str() {
                "start" => "[System.Media.SystemSounds]::Beep.Play()",
                "stop" => "[System.Media.SystemSounds]::Hand.Play()",
                "complete" => "[System.Media.SystemSounds]::Asterisk.Play()",
                "processing" => "[System.Media.SystemSounds]::Question.Play()",
                _ => return,
            };
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", ps_command])
                .status();
        }
    });
}
