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

/// Virtual key codes on macOS (hardware scan-codes, layout-independent).
const VKEY_CMD: u16 = 55; // kVK_Command
const VKEY_C: u16 = 8;   // kVK_ANSI_C
const VKEY_V: u16 = 9;   // kVK_ANSI_V

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Checks whether the current process has been granted macOS Accessibility permissions.
pub fn is_accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        AXIsProcessTrusted()
    }
    #[cfg(not(target_os = "macos"))]
    true
}

/// Injects `text` into the currently focused application:
///   1. Writes the text to the macOS General Pasteboard.
///   2. Synthesizes a full Cmd+V key-press sequence (Cmd Down -> V Down -> V Up -> Cmd Up).
///
/// Must be called from the **main thread** (Tauri's `run_on_main_thread` callback)
/// so CGEventPost has access to the active window-server session.
#[cfg(target_os = "macos")]
pub fn inject_text(text: &str) -> Result<()> {
    if !is_accessibility_trusted() {
        eprintln!("⚠️ Accessibility permission not granted! Keystroke injection may be blocked by macOS.");
    }

    // Write the text to the macOS General Pasteboard via arboard (native, no subprocess)
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| anyhow::anyhow!("Failed to initialize arboard clipboard: {:?}", e))?;
    cb.set_text(text.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to copy text to clipboard: {:?}", e))?;

    // Delay to ensure system pasteboard registers the write
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Cmd+V simulation using CombinedSessionState
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

    // 1. Command Down
    let cmd_down = CGEvent::new_keyboard_event(source.clone(), VKEY_CMD, true)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent Cmd Down"))?;
    cmd_down.set_flags(CGEventFlags::CGEventFlagCommand);
    cmd_down.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(25));

    // 2. V Down
    let v_down = CGEvent::new_keyboard_event(source.clone(), VKEY_V, true)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent V Down"))?;
    v_down.set_flags(CGEventFlags::CGEventFlagCommand);
    v_down.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(50));

    // 3. V Up
    let v_up = CGEvent::new_keyboard_event(source.clone(), VKEY_V, false)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent V Up"))?;
    v_up.set_flags(CGEventFlags::CGEventFlagCommand);
    v_up.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(25));

    // 4. Command Up
    let cmd_up = CGEvent::new_keyboard_event(source, VKEY_CMD, false)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent Cmd Up"))?;
    cmd_up.set_flags(CGEventFlags::empty());
    cmd_up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Synthesizes a full Cmd+C key-press sequence to copy selected text to clipboard.
#[cfg(target_os = "macos")]
pub fn simulate_copy() -> Result<()> {
    if !is_accessibility_trusted() {
        eprintln!("⚠️ Accessibility permission not granted! Keystroke copy may be blocked by macOS.");
    }

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

    // 1. Command Down
    let cmd_down = CGEvent::new_keyboard_event(source.clone(), VKEY_CMD, true)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent Cmd Down"))?;
    cmd_down.set_flags(CGEventFlags::CGEventFlagCommand);
    cmd_down.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(25));

    // 2. C Down
    let c_down = CGEvent::new_keyboard_event(source.clone(), VKEY_C, true)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent C Down"))?;
    c_down.set_flags(CGEventFlags::CGEventFlagCommand);
    c_down.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(50));

    // 3. C Up
    let c_up = CGEvent::new_keyboard_event(source.clone(), VKEY_C, false)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent C Up"))?;
    c_up.set_flags(CGEventFlags::CGEventFlagCommand);
    c_up.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(25));

    // 4. Command Up
    let cmd_up = CGEvent::new_keyboard_event(source, VKEY_CMD, false)
        .map_err(|_| anyhow::anyhow!("Failed to create CGEvent Cmd Up"))?;
    cmd_up.set_flags(CGEventFlags::empty());
    cmd_up.post(CGEventTapLocation::HID);

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
