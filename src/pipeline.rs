//! Speech -> text -> DeepSeek -> island reply worker.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender};

use crate::Shared;
use crate::config::Config;
use crate::deepseek::{ChatMessage, DeepSeekClient, SYSTEM_PROMPT};
use crate::logging;
use crate::manager::ConfigCommand;
use crate::stt;
use crate::vosk::VoskRuntime;

#[derive(Clone, Debug)]
pub struct Utterance {
    pub pcm: Vec<i16>,
}

pub fn run(
    rx: Receiver<Utterance>,
    vosk: Option<Arc<VoskRuntime>>,
    config: Config,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    tts_tx: Sender<Option<String>>,
) {
    let mut history = vec![ChatMessage::system(SYSTEM_PROMPT)];
    while !stop.load(Ordering::Relaxed) {
        let utterance = match rx.recv_timeout(std::time::Duration::from_millis(150)) {
            Ok(utterance) => utterance,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if utterance.pcm.is_empty() {
            continue;
        }

        shared.set_status("识别中…");
        // The laptop mic array is often very quiet. Normalize the utterance
        // before recognition; this is the single biggest accuracy win.
        let peak = utterance
            .pcm
            .iter()
            .map(|sample| sample.unsigned_abs())
            .max()
            .unwrap_or(0);
        let gain = if peak > 50 {
            (24_000.0 / peak as f32).clamp(1.0, 12.0)
        } else {
            1.0
        };
        let mut pcm = utterance.pcm.clone();
        if gain > 1.01 {
            for sample in &mut pcm {
                *sample = (*sample as f32 * gain).clamp(-32_768.0, 32_767.0) as i16;
            }
        }
        logging::info(
            &shared.host,
            &format!("STT 电平：peak={peak} gain={gain:.1}"),
        );
        let text = match stt::transcribe(&config, &pcm, vosk.as_ref()) {
            Ok(text) => strip_wake_prefix(text.trim()),
            Err(error) => {
                logging::warn(&shared.host, &format!("语音识别失败：{error}"));
                shared.set_status("语音识别失败");
                shared
                    .notifier
                    .show_error(&format!("语音识别失败：{error}"));
                shared.send(ConfigCommand::Status("语音识别失败".to_owned()));
                continue;
            }
        };
        if text.is_empty() {
            shared.set_status("没有听清");
            shared
                .notifier
                .show_info("IslandVoice", "没有听清，请再说一次");
            continue;
        }
        logging::info(&shared.host, &format!("识别到：{text}"));
        shared.set_status(format!("识别到：{text}"));
        shared.send(ConfigCommand::Status(format!("识别到：{text}")));

        let current = shared.config();
        if current.api_key.trim().is_empty() {
            shared
                .notifier
                .show_error("请先在 IslandVoice 配置面板填写 DeepSeek API 密钥");
            shared.set_status("缺少 DeepSeek API 密钥");
            continue;
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }

        shared.notifier.show_thinking(&current);
        shared.set_status("思考中…");
        shared.send(ConfigCommand::Status("思考中…".to_owned()));
        history.push(ChatMessage::user(text.clone()));
        let client = match DeepSeekClient::new(&current) {
            Ok(client) => client,
            Err(error) => {
                shared.notifier.show_error(&error);
                shared.set_status("DeepSeek 配置错误");
                continue;
            }
        };
        match client.chat(&history) {
            Ok(reply) => {
                logging::info(&shared.host, &format!("AI 回复：{reply}"));
                if current.speak_reply {
                    let speak_ms = 1_500 + reply.chars().count() as u64 * 180;
                    shared.siri.show_for(speak_ms);
                    shared.mute_listening(std::time::Duration::from_millis(speak_ms));
                    let _ = tts_tx.try_send(Some(reply.clone()));
                }
                shared.notifier.show_reply(&current, &reply);
                shared.set_status("待命");
                shared.send(ConfigCommand::Status("待命".to_owned()));
                history.push(ChatMessage::assistant(reply));
                if history.len() > 9 {
                    history.drain(1..3);
                }
            }
            Err(error) => {
                logging::warn(&shared.host, &format!("DeepSeek 请求失败：{error}"));
                shared.set_status("DeepSeek 请求失败");
                shared.notifier.show_error(&format!("DeepSeek：{error}"));
                shared.send(ConfigCommand::Status("DeepSeek 请求失败".to_owned()));
                // Do not keep the unanswered user turn in the context.
                if history.last().map(|message| message.role.as_str()) == Some("user") {
                    history.pop();
                }
            }
        }
    }
}

fn strip_wake_prefix(text: &str) -> String {
    let mut value = text.trim();
    for prefix in ["hey siri", "hi siri", "siri", "嘿 siri", "嘿siri"] {
        let lower = value.to_ascii_lowercase();
        if lower.starts_with(prefix) {
            value = value[prefix.len()..].trim_start_matches([',', '，', '。', '!', '！', ' ']);
            break;
        }
    }
    value.trim().to_owned()
}
