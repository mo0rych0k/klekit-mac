use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use crossbeam_channel::unbounded;

pub struct AudioRecorder {
    stream: Option<Stream>,
    rx: Option<crossbeam_channel::Receiver<f32>>,
    sample_rate: u32,
    channels: u16,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            stream: None,
            rx: None,
            sample_rate: 16000,
            channels: 1,
        }
    }

    /// Starts capturing audio from the default input device.
    pub fn start_recording(&mut self) -> Result<()> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .context("No default input audio device found")?;
        
        let supported_config = device.default_input_config()
            .context("Failed to get default input audio configuration")?;

        let sample_format = supported_config.sample_format();
        let config = supported_config.config();
        
        self.sample_rate = config.sample_rate.0;
        self.channels = config.channels;

        let (tx, rx) = unbounded::<f32>();

        let err_fn = |err| eprintln!("An error occurred on the input stream: {}", err);

        // Build stream depending on sample format
        let stream = match sample_format {
            SampleFormat::F32 => {
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        for &sample in data {
                            let _ = tx.send(sample);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::I16 => {
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        for &sample in data {
                            let f32_sample = sample as f32 / 32768.0;
                            let _ = tx.send(f32_sample);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        for &sample in data {
                            let f32_sample = (sample as f32 - 32768.0) / 32768.0;
                            let _ = tx.send(f32_sample);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            _ => bail!("Unsupported sample format: {:?}", sample_format),
        }.context("Failed to build input stream")?;

        stream.play().context("Failed to start audio stream")?;

        self.stream = Some(stream);
        self.rx = Some(rx);

        Ok(())
    }

    /// Stops capturing, processes the buffer (downmixes stereo, resamples to 16kHz Mono f32),
    /// and returns the raw f32 PCM buffer ready for Whisper.
    pub fn stop_recording(&mut self) -> Result<Vec<f32>> {
        // Drop the stream to stop recording
        self.stream = None;

        let rx = self.rx.take().context("No active recording session")?;
        
        // Read all captured f32 samples from the channel
        let raw_samples: Vec<f32> = rx.try_iter().collect();

        if raw_samples.is_empty() {
            bail!("No audio samples captured");
        }

        // 1. Stereo to Mono Downmixing (average channels)
        let mono_samples = if self.channels > 1 {
            let channels_count = self.channels as usize;
            let mut mono = Vec::with_capacity(raw_samples.len() / channels_count);
            let mut chunk_sum = 0.0;
            let mut count = 0;

            for sample in raw_samples {
                chunk_sum += sample;
                count += 1;
                if count == channels_count {
                    mono.push(chunk_sum / channels_count as f32);
                    chunk_sum = 0.0;
                    count = 0;
                }
            }
            mono
        } else {
            raw_samples
        };

        // 2. High-Performance Linear Resampling to exactly 16kHz
        let resampled = resample_linear(&mono_samples, self.sample_rate, 16000);

        Ok(resampled)
    }
}

/// Linear audio resampler from arbitrary rate down to 16kHz Mono f32 PCM.
pub fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz {
        return input.to_vec();
    }
    let ratio = from_hz as f64 / to_hz as f64;
    let output_len = (input.len() as f64 / ratio).floor() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let pos = i as f64 * ratio;
        let low = pos.floor() as usize;
        let high = pos.ceil() as usize;
        if high >= input.len() {
            break;
        }
        let weight = pos - low as f64;
        let sample = (1.0 - weight) as f32 * input[low] + weight as f32 * input[high];
        output.push(sample);
    }
    output
}
