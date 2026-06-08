use std::io::{self, Read};
use std::process::Command;
use std::path::PathBuf;
use anyhow::{Context, Result, anyhow, bail};

/// Finds the `litert-lm` binary path by checking the system's `PATH` env variable,
/// with a fallback to the macOS Python user-space path if `HOME` is set.
fn find_litert_lm() -> Result<PathBuf> {
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("litert-lm");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let fallback = PathBuf::from(home)
            .join("Library/Python/3.14/bin/litert-lm");
        if fallback.is_file() {
            return Ok(fallback);
        }
    }

    Err(anyhow!(
        "litert-lm binary not found in PATH or at ~/Library/Python/3.14/bin/litert-lm"
    ))
}

fn main() -> Result<()> {
    // 2. Read the raw transcription string from stdin. Do not manually escape quotes or backslashes.
    let mut raw_transcript = String::new();
    io::stdin()
        .read_to_string(&mut raw_transcript)
        .context("Failed to read raw transcript from stdin")?;
    let raw_transcript = raw_transcript.trim();

    if raw_transcript.is_empty() {
        return Ok(());
    }

    // 3. Construct the exact English prompt payload using a raw string literal to maintain formatting
    let prompt = format!(
        r#"You are a strict text editing assistant. Your only task is to correct grammatical, spelling, and punctuation errors in the provided Ukrainian text. 
Follow these constraints strictly:
1. DO NOT add any extra commentary, explanations, or introductory phrases.
2. DO NOT change the style, tone, or sentence structure unless it contains a grammatical error.
3. DO NOT insert random spaces, special characters, or symbols.
4. Output ONLY the final corrected text.

Input text: "{}"
Corrected output:"#,
        raw_transcript
    );

    // 4. Implement dynamic execution of the litert-lm binary using std::process::Command
    let litert_lm_path = find_litert_lm()?;

    // Parse model path from argv[1] (passed by the engine) with fallback to default assets path
    let args: Vec<String> = std::env::args().collect();
    let model_path = if args.len() > 1 {
        &args[1]
    } else {
        "models/gemma-4-E2B-it.litertlm"
    };

    // 5. Pass arguments safely via the OS execve array using separate .arg() calls
    let output = Command::new(litert_lm_path)
        .arg("run")
        .arg(model_path)
        .arg(format!("--prompt={}", prompt))
        .output()
        .context("Failed to execute litert-lm command")?;

    // 6. If output.status.success() is true, print the exact trimmed stdout to the sidecar's stdout.
    // If it fails, capture stderr and return a structured error.
    if output.status.success() {
        let refined_text = String::from_utf8_lossy(&output.stdout);
        print!("{}", refined_text.trim());
        Ok(())
    } else {
        let stderr_err = String::from_utf8_lossy(&output.stderr);
        bail!(
            "litert-lm execution failed with exit code: {:?}. Error: {}",
            output.status.code(),
            stderr_err.trim()
        );
    }
}
