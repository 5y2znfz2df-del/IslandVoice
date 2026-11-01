//! Offline smoke test for the Vosk runtime used by IslandVoice.
//!
//! Usage:
//!   cargo run --example vosk-check -- <libvosk.dll> <model-dir> <16k-mono.wav>

use std::path::Path;

use hound::WavReader;

fn main() {
    let mut args = std::env::args().skip(1);
    let lib = args.next().unwrap_or_else(|| "libvosk.dll".to_owned());
    let model = args
        .next()
        .unwrap_or_else(|| "models/vosk-model-small-en-us-0.15".to_owned());
    let wav = args.next().unwrap_or_else(|| "hey.wav".to_owned());
    if !Path::new(&wav).is_file() {
        eprintln!("missing wav file: {wav}");
        std::process::exit(2);
    }
    let mut reader = WavReader::open(&wav).expect("open wav");
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 || spec.bits_per_sample != 16 {
        eprintln!(
            "expected 16 kHz mono 16-bit PCM, got {} Hz / {} ch / {} bit",
            spec.sample_rate, spec.channels, spec.bits_per_sample
        );
        std::process::exit(2);
    }
    let pcm: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read samples");
    println!("samples: {}", pcm.len());
    match island_voice::debug_transcribe_vosk(&lib, &model, &pcm) {
        Ok(text) => println!("transcript: {text}"),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
    match island_voice::debug_wake_check(&lib, &model, "hey siri", &pcm) {
        Ok(text) if !text.is_empty() => println!("wake detected: {text}"),
        Ok(_) => println!("wake detected: NO"),
        Err(error) => {
            eprintln!("wake check error: {error}");
            std::process::exit(1);
        }
    }
}
