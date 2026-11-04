//! Microphone capture, endpointing and the offline wake-word state machine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender, bounded};

use crate::Shared;
use crate::config::Config;
use crate::manager::ConfigCommand;
use crate::pipeline;
use crate::util::{play_cue, wake_matches};
use crate::vosk::{VoskRecognizer, VoskRuntime};

const TARGET_RATE: u32 = 16_000;
const FRAME_SAMPLES: usize = 320; // 20 ms at 16 kHz
const WAKE_FEED_SAMPLES: usize = 1_600; // 100 ms
const PRE_ROLL_SAMPLES: usize = 3_200; // 200 ms before wake
const MAX_RECORD_SECONDS: u32 = 60;
const END_SILENCE_MS: u64 = 5_000;
const MIN_START_SPEECH_MS: u64 = 180;
const MAX_WAIT_SPEECH_MS: u64 = 15_000;

#[derive(Clone, Copy, Debug)]
struct AudioFormat {
    sample_rate: u32,
}

pub struct Engine {
    stop: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    tts: Option<crate::tts::TtsWorker>,
    tts_tx: Sender<Option<String>>,
}

impl Engine {
    pub fn start(shared: Arc<Shared>, config: Config) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let manual = Arc::new(AtomicBool::new(false));
        let (frame_tx, frame_rx) = bounded::<Vec<f32>>(64);
        let (utt_tx, utt_rx) = bounded::<pipeline::Utterance>(8);
        let (format_tx, format_rx) = bounded::<AudioFormat>(1);
        let (error_tx, error_rx) = bounded::<String>(8);

        let wake_vosk = match VoskRuntime::load(&config) {
            Ok(runtime) => {
                crate::logging::warn(
                    &shared.host,
                    &format!(
                        "Vosk 已加载：{} / model={} / wake={}",
                        runtime.description(),
                        runtime.model_dir.display(),
                        config.wake_word
                    ),
                );
                shared.set_engine_status(runtime.description());
                Some(runtime)
            }
            Err(error) => {
                crate::logging::warn(&shared.host, &format!("Vosk 加载失败：{error}"));
                shared.set_engine_status(format!("不可用：{error}"));
                shared.set_status("未加载唤醒引擎（可手动触发）");
                None
            }
        };

        let audio_stop = stop.clone();
        let audio_shared = shared.clone();
        let audio_thread = thread::Builder::new()
            .name("island-voice-audio".to_owned())
            .spawn(move || {
                run_audio(audio_stop, frame_tx, format_tx, error_tx, audio_shared);
            })
            .map_err(|error| format!("创建音频线程失败：{error}"))?;

        let process_stop = stop.clone();
        let process_manual = manual.clone();
        let process_shared = shared.clone();
        let process_config = config.clone();
        let process_vosk = wake_vosk.clone();
        let processor_thread = thread::Builder::new()
            .name("island-voice-processor".to_owned())
            .spawn(move || {
                let panic_host = process_shared.host.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    run_processor(
                        process_stop,
                        process_manual,
                        frame_rx,
                        format_rx,
                        error_rx,
                        utt_tx,
                        process_vosk,
                        process_config,
                        process_shared,
                    );
                }));
                if result.is_err() {
                    crate::logging::warn(&panic_host, "音频处理线程 panic");
                }
            })
            .map_err(|error| format!("创建语音处理线程失败：{error}"))?;

        let stt_vosk = match VoskRuntime::load_stt(&config) {
            Ok(runtime) => {
                crate::logging::warn(
                    &shared.host,
                    &format!("中文/STT 模型：{}", runtime.description()),
                );
                Some(runtime)
            }
            Err(error) => {
                crate::logging::warn(&shared.host, &format!("STT 模型加载失败：{error}"));
                wake_vosk.clone()
            }
        };

        let tts = crate::tts::TtsWorker::start(config.tts_voice.clone());
        let tts_tx = tts.sender();
        let engine_tts_tx = tts_tx.clone();

        let pipeline_stop = stop.clone();
        let pipeline_shared = shared.clone();
        let pipeline_config = config.clone();
        let pipeline_thread = thread::Builder::new()
            .name("island-voice-pipeline".to_owned())
            .spawn(move || {
                pipeline::run(
                    utt_rx,
                    stt_vosk,
                    pipeline_config,
                    pipeline_shared,
                    pipeline_stop,
                    tts_tx,
                );
            })
            .map_err(|error| format!("创建 AI 管道线程失败：{error}"))?;

        let initial_status = if config.wake_enabled() {
            "监听中，等待唤醒词"
        } else {
            "监听中（未设置唤醒词，请手动触发）"
        };
        shared.set_status(initial_status);
        shared.send(ConfigCommand::Status(initial_status.to_owned()));

        Ok(Self {
            stop,
            manual,
            handles: vec![audio_thread, processor_thread, pipeline_thread],
            tts: Some(tts),
            tts_tx: engine_tts_tx,
        })
    }

    pub fn speak(&self, text: &str) {
        let _ = self.tts_tx.try_send(Some(text.to_owned()));
    }

    pub fn trigger_manual(&self) {
        self.manual.store(true, Ordering::SeqCst);
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
        if let Some(mut tts) = self.tts.take() {
            tts.stop();
        }
    }
}

fn run_audio(
    stop: Arc<AtomicBool>,
    frame_tx: Sender<Vec<f32>>,
    format_tx: Sender<AudioFormat>,
    error_tx: Sender<String>,
    shared: Arc<Shared>,
) {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        let _ = error_tx.try_send("未找到麦克风输入设备".to_owned());
        return;
    };
    let device_name = device.name().unwrap_or_else(|_| "unknown".to_owned());
    let supported = match device.default_input_config() {
        Ok(config) => config,
        Err(error) => {
            let _ = error_tx.try_send(format!("读取麦克风默认格式失败：{error}"));
            return;
        }
    };
    let sample_format = supported.sample_format();
    let stream_config: StreamConfig = supported.config();
    crate::logging::warn(
        &shared.host,
        &format!(
            "麦克风：{device_name} / {:?} / {} Hz / {} ch",
            sample_format, stream_config.sample_rate.0, stream_config.channels
        ),
    );
    let format = AudioFormat {
        sample_rate: stream_config.sample_rate.0,
    };
    if format_tx.send(format).is_err() {
        return;
    }
    let stream = build_stream(
        &device,
        &stream_config,
        sample_format,
        frame_tx,
        error_tx.clone(),
        stream_config.channels,
    );
    let stream: Stream = match stream {
        Ok(stream) => stream,
        Err(error) => {
            let _ = error_tx.try_send(format!("创建麦克风音频流失败：{error}"));
            return;
        }
    };
    if let Err(error) = stream.play() {
        let _ = error_tx.try_send(format!("启动麦克风失败：{error}"));
        return;
    }
    shared.set_status("监听中，等待唤醒词");
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(40));
    }
    drop(stream);
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    tx: Sender<Vec<f32>>,
    error_tx: Sender<String>,
    channels: u16,
) -> Result<Stream, cpal::BuildStreamError> {
    let channels = channels.max(1) as usize;
    match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| *value));
            },
            on_error(error_tx.clone()),
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| *value as f32 / 32_768.0));
            },
            on_error(error_tx.clone()),
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| {
                    (*value as f32 - 32_768.0) / 32_768.0
                }));
            },
            on_error(error_tx.clone()),
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            config,
            move |data: &[i32], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| {
                    *value as f32 / 2_147_483_648.0
                }));
            },
            on_error(error_tx.clone()),
            None,
        ),
        SampleFormat::F64 => device.build_input_stream(
            config,
            move |data: &[f64], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| *value as f32));
            },
            on_error(error_tx.clone()),
            None,
        ),
        SampleFormat::I8 => device.build_input_stream(
            config,
            move |data: &[i8], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| *value as f32 / 128.0));
            },
            on_error(error_tx.clone()),
            None,
        ),
        SampleFormat::U8 => device.build_input_stream(
            config,
            move |data: &[u8], _: &cpal::InputCallbackInfo| {
                let _ = tx.try_send(downmix(data, channels, |value| {
                    (*value as f32 - 128.0) / 128.0
                }));
            },
            on_error(error_tx.clone()),
            None,
        ),
        other => {
            let _ = error_tx.try_send(format!("不支持的麦克风采样格式：{other}"));
            // Return the host's build error type without opening a stream.
            Err(cpal::BuildStreamError::StreamConfigNotSupported)
        }
    }
}

fn on_error(error_tx: Sender<String>) -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |error| {
        let _ = error_tx.try_send(format!("麦克风流错误：{error}"));
    }
}

fn downmix<T: Copy>(data: &[T], channels: usize, convert: impl Fn(&T) -> f32) -> Vec<f32> {
    if channels <= 1 {
        return data.iter().map(&convert).collect();
    }
    data.chunks(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().map(&convert).sum();
            sum / frame.len() as f32
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn run_processor(
    stop: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    frame_rx: Receiver<Vec<f32>>,
    format_rx: Receiver<AudioFormat>,
    error_rx: Receiver<String>,
    utt_tx: Sender<pipeline::Utterance>,
    vosk: Option<Arc<VoskRuntime>>,
    config: Config,
    shared: Arc<Shared>,
) {
    crate::logging::warn(&shared.host, "音频处理线程已进入");
    let format = match format_rx.recv_timeout(Duration::from_secs(8)) {
        Ok(format) => format,
        Err(_) => {
            crate::logging::warn(&shared.host, "音频处理线程等待音频格式超时");
            report_errors(&error_rx, &shared);
            return;
        }
    };
    crate::logging::warn(
        &shared.host,
        &format!("音频处理线程收到格式：{} Hz", format.sample_rate),
    );
    let mut resampler = LinearResampler::new(format.sample_rate, TARGET_RATE);
    let mut wake_recognizer = if config.wake_enabled() {
        match vosk.as_ref().map(|runtime| runtime.wake_recognizer()) {
            Some(Ok(recognizer)) => Some(recognizer),
            Some(Err(error)) => {
                crate::logging::warn(&shared.host, &format!("创建 Vosk 唤醒识别器失败：{error}"));
                shared.send(ConfigCommand::Status(format!(
                    "唤醒引擎初始化失败：{error}"
                )));
                None
            }
            None => None,
        }
    } else {
        None
    };
    crate::logging::warn(
        &shared.host,
        &format!("Vosk 唤醒识别器就绪：{}", wake_recognizer.is_some()),
    );
    let mut wake_feed: Vec<i16> = Vec::with_capacity(WAKE_FEED_SAMPLES * 2);
    let mut pcm_buffer: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 4);
    let mut recording: Option<Recording> = None;
    let mut cooldown_until = Instant::now();
    let mut frames_seen = 0u64;
    let mut recent_pcm: Vec<f32> = Vec::with_capacity(PRE_ROLL_SAMPLES);
    let mut last_partial = String::new();
    let mut wake_hits = 0u8;
    let mut level_peak = 0.0f32;
    let mut level_at = Instant::now();
    crate::logging::warn(
        &shared.host,
        &format!(
            "音频处理线程启动：wake_ready={} 源采样率={}",
            wake_recognizer.is_some(),
            format.sample_rate
        ),
    );

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        report_errors(&error_rx, &shared);
        if shared.is_listening_muted() {
            while frame_rx.try_recv().is_ok() {}
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        if manual.swap(false, Ordering::SeqCst) && recording.is_none() {
            recording = Some(Recording::new_with_pre_roll(std::mem::take(
                &mut recent_pcm,
            )));
            wake_feed.clear();
            shared.siri.show();
            play_cue(true);
            shared.set_status("请说话…");
            shared.send(ConfigCommand::Status("请说话…".to_owned()));
            continue;
        }
        let input = match frame_rx.recv_timeout(Duration::from_millis(40)) {
            Ok(input) => input,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        frames_seen += 1;
        if frames_seen == 1 {
            crate::logging::warn(&shared.host, "收到第一帧麦克风音频");
        }
        let pcm = resampler.process(&input);
        if pcm.is_empty() {
            continue;
        }
        pcm_buffer.extend_from_slice(&pcm);
        let mut offset = 0usize;
        while offset + FRAME_SAMPLES <= pcm_buffer.len() {
            let frame = &pcm_buffer[offset..offset + FRAME_SAMPLES];
            offset += FRAME_SAMPLES;
            level_peak = frame
                .iter()
                .fold(level_peak, |peak, sample| peak.max(sample.abs()));
            if recording.is_none() {
                recent_pcm.extend_from_slice(frame);
                if recent_pcm.len() > PRE_ROLL_SAMPLES {
                    let drop = recent_pcm.len() - PRE_ROLL_SAMPLES;
                    recent_pcm.drain(0..drop);
                }
            }
            if let Some(active) = recording.as_mut() {
                match active.push(frame) {
                    RecordingResult::Continue => {}
                    RecordingResult::Finished => {
                        crate::util::debug_line("recording finished");
                        shared.siri.hide();
                        let finished = recording.take().unwrap();
                        let pcm: Vec<i16> = finished
                            .pcm
                            .iter()
                            .map(|sample| (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
                            .collect();
                        if finished.speech_started && pcm.len() >= 1_600 {
                            let _ = utt_tx.try_send(pipeline::Utterance { pcm });
                            shared.set_status("识别中…");
                            shared.send(ConfigCommand::Status("识别中…".to_owned()));
                            play_cue(true);
                        } else {
                            shared.set_status("没有听到有效语音");
                            shared.send(ConfigCommand::Status("没有听到有效语音".to_owned()));
                        }
                        cooldown_until = Instant::now() + Duration::from_millis(3_000);
                        wake_feed.clear();
                        wake_recognizer = recreate_wake(vosk.as_ref(), &config);
                    }
                    RecordingResult::Cancelled => {
                        crate::util::debug_line("recording cancelled: no speech");
                        recording = None;
                        shared.siri.hide();
                        shared.set_status("没有检测到语音，已取消");
                        shared.send(ConfigCommand::Status("没有检测到语音，已取消".to_owned()));
                        cooldown_until = Instant::now() + Duration::from_millis(2_000);
                        wake_feed.clear();
                    }
                }
                continue;
            }

            // Idle: feed the wake recognizer and look for the wake phrase.
            let Some(recognizer) = wake_recognizer.as_mut() else {
                continue;
            };
            if Instant::now() < cooldown_until {
                continue;
            }
            wake_feed.extend(frame.iter().map(|sample| {
                (sample.clamp(-1.0, 1.0) * 4.0)
                    .clamp(-1.0, 1.0)
                    .mul_add(32_767.0, 0.0)
                    .round() as i16
            }));
            if wake_feed.len() < WAKE_FEED_SAMPLES {
                continue;
            }
            let ended = recognizer.accept(&wake_feed);
            let text = if ended {
                recognizer.result_text()
            } else {
                recognizer.partial_text()
            };
            wake_feed.clear();
            if !text.is_empty() && text != last_partial {
                crate::util::debug_line(&format!("partial: {text}"));
                last_partial = text.clone();
            }
            if !text.is_empty() && wake_matches(&text, &config.wake_word) {
                wake_hits = wake_hits.saturating_add(1);
            } else {
                wake_hits = 0;
            }
            if wake_hits >= 2 {
                wake_hits = 0;
                crate::util::debug_line(&format!("wake: {text}"));
                recording = Some(Recording::new_with_pre_roll(std::mem::take(
                    &mut recent_pcm,
                )));
                wake_feed.clear();
                wake_recognizer = recreate_wake(vosk.as_ref(), &config);
                shared.siri.show();
                play_cue(true);
                shared.set_status("已唤醒，请说话…");
                shared.send(ConfigCommand::Status(format!("已唤醒（识别到：{text}）")));
            }
        }
        if offset > 0 {
            pcm_buffer.drain(0..offset);
        }
        if level_at.elapsed() >= Duration::from_secs(2) {
            crate::util::debug_line(&format!("peak: {level_peak:.4}"));
            level_peak = 0.0;
            level_at = Instant::now();
        }
    }
}

fn report_errors(error_rx: &Receiver<String>, shared: &Shared) {
    while let Ok(error) = error_rx.try_recv() {
        shared.set_error(error);
        shared.send(ConfigCommand::Status("音频错误".to_owned()));
    }
}

fn recreate_wake(vosk: Option<&Arc<VoskRuntime>>, config: &Config) -> Option<VoskRecognizer> {
    if !config.wake_enabled() {
        return None;
    }
    vosk.and_then(|runtime| runtime.wake_recognizer().ok())
}

struct Recording {
    started: Instant,
    last_voice: Instant,
    speech_started: bool,
    pcm: Vec<f32>,
    vad: AdaptiveVad,
    speech_frames: u32,
}

enum RecordingResult {
    Continue,
    Finished,
    Cancelled,
}

impl Recording {
    fn new_with_pre_roll(pre_roll: Vec<f32>) -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last_voice: now,
            speech_started: false,
            pcm: pre_roll,
            vad: AdaptiveVad::new(),
            speech_frames: 0,
        }
    }

    fn push(&mut self, frame: &[f32]) -> RecordingResult {
        self.pcm.extend_from_slice(frame);
        let rms = rms(frame);
        let now = Instant::now();
        if self.vad.is_speech(rms) {
            self.speech_frames += 1;
            if self.speech_frames as u64 * 20 >= MIN_START_SPEECH_MS {
                self.speech_started = true;
            }
            self.last_voice = now;
        }
        if self.speech_started {
            let silence_ms = now.duration_since(self.last_voice).as_millis() as u64;
            if silence_ms >= END_SILENCE_MS
                || self.pcm.len() >= TARGET_RATE as usize * MAX_RECORD_SECONDS as usize
            {
                return RecordingResult::Finished;
            }
        } else if now.duration_since(self.started).as_millis() as u64 > MAX_WAIT_SPEECH_MS {
            return RecordingResult::Cancelled;
        }
        RecordingResult::Continue
    }
}

fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f32 = frame.iter().map(|sample| sample * sample).sum();
    (sum / frame.len() as f32).sqrt()
}

struct AdaptiveVad {
    noise: f32,
    speech_run: u32,
    silence_run: u32,
}

impl AdaptiveVad {
    fn new() -> Self {
        Self {
            noise: 0.0030,
            speech_run: 0,
            silence_run: 0,
        }
    }

    fn is_speech(&mut self, rms: f32) -> bool {
        let threshold = (self.noise * 5.0).clamp(0.0120, 0.035);
        if rms > threshold {
            self.speech_run = self.speech_run.saturating_add(1);
            self.silence_run = 0;
            self.speech_run >= 2
        } else {
            self.speech_run = 0;
            self.silence_run = self.silence_run.saturating_add(1);
            self.noise = (self.noise * 0.995 + rms * 0.005).clamp(0.0005, 0.02);
            false
        }
    }
}

struct LinearResampler {
    step: f64,
    position: f64,
    buffer: Vec<f32>,
}

impl LinearResampler {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            step: input_rate as f64 / output_rate as f64,
            position: 0.0,
            buffer: Vec::new(),
        }
    }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.buffer.extend_from_slice(input);
        let mut output = Vec::with_capacity((self.buffer.len() as f64 / self.step) as usize + 2);
        while (self.position as usize) + 1 < self.buffer.len() {
            let index = self.position as usize;
            let fraction = self.position - index as f64;
            let sample = self.buffer[index] as f64 * (1.0 - fraction)
                + self.buffer[index + 1] as f64 * fraction;
            output.push(sample as f32);
            self.position += self.step;
        }
        let consumed = self.position as usize;
        if consumed > 0 {
            self.buffer.drain(0..consumed);
            self.position -= consumed as f64;
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_reduces_sample_count() {
        let mut resampler = LinearResampler::new(48_000, 16_000);
        let input = vec![0.0f32; 4_800];
        let output = resampler.process(&input);
        assert!((output.len() as i32 - 1_600).abs() <= 2);
    }

    #[test]
    fn vad_starts_on_loud_frames() {
        let mut vad = AdaptiveVad::new();
        assert!(!vad.is_speech(0.001));
        assert!(!vad.is_speech(0.2));
        assert!(vad.is_speech(0.2));
    }
}
