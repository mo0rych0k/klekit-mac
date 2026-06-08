use std::sync::Arc;
use klekit::{AppState as EngineState, AppSettings, RecorderCommand, VoiceAssistantEngine};
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager, RunEvent, Emitter,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use klekit::os_integration;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use block2::StackBlock;
use std::ptr::NonNull;


pub struct AppState {
    pub settings: Arc<Mutex<Arc<AppSettings>>>,
    pub engine: Arc<VoiceAssistantEngine>,
    pub recorder_tx: crossbeam_channel::Sender<RecorderCommand>,
}

pub fn parse_shortcut(value: &str) -> Option<tauri_plugin_global_shortcut::Shortcut> {
    if value.trim().is_empty() { return None; }
    use tauri_plugin_global_shortcut::{Shortcut, Modifiers, Code};
    let parts: Vec<&str> = value.split('+').collect();
    if parts.is_empty() { return None; }
    
    let mut modifiers = Modifiers::empty();
    let mut key_code = None;

    for part in parts {
        let clean = part.trim().to_lowercase();
        match clean.as_str() {
            "command" | "cmd" | "meta" | "super" => modifiers.insert(Modifiers::SUPER),
            "alt" | "option" => modifiers.insert(Modifiers::ALT),
            "control" | "ctrl" => modifiers.insert(Modifiers::CONTROL),
            "shift" => modifiers.insert(Modifiers::SHIFT),
            other => {
                key_code = match other {
                    "f1" => Some(Code::F1),
                    "f2" => Some(Code::F2),
                    "f3" => Some(Code::F3),
                    "f4" => Some(Code::F4),
                    "f5" => Some(Code::F5),
                    "f6" => Some(Code::F6),
                    "f7" => Some(Code::F7),
                    "f8" => Some(Code::F8),
                    "f9" => Some(Code::F9),
                    "f10" => Some(Code::F10),
                    "f11" => Some(Code::F11),
                    "f12" => Some(Code::F12),
                    "capslock" => Some(Code::CapsLock),
                    "escape" => Some(Code::Escape),
                    "enter" | "return" => Some(Code::Enter),
                    "space" => Some(Code::Space),
                    "a" => Some(Code::KeyA),
                    "b" => Some(Code::KeyB),
                    "c" => Some(Code::KeyC),
                    "d" => Some(Code::KeyD),
                    "e" => Some(Code::KeyE),
                    "f" => Some(Code::KeyF),
                    "g" => Some(Code::KeyG),
                    "h" => Some(Code::KeyH),
                    "i" => Some(Code::KeyI),
                    "j" => Some(Code::KeyJ),
                    "k" => Some(Code::KeyK),
                    "l" => Some(Code::KeyL),
                    "m" => Some(Code::KeyM),
                    "n" => Some(Code::KeyN),
                    "o" => Some(Code::KeyO),
                    "p" => Some(Code::KeyP),
                    "q" => Some(Code::KeyQ),
                    "r" => Some(Code::KeyR),
                    "s" => Some(Code::KeyS),
                    "t" => Some(Code::KeyT),
                    "u" => Some(Code::KeyU),
                    "v" => Some(Code::KeyV),
                    "w" => Some(Code::KeyW),
                    "x" => Some(Code::KeyX),
                    "y" => Some(Code::KeyY),
                    "z" => Some(Code::KeyZ),
                    "0" => Some(Code::Digit0),
                    "1" => Some(Code::Digit1),
                    "2" => Some(Code::Digit2),
                    "3" => Some(Code::Digit3),
                    "4" => Some(Code::Digit4),
                    "5" => Some(Code::Digit5),
                    "6" => Some(Code::Digit6),
                    "7" => Some(Code::Digit7),
                    "8" => Some(Code::Digit8),
                    "9" => Some(Code::Digit9),
                    _ => None,
                };
            }
        }
    }

    key_code.map(|code| Shortcut::new(Some(modifiers), code))
}

mod commands {
    use super::{AppState, AppSettings, parse_shortcut};
    use std::sync::Arc;
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    #[tauri::command]
    pub fn get_settings(state: tauri::State<'_, AppState>) -> AppSettings {
        let lock = state.settings.lock().unwrap();
        let settings = (*lock.clone()).clone();
        settings
    }

    #[tauri::command]
    pub fn save_settings(
        new_settings: AppSettings,
        state: tauri::State<'_, AppState>,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let mut new_settings = new_settings;
        for agent in &mut new_settings.agents {
            if agent.hotkey_value.starts_with("MouseButton") {
                agent.hotkey_type = "Mouse".to_string();
            } else {
                agent.hotkey_type = "Keyboard".to_string();
            }
        }

        // 1. Enforce agent profile cap
        if new_settings.agents.len() > 10 {
            state.engine.log("[commands] save_settings error: Maximum limit of 10 agents reached");
            return Err("Maximum limit of 10 agents reached".to_string());
        }

        // 1.5. Ensure at least one agent profile is present and active
        if new_settings.agents.is_empty() {
            state.engine.log("[commands] save_settings error: At least one agent profile must be present");
            return Err("At least one agent profile must be present".to_string());
        }
        if !new_settings.agents.iter().any(|a| a.is_active) {
            state.engine.log("[commands] save_settings error: At least one agent profile must be active");
            return Err("At least one agent profile must be active".to_string());
        }

        // 2. Validate cross-agent duplicate hotkeys (ignore empty hotkeys)
        let mut seen_hotkeys = std::collections::HashSet::new();
        for agent in &new_settings.agents {
            if !agent.hotkey_value.trim().is_empty() {
                if !seen_hotkeys.insert((&agent.hotkey_type, &agent.hotkey_value)) {
                    let err_msg = format!("Duplicate hotkey assigned to multiple agents: {}", agent.hotkey_value);
                    state.engine.log(format!("[commands] save_settings error: {}", err_msg));
                    return Err(err_msg);
                }
            }
        }

        // Save to SQLite database
        state.engine.db.save_agents(&new_settings.agents).map_err(|e| {
            state.engine.log(format!("[commands] save_settings DB error: {:?}", e));
            e.to_string()
        })?;

        // 3. Reconcile global hotkeys
        let _ = app.global_shortcut().unregister_all();

        // 4. Save and Register active keyboard profiles
        for agent in &new_settings.agents {
            if agent.is_active && agent.hotkey_type == "Keyboard" {
                if let Some(shortcut) = parse_shortcut(&agent.hotkey_value) {
                    state.engine.log(format!("[global-shortcut] Registering keyboard shortcut '{}' for agent '{}'", agent.hotkey_value, agent.name));
                    if let Err(e) = app.global_shortcut().register(shortcut) {
                        state.engine.log(format!("[global-shortcut] ❌ Failed to register keyboard shortcut '{}' for agent '{}': {:?}", agent.hotkey_value, agent.name, e));
                        return Err(format!("Failed to register hotkey for agent '{}': {:?}", agent.name, e));
                    }
                    state.engine.log(format!("[global-shortcut] ✅ Registered keyboard shortcut '{}' for agent '{}'", agent.hotkey_value, agent.name));
                }
            }
        }

        // 4.5. Register global voice recording shortcut
        if !new_settings.voice_hotkey_value.trim().is_empty() && new_settings.voice_hotkey_type == "Keyboard" {
            if let Some(shortcut) = parse_shortcut(&new_settings.voice_hotkey_value) {
                state.engine.log(format!("[global-shortcut] Registering global voice shortcut '{}'", new_settings.voice_hotkey_value));
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    state.engine.log(format!("[global-shortcut] ❌ Failed to register global voice shortcut '{}': {:?}", new_settings.voice_hotkey_value, e));
                    return Err(format!("Failed to register global voice shortcut: {:?}", e));
                }
                state.engine.log(format!("[global-shortcut] ✅ Registered global voice shortcut '{}'", new_settings.voice_hotkey_value));
            }
        }

        // Swap atomic pointer
        {
            let mut lock = state.settings.lock().unwrap();
            *lock = Arc::new(new_settings.clone());
        }

        // Flush asynchronously to SSD config file
        let engine_save = Arc::clone(&state.engine);
        tauri::async_runtime::spawn(async move {
            engine_save.log("[commands] save_settings: flushing configuration to SSD on background task...");
            match new_settings.save() {
                Ok(_) => engine_save.log("[commands] save_settings: configuration file saved successfully"),
                Err(e) => engine_save.log(format!("[commands] save_settings error: failed to save config file: {:?}", e)),
            }
        });

        Ok(())
    }

    #[tauri::command]
    pub fn test_hotkey(hotkey: String, app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
        let shortcut = parse_shortcut(&hotkey).ok_or_else(|| {
            "Invalid hotkey format".to_string()
        })?;
        
        state.engine.log(format!("[global-shortcut] Testing hotkey '{}' speculative registration...", hotkey));

        // Speculative bind: Register
        if let Err(e) = app.global_shortcut().register(shortcut.clone()) {
            state.engine.log(format!("[global-shortcut] ❌ Speculative registration failed for hotkey '{}': {:?}", hotkey, e));
            eprintln!("⚠️ Speculative registration failed for hotkey '{}': {:?}", hotkey, e);
            
            let is_macos_hijack_candidate = hotkey.contains("F9") || hotkey.contains("F10") || hotkey.contains("F11") || hotkey.contains("F12");
            if cfg!(target_os = "macos") && is_macos_hijack_candidate {
                return Err(format!(
                    "Hotkey '{}' may be globally hijacked by macOS Mission Control. Please disable it in Keyboard Shortcuts -> Mission Control, or choose another hotkey.",
                    hotkey
                ));
            }
            return Err(format!("Hotkey already globally reserved or conflicts with system actions: {:?}", e));
        }

        // Unregister immediately to prevent key hijacking/swallowing during recording
        let _ = app.global_shortcut().unregister(shortcut);

        state.engine.log(format!("[global-shortcut] ✅ Speculative registration succeeded for hotkey '{}'", hotkey));
        Ok(())
    }

    #[tauri::command]
    pub fn get_history_page(page: i64, state: tauri::State<'_, AppState>) -> Result<Vec<klekit::db::LogRecord>, String> {
        let res = state.engine.db.get_logs_page(50, page * 50).map_err(|e| {
            e.to_string()
        });
        res
    }

    #[tauri::command]
    pub fn delete_history_item(id: i64, state: tauri::State<'_, AppState>) -> Result<(), String> {
        let res = state.engine.db.delete_log(id).map_err(|e| {
            e.to_string()
        });
        res
    }

    #[tauri::command]
    pub fn clear_history(state: tauri::State<'_, AppState>) -> Result<(), String> {
        let res = state.engine.db.clear_history().map_err(|e| {
            e.to_string()
        });
        res
    }

    #[tauri::command]
    pub fn get_preset_blueprints() -> Vec<klekit::settings::PresetBlueprint> {
        klekit::settings::load_gemma_prompts().presets
    }

    #[tauri::command]
    pub fn open_url(url: String) {
        // Validate URL to prevent command/argument injection
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return;
        }
        if url.chars().any(|c| c.is_whitespace() || ['&', '|', ';', '$', '>', '<', '`', '\\', '\''].contains(&c)) {
            return;
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&url).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
    }

    #[tauri::command]
    pub fn refine_text(
        text: String,
        agent_id: String,
        state: tauri::State<'_, AppState>,
    ) -> Result<String, String> {
        state.engine.run_text_refinement(&text, &agent_id)
            .map_err(|e| e.to_string())
    }
}



static MOUSE_LISTENER_SPAWNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn spawn_mouse_listener(
    settings_ref: Arc<Mutex<Arc<AppSettings>>>,
    engine: Arc<VoiceAssistantEngine>,
    recorder_tx: crossbeam_channel::Sender<RecorderCommand>,
) {
    if MOUSE_LISTENER_SPAWNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let engine_c = Arc::clone(&engine);
    let engine_c_local = Arc::clone(&engine);

    engine.log("[global-mouse] Registering global and local mouse monitors on main thread...");

    let handle_mouse_event = move |event_type: NSEventType, event: &NSEvent| {

        let settings = {
            let lock = settings_ref.lock().unwrap();
            Arc::clone(&*lock)
        };

        let is_voice_mouse = if settings.voice_hotkey_type == "Mouse" {
            let button_number = unsafe { event.buttonNumber() };
            let target_number = match settings.voice_hotkey_value.as_str() {
                "LeftClick" | "MouseButton1"  => Some(0),
                "RightClick" | "MouseButton2" => Some(1),
                "MiddleClick" | "MouseButton3"=> Some(2),
                "MouseButton4"               => Some(3),
                "MouseButton5"               => Some(4),
                _ => None,
            };
            target_number.map_or(false, |tn| button_number == tn)
        } else {
            false
        };

        let is_press = event_type == NSEventType::LeftMouseDown || 
                       event_type == NSEventType::RightMouseDown || 
                       event_type == NSEventType::OtherMouseDown;

        let e = Arc::clone(&engine);
        let tx = recorder_tx.clone();

        if is_voice_mouse {
            if is_press {
                e.log(format!("[global-mouse] Global voice recording Pressed '{}'", settings.voice_hotkey_value));
                tauri::async_runtime::spawn(async move {
                    let _ = VoiceAssistantEngine::start_recording_for_agent(e, tx, "global_voice".to_string());
                });
            } else {
                e.log(format!("[global-mouse] Global voice recording Released '{}'", settings.voice_hotkey_value));
                tauri::async_runtime::spawn(async move {
                    let _ = VoiceAssistantEngine::stop_and_process_for_agent(e, tx, "global_voice".to_string());
                });
            }
            return;
        }

        let matched = settings.agents.iter().find(|agent| {
            if !agent.is_active || agent.hotkey_type != "Mouse" { return false; }
            let button_number = unsafe { event.buttonNumber() };
            let target_number = match agent.hotkey_value.as_str() {
                "LeftClick" | "MouseButton1"  => Some(0),
                "RightClick" | "MouseButton2" => Some(1),
                "MiddleClick" | "MouseButton3"=> Some(2),
                "MouseButton4"               => Some(3),
                "MouseButton5"               => Some(4),
                _ => None,
            };
            target_number.map_or(false, |tn| button_number == tn)
        });

        if let Some(agent) = matched {
            let agent_id = agent.id.clone();
            if is_press {
                e.log(format!("[global-mouse] MATCH Pressed '{}' for agent '{}'", agent.hotkey_value, agent.name));
                run_os_refinement(e, agent_id);
            }
        }
    };

    let handle_mouse_event = Arc::new(handle_mouse_event);
    let handle_mouse_event_global = Arc::clone(&handle_mouse_event);
    let handle_mouse_event_local = Arc::clone(&handle_mouse_event);

    unsafe {
        // Event mask for catching clicks in the background (outside the app window)
        let mask_bits = NSEventMask::LeftMouseDown.bits()
            | NSEventMask::LeftMouseUp.bits()
            | NSEventMask::RightMouseDown.bits()
            | NSEventMask::RightMouseUp.bits()
            | NSEventMask::OtherMouseDown.bits()
            | NSEventMask::OtherMouseUp.bits();

        let event_mask = NSEventMask::from_bits_retain(mask_bits);

        // 1. Global event monitor (clicks outside the app window)
        let handler = StackBlock::new(move |event: NonNull<NSEvent>| {
            let event = event.as_ref();
            let event_type = event.r#type();
            handle_mouse_event_global(event_type, event);
        });

        let handler = handler.copy();
        let global_monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(event_mask, &handler);
        if global_monitor.is_some() {
            engine_c.log("[global-mouse] ✅ Global mouse monitor registered successfully!");
        } else {
            engine_c.log("[global-mouse] ❌ Failed to register global mouse monitor (returned None)!");
        }
        std::mem::forget(global_monitor);

        // 2. Local event monitor (clicks inside the app window)
        let handler_local = StackBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = event.as_ref();
            let event_type = event_ref.r#type();
            handle_mouse_event_local(event_type, event_ref);
            event.as_ptr()
        });

        let handler_local = handler_local.copy();
        let local_monitor = NSEvent::addLocalMonitorForEventsMatchingMask_handler(event_mask, &handler_local);
        if local_monitor.is_some() {
            engine_c_local.log("[global-mouse] ✅ Local mouse monitor registered successfully!");
        } else {
            engine_c_local.log("[global-mouse] ❌ Failed to register local mouse monitor (returned None)!");
        }
        std::mem::forget(local_monitor);
    }
}

fn run_os_refinement(engine: Arc<VoiceAssistantEngine>, agent_id: String) {
    tauri::async_runtime::spawn(async move {
        engine.log(format!("[global-refine] Starting OS-wide refinement for agent '{}'...", agent_id));
        if let Err(e) = os_integration::simulate_copy() {
            engine.log(format!("❌ Failed to simulate copy: {:?}", e));
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        
        let clipboard_text = match arboard::Clipboard::new() {
            Ok(mut cb) => cb.get_text().unwrap_or_default(),
            Err(_) => String::new(),
        };
        
        if clipboard_text.trim().is_empty() {
            engine.log("⚠️ Clipboard is empty, nothing to refine");
            return;
        }
        
        engine.log(format!("[global-refine] Selected text to refine: '{}'", clipboard_text));
        match engine.run_text_refinement(&clipboard_text, &agent_id) {
            Ok(refined) => {
                engine.log(format!("[global-refine] Refined text: '{}'", refined));
                if let Err(e) = os_integration::inject_text(&refined) {
                    engine.log(format!("❌ Failed to inject refined text: {:?}", e));
                }
            }
            Err(e) => {
                engine.log(format!("❌ Refinement failed: {:?}", e));
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load settings from disk
    let initial_settings = AppSettings::load();
    let shared_settings: Arc<Mutex<Arc<AppSettings>>> =
        Arc::new(Mutex::new(Arc::new(initial_settings)));

    // 1. Initialize our engine — pass shared_settings so the pipeline reads configuration
    let engine = Arc::new(VoiceAssistantEngine::new(
        "./models/ggml-large-v3-turbo-q5_0.bin",
        "./models/gemma-4-E2B-it.litertlm",
        "./bin/llm_refiner",
        Arc::clone(&shared_settings),
    ));

    // Start inactivity weight unloading timer (Zero-Memory Idle)
    VoiceAssistantEngine::start_inactivity_timer(Arc::clone(&engine));

    // Create control channels for local audio recorder on dedicated OS thread
    let (recorder_tx, recorder_rx) = crossbeam_channel::unbounded::<RecorderCommand>();

    // AppState shares the SAME Arc — save_settings pointer-swap reaches the engine instantly
    let app_state = AppState {
        settings: Arc::clone(&shared_settings),
        engine: Arc::clone(&engine),
        recorder_tx: recorder_tx.clone(),
    };

    // Start dedicated local thread for AudioRecorder (cpal::Stream is !Send and !Sync)
    std::thread::spawn(move || {
        use klekit::audio;
        let mut recorder = audio::AudioRecorder::new();
        while let Ok(cmd) = recorder_rx.recv() {
            match cmd {
                RecorderCommand::Start => {
                    if let Err(e) = recorder.start_recording() {
                        eprintln!("❌ Failed to start audio recording in GUI: {:?}", e);
                    }
                }
                RecorderCommand::Stop(res_tx) => {
                    let res = recorder.stop_recording();
                    let _ = res_tx.send(res);
                }
            }
        }
    });


    // Clone Arcs for closures that need them before setup
    let engine_toggle        = Arc::clone(&engine);
    let recorder_tx_toggle   = recorder_tx.clone();
    let settings_for_shortcut = Arc::clone(&app_state.settings);
    let engine_shortcut      = Arc::clone(&engine);
    let rec_tx_shortcut      = recorder_tx.clone();

    tauri::Builder::default()
        .manage(app_state)
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |_app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;

                    engine_shortcut.log(format!(
                        "[global-shortcut] Event received: shortcut={:?}, state={:?}",
                        shortcut, event.state
                    ));

                    // Atomic pointer read — nanoseconds
                    let settings = {
                        let lock = settings_for_shortcut.lock().unwrap();
                        Arc::clone(&*lock)
                    };

                    let is_voice_shortcut = settings.voice_hotkey_type == "Keyboard" &&
                        parse_shortcut(&settings.voice_hotkey_value).map_or(false, |s| s == *shortcut);

                    if is_voice_shortcut {
                        let e = Arc::clone(&engine_shortcut);
                        let tx = rec_tx_shortcut.clone();
                        engine_shortcut.log(format!(
                            "[global-shortcut] Global voice recording shortcut matched with state {:?}",
                            event.state
                        ));
                        match event.state {
                            ShortcutState::Pressed => {
                                tauri::async_runtime::spawn(async move {
                                    let _ = VoiceAssistantEngine::start_recording_for_agent(e, tx, "global_voice".to_string());
                                });
                            }
                            ShortcutState::Released => {
                                tauri::async_runtime::spawn(async move {
                                    let _ = VoiceAssistantEngine::stop_and_process_for_agent(e, tx, "global_voice".to_string());
                                });
                            }
                        }
                        return;
                    }

                    let matched_agent = settings.agents.iter().find(|a| {
                        a.is_active &&
                        a.hotkey_type == "Keyboard" && 
                        parse_shortcut(&a.hotkey_value).map_or(false, |s| s == *shortcut)
                    });

                    if let Some(agent) = matched_agent {
                        let agent_id = agent.id.clone();
                        let e = Arc::clone(&engine_shortcut);
                        engine_shortcut.log(format!(
                            "[global-shortcut] Match for agent '{}' ({}) with state {:?}",
                            agent.name, agent.id, event.state
                        ));
                        match event.state {
                            ShortcutState::Pressed => {
                                run_os_refinement(e, agent_id);
                            }
                            _ => {}
                        }
                    } else {
                        engine_shortcut.log("[global-shortcut] No matching agent or voice shortcut found".to_string());
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::test_hotkey,
            commands::get_history_page,
            commands::delete_history_item,
            commands::clear_history,
            commands::get_preset_blueprints,
            commands::open_url,
            commands::refine_text
        ])
        .setup(move |app| {
            // Configure logging for debugging
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // 2. System tray menu items
            let show_dashboard = MenuItemBuilder::with_id("dashboard", "Open Window").build(app)?;
            let quit_item     = MenuItemBuilder::with_id("quit",      "Exit").build(app)?;

            let menu = MenuBuilder::new(app)
                .items(&[&show_dashboard, &quit_item])
                .build()?;

            // 3. System tray icon
            let mut tray_builder = TrayIconBuilder::with_id("main-tray").menu(&menu);
            if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/status_bar_icons/status_bar_32_dark.png")) {
                tray_builder = tray_builder.icon(icon);
            } else if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            let _tray = tray_builder
                .on_menu_event(move |app_handle, event| {
                    match event.id().as_ref() {
                        "quit" => { std::process::exit(0); }
                        "dashboard" => {
                            if let Some(w) = app_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;



            // 4. Custom macOS application menu bar
            #[cfg(target_os = "macos")]
            {
                let app_name = "KleKit";
                
                // Create custom "About KleKit" and "Exit" menu items
                let open_window_item = tauri::menu::MenuItemBuilder::with_id("about_klekit", "About KleKit")
                    .build(app)?;
                
                let exit_item = tauri::menu::MenuItemBuilder::with_id("quit", "Exit")
                    .accelerator("Cmd+Q")
                    .build(app)?;

                // Build the KleKit Submenu
                let klekit_submenu = tauri::menu::SubmenuBuilder::new(app, app_name)
                    .items(&[&open_window_item, &exit_item])
                    .build()?;

                // Build the main menu with just the KleKit submenu
                let main_menu = tauri::menu::MenuBuilder::new(app)
                    .items(&[&klekit_submenu])
                    .build()?;

                // Set the application menu bar
                app.set_menu(main_menu)?;

                // Register event handler for the application menu items
                app.on_menu_event(move |app_handle, event| {
                    match event.id().as_ref() {
                        "quit" => {
                            std::process::exit(0);
                        }
                        "about_klekit" => {
                            if let Some(w) = app_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                                let _ = w.eval("if (window.showAboutModal) { window.showAboutModal(); }");
                            }
                        }
                        "dashboard" => {
                            if let Some(w) = app_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        _ => {}
                    }
                });
            }

            // 5. Register the boot-time configured keyboard shortcuts for all agents
            {
                let boot_settings = {
                    let state: tauri::State<'_, AppState> = app.state();
                    let lock = state.settings.lock().unwrap();
                    Arc::clone(&*lock)
                };

                engine_toggle.log("[global-shortcut] Registering boot keyboard shortcuts...".to_string());
                for agent in &boot_settings.agents {
                    if agent.is_active && agent.hotkey_type == "Keyboard" {
                        if let Some(shortcut) = parse_shortcut(&agent.hotkey_value) {
                            match app.global_shortcut().register(shortcut.clone()) {
                                Ok(_) => engine_toggle.log(format!(
                                    "[global-shortcut] ✅ Registered boot hotkey '{}' for agent '{}'",
                                    agent.hotkey_value, agent.name
                                )),
                                Err(e) => engine_toggle.log(format!(
                                    "[global-shortcut] ❌ Failed to register boot hotkey '{}' for agent '{}': {:?}",
                                    agent.hotkey_value, agent.name, e
                                )),
                            }
                        } else {
                            engine_toggle.log(format!(
                                "[global-shortcut] ⚠️ Failed to parse hotkey value '{}' for agent '{}'",
                                agent.hotkey_value, agent.name
                            ));
                        }
                    }
                }

                // Register global voice hotkey on boot
                if !boot_settings.voice_hotkey_value.trim().is_empty() && boot_settings.voice_hotkey_type == "Keyboard" {
                    if let Some(shortcut) = parse_shortcut(&boot_settings.voice_hotkey_value) {
                        match app.global_shortcut().register(shortcut) {
                            Ok(_) => engine_toggle.log(format!(
                                "[global-shortcut] ✅ Registered boot global voice hotkey '{}'",
                                boot_settings.voice_hotkey_value
                            )),
                            Err(e) => engine_toggle.log(format!(
                                "[global-shortcut] ❌ Failed to register boot global voice hotkey '{}': {:?}",
                                boot_settings.voice_hotkey_value, e
                            )),
                        }
                    }
                }
            }



            // 7. Bind engine state changes to the Dashboard webview
            let app_handle = app.handle().clone();
            let engine_status = Arc::clone(&engine_toggle);
            engine_toggle.set_status_callback(move |new_state| {
                let active_agent_id = {
                    let aid = engine_status.active_agent_id.lock().unwrap();
                    aid.clone().unwrap_or_default()
                };

                let status_str = match new_state {
                    EngineState::Idle => "idle",
                    EngineState::Recording => "listening",
                    EngineState::Transcribing => "transcribing",
                    EngineState::Refining => "refining",
                    EngineState::Success => "success",
                };

                // Update tray icon dynamically based on active status
                if let Some(tray) = app_handle.tray_by_id("main-tray") {
                    let bytes = match new_state {
                        EngineState::Idle => include_bytes!("../icons/status_bar_icons/status_bar_32_dark.png") as &[u8],
                        EngineState::Recording => include_bytes!("../icons/status_bar_icons/status_bar_32_rec.png") as &[u8],
                        EngineState::Transcribing | EngineState::Refining | EngineState::Success => {
                            include_bytes!("../icons/status_bar_icons/status_bar_32_transcribing.png") as &[u8]
                        }
                    };
                    if let Ok(icon) = tauri::image::Image::from_bytes(bytes) {
                        let _ = tray.set_icon(Some(icon));
                    }
                }

                if let Some(window) = app_handle.get_webview_window("main") {
                    let js = format!("if (window.updateStatus) {{ window.updateStatus('{}'); }}", status_str);
                    let _ = window.eval(&js);
                }

                let _ = app_handle.emit("agent-status-change", serde_json::json!({
                    "agent_id": active_agent_id,
                    "status": status_str
                }));
            });

            // 7.5. Bind engine log changes to the Dashboard webview
            let app_handle_log = app.handle().clone();
            engine_toggle.set_log_callback(move |log_msg| {
                if let Some(window) = app_handle_log.get_webview_window("main") {
                    let safe_msg = log_msg.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "<br>");
                    let js = format!("if (window.addLog) {{ window.addLog('{}'); }}", safe_msg);
                    let _ = window.eval(&js);
                }
            });

            // 8. Bind paste/type trigger to execute simulated typing strictly on the main thread
            let app_handle_paste = app.handle().clone();
            engine_toggle.set_paste_callback(move |refined_text| {
               let app_h = app_handle_paste.clone();
                let _ = app_h.run_on_main_thread(move || {
                    // Remove the entire block with focusTestArea and window focus
                    // Delay to allow OS to return focus to target application
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    
                    if let Err(e) = os_integration::inject_text(&refined_text) {
                        eprintln!("❌ inject_text error: {:?}", e);
                    }
                });
            });

            // Spawn mouse hold-to-talk listener unconditionally on the main thread
            {
                let state: tauri::State<'_, AppState> = app.state();
                spawn_mouse_listener(
                    Arc::clone(&state.settings),
                    Arc::clone(&engine_toggle),
                    recorder_tx_toggle.clone(),
                );
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
            }
            RunEvent::Reopen { .. } => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        });
}




