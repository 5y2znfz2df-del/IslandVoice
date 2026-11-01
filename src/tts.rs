//! Neural Edge TTS with an offline SAPI fallback.
//!
//! Microsoft Edge's Read Aloud neural voices sound much better than the
//! installed SAPI desktop voices and need no API key. The worker synthesizes
//! MP3 to a temp file and plays it with MCI. If the network/Edge endpoint is
//! unavailable, it falls back to the local Windows SAPI voice.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
use std::process::{Command, Stdio};
use windows::Win32::Media::Speech::{ISpVoice, SPF_ASYNC, SPF_PURGEBEFORESPEAK, SpVoice};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::PCWSTR;

use crate::util::wide;

const EDGE_VOICE_DEFAULT: &str = "zh-CN-XiaoxiaoNeural";

pub struct TtsWorker {
    tx: Sender<Option<String>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TtsWorker {
    pub fn start(voice: String) -> Self {
        let (tx, rx) = bounded::<Option<String>>(4);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = thread::Builder::new()
            .name("island-voice-tts".to_owned())
            .spawn(move || run(rx, thread_stop, voice))
            .ok();
        Self { tx, stop, handle }
    }

    pub fn sender(&self) -> Sender<Option<String>> {
        self.tx.clone()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.try_send(None);
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for TtsWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn play_mp3(bytes: &[u8]) -> Result<(OutputStream, Sink), String> {
    let path = std::env::temp_dir().join("islandvoice_tts.mp3");
    std::fs::write(&path, bytes).map_err(|e| format!("write mp3: {e}"))?;
    let (stream, handle) = OutputStream::try_default().map_err(|e| format!("audio output: {e}"))?;
    let sink = Sink::try_new(&handle).map_err(|e| format!("audio sink: {e}"))?;
    let file = File::open(&path).map_err(|e| format!("open mp3: {e}"))?;
    let source = Decoder::new(BufReader::new(file)).map_err(|e| format!("decode mp3: {e}"))?;
    sink.append(source);
    Ok((stream, sink))
}

fn edge_synthesize(text: &str, voice: &str) -> Result<Vec<u8>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let path = std::env::temp_dir().join("islandvoice_edge_tts.mp3");
    let err_path = std::env::temp_dir().join("islandvoice_edge_tts.err");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&err_path);

    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();
    for base in [
        std::env::var("LOCALAPPDATA").ok(),
        std::env::var("USERPROFILE").ok(),
    ] {
        let Some(base) = base else { continue };
        for version in ["313", "312", "311", "314"] {
            let path = if base.ends_with("AppData\\Local") {
                format!("{base}\\Programs\\Python\\Python{version}\\python.exe")
            } else {
                format!("{base}\\AppData\\Local\\Programs\\Python\\Python{version}\\python.exe")
            };
            if std::path::Path::new(&path).is_file() {
                candidates.push((path, Vec::new()));
            }
        }
    }
    candidates.push(("py".to_owned(), vec!["-3.13".to_owned()]));
    candidates.push(("py".to_owned(), vec!["-3".to_owned()]));
    candidates.push(("python".to_owned(), Vec::new()));

    let mut last_error = String::new();
    for (python, prefix) in candidates {
        let stderr = std::fs::File::create(&err_path)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        let mut command = Command::new(&python);
        command
            .args(prefix)
            .args(["-m", "edge_tts", "--text", text, "--voice", voice])
            .arg("--write-media")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .creation_flags(CREATE_NO_WINDOW);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                last_error = format!("{python}: {error}");
                continue;
            }
        };
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() && path.is_file() {
                        return std::fs::read(&path).map_err(|error| format!("read mp3: {error}"));
                    }
                    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
                    last_error = format!(
                        "{python} edge-tts exited with {status}: {}",
                        crate::util::truncate_bytes(stderr.trim(), 1000)
                    );
                    break;
                }
                Ok(None) => {
                    if started.elapsed() > Duration::from_secs(15) {
                        let _ = child.kill();
                        let _ = child.wait();
                        last_error = format!("{python} edge-tts timed out");
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    last_error = format!("{python} edge-tts wait: {error}");
                    break;
                }
            }
        }
    }
    Err(last_error)
}

fn run(rx: Receiver<Option<String>>, stop: Arc<AtomicBool>, voice_name: String) {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let owns_com = hr.is_ok();
    let mut sapi: Option<ISpVoice> = None;
    let created: windows::core::Result<ISpVoice> =
        unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) };
    match created {
        Ok(voice) => {
            let _ = unsafe { voice.SetRate(0) };
            let _ = unsafe { voice.SetVolume(100) };
            sapi = Some(voice);
            crate::util::debug_line("TTS SAPI fallback ready");
        }
        Err(error) => {
            crate::util::debug_line(&format!("TTS SAPI init failed: {error}"));
        }
    }

    let voice_name = if voice_name.trim().is_empty() {
        EDGE_VOICE_DEFAULT.to_owned()
    } else {
        voice_name
    };

    let mut playback: Option<(OutputStream, Sink)> = None;

    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(Some(text)) => {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let mut played = false;
                playback = None;
                match edge_synthesize(text, &voice_name) {
                    Ok(audio) if !audio.is_empty() => {
                        if let Some(voice) = sapi.as_ref() {
                            let _ = unsafe {
                                voice.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None)
                            };
                        }
                        match play_mp3(&audio) {
                            Ok(stream) => {
                                playback = Some(stream);
                                crate::util::debug_line(&format!(
                                    "Edge TTS queued: {} chars, {} bytes mp3",
                                    text.chars().count(),
                                    audio.len()
                                ));
                                played = true;
                            }
                            Err(error) => {
                                crate::util::debug_line(&format!(
                                    "Edge TTS mp3 playback failed: {error}"
                                ));
                            }
                        }
                    }
                    Ok(_) => {
                        crate::util::debug_line("Edge TTS returned empty audio");
                    }
                    Err(error) => {
                        crate::util::debug_line(&format!("Edge TTS failed: {error}"));
                    }
                }

                if !played {
                    playback = None;
                    if let Some(voice) = sapi.as_ref() {
                        let wide = wide(text);
                        let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
                        // SAFETY: the wide buffer is NUL terminated and stays
                        // alive for the asynchronous speak call.
                        if let Err(error) =
                            unsafe { voice.Speak(PCWSTR(wide.as_ptr()), flags, None) }
                        {
                            crate::util::debug_line(&format!("SAPI speak failed: {error}"));
                        } else {
                            crate::util::debug_line(&format!(
                                "SAPI fallback queued: {} chars",
                                text.chars().count()
                            ));
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(playback);
    if let Some(voice) = sapi.as_ref() {
        let _ = unsafe { voice.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None) };
    }
    if owns_com {
        unsafe { CoUninitialize() };
    }
}
