use anyhow::Result;
use klekit::{audio, AppSettings, AppState, RecorderCommand, VoiceAssistantEngine};
use std::sync::{Arc, Mutex};

/// CLI / debug entry-point (macOS only).
///
/// Since the shipping product is the Tauri tray app, this binary provides a
/// minimal interactive loop for quick smoke-tests without launching the full UI.
/// Global hotkey registration (via rdev) has been removed — the Tauri app handles
/// all OS-level key/mouse listeners.  Press <Enter> here to toggle recording.
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let is_cli = args.iter().any(|arg| arg == "--cli");

    if !is_cli {
        println!("💡 Tip: Run 'cargo run --bin app' to launch the graphical app in the system tray (menu bar)!");
        println!("🚀 Launching interactive CLI mode by default...\n");
    }

    // Initialize our voice assistant engine with model paths
    let cli_settings = Arc::new(Mutex::new(Arc::new(AppSettings::load())));
    let engine = Arc::new(VoiceAssistantEngine::new(
        "./models/ggml-large-v3-turbo-q5_0.bin",
        "./models/gemma-4-E2B-it.litertlm",
        "./bin/llm_refiner",
        cli_settings,
    ));

    // Start inactivity weight unloading timer (Zero-Memory Idle)
    VoiceAssistantEngine::start_inactivity_timer(Arc::clone(&engine));

    // Create control channels for local audio recorder on dedicated OS thread
    let (recorder_tx, recorder_rx) = crossbeam_channel::unbounded::<RecorderCommand>();

    // Start dedicated local thread for AudioRecorder (since cpal::Stream is !Send and !Sync)
    std::thread::spawn(move || {
        let mut recorder = audio::AudioRecorder::new();
        while let Ok(cmd) = recorder_rx.recv() {
            match cmd {
                RecorderCommand::Start => {
                    if let Err(e) = recorder.start_recording() {
                        eprintln!("❌ Failed to start audio recording: {:?}", e);
                    }
                }
                RecorderCommand::Stop(res_tx) => {
                    let res = recorder.stop_recording();
                    let _ = res_tx.send(res);
                }
            }
        }
    });

    // stdin-based toggle loop (developer / smoke-test mode).
    // Press <Enter> to start recording; press <Enter> again to stop and process.
    // The global hotkey in the production app is handled by tauri-plugin-global-shortcut.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    std::thread::spawn(move || {
        println!("⌨️  CLI mode: press <Enter> to toggle recording. Ctrl-C to exit.");
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let _ = tx.send(());
                }
                Err(e) => {
                    eprintln!("❌ Failed to read stdin: {:?}", e);
                    break;
                }
            }
        }
    });

    println!("\n🚀 App successfully launched!");
    println!("🎙️  Press <Enter> to START / STOP recording voice.");

    // Process keypress signals through our engine
    while let Some(()) = rx.recv().await {
        let state = engine.get_state();
        let engine_clone = Arc::clone(&engine);
        let recorder_tx_clone = recorder_tx.clone();
        let active_id = "slack_refiner".to_string();
        let res = if state == AppState::Recording {
            VoiceAssistantEngine::stop_and_process_for_agent(engine_clone, recorder_tx_clone, active_id)
        } else if state == AppState::Idle {
            VoiceAssistantEngine::start_recording_for_agent(engine_clone, recorder_tx_clone, active_id)
        } else {
            println!("⏳ Please wait... processing previous recording");
            continue;
        };
        if let Err(e) = res {
            eprintln!("❌ Failed to toggle recording state: {:?}", e);
        }
    }

    Ok(())
}