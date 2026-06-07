#[allow(deprecated)]
use std::io::{self, Read};
use std::path::Path;
use anyhow::{Context, Result, bail};
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    context::params::LlamaContextParams,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
    llama_batch::LlamaBatch,
};

fn main() -> Result<()> {
    // 1. Get model path from argv[1] (required)
    let args: Vec<String> = std::env::args().collect();
    let model_path_str = if args.len() > 1 {
        &args[1]
    } else {
        "./models/gemma-2-2b-it-Q6_K.gguf"
    };
    let model_path = Path::new(model_path_str);

    // 2. Get dynamic system prompt from argv[2] (optional — falls back to sensible default)
    let system_prompt = if args.len() > 2 {
        args[2].clone()
    } else {
        "You are a high-performance offline text refinement utility.\n\
         Your job is to:\n\
         1. Fix spelling, grammar, and punctuation mistakes in the transcript.\n\
         2. Maintain the original language (translate to clear English only if explicitly requested).\n\
         3. If technical terms or code fragments are present, apply proper formatting.\n\
         4. Strictly output ONLY the corrected/translated text. Do NOT add any introductions, \
            explanations, thoughts, notes, or markdown wrappers."
            .to_string()
    };

    if !model_path.exists() {
        bail!("Model file not found at: {}", model_path.display());
    }

    // 3. Read raw transcript from stdin
    let mut raw_transcript = String::new();
    io::stdin().read_to_string(&mut raw_transcript)
        .context("Failed to read raw transcript from stdin")?;
    let raw_transcript = raw_transcript.trim();

    if raw_transcript.is_empty() {
        return Ok(());
    }

    // Sanitize transcript to prevent prompt/turn injection in Gemma 2
    let raw_transcript = raw_transcript
        .replace("<start_of_turn>", "")
        .replace("<end_of_turn>", "");

    // 3. Initialize backend and load model with GPU (Metal) offloading
    let backend = LlamaBackend::init().context("Failed to initialize LlamaBackend")?;
    let model_params = LlamaModelParams::default()
        .with_n_gpu_layers(99); // Offload all layers to GPU
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
        .context("Failed to load Gemma 2 model file")?;

    // 4. Create a lightweight inference context (2048 tokens limit)
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(std::num::NonZeroU32::new(2048));
    let mut ctx = model.new_context(&backend, ctx_params)
        .context("Failed to create inference context")?;

    // 5. Format prompt: system prompt from argv[2] + Gemma 2 turn template + forced prefill
    let prompt = format!(
        "<start_of_turn>user\n\
         {}\n\n\
         Here is the raw transcript to refine:\n\
         \"{}\"\n\
         <end_of_turn>\n\
         <start_of_turn>model\n",
        system_prompt,
        raw_transcript
    );

    // 6. Tokenize prompt
    let tokens = model.str_to_token(&prompt, AddBos::Always)
        .context("Failed to tokenize prompt")?;

    if tokens.is_empty() {
        bail!("Tokenized prompt is empty");
    }

    // 7. Decode prompt
    let mut batch = LlamaBatch::new(tokens.len(), 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch.add(token, i as i32, &[0], is_last)?;
    }
    ctx.decode(&mut batch).context("Failed to decode prompt batch")?;

    // 8. Autoregressive token generation loop
    let mut generated_tokens = Vec::new();
    let mut current_pos = tokens.len() as i32;

    // Use a strict greedy sampler to avoid chat filler
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::greedy()
    ]);

    for _ in 0..512 {
        let next_token = sampler.sample(&ctx, batch.n_tokens() - 1);

        if next_token == model.token_eos() {
            break;
        }

        generated_tokens.push(next_token);

        batch.clear();
        batch.add(next_token, current_pos, &[0], true)?;
        ctx.decode(&mut batch).context("Failed to decode generated token")?;
        current_pos += 1;
    }

    // 9. Convert generated token IDs back to UTF-8 text piece-by-piece
    let mut refined_text = String::new();
    #[allow(deprecated)]
    for token in generated_tokens {
        if let Ok(piece) = model.token_to_str(token, llama_cpp_2::model::Special::Plaintext) {
            refined_text.push_str(&piece);
        }
    }

    print!("{}", refined_text.trim());
    Ok(())
}
