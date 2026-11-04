//! Speech -> text -> large language model -> island reply worker.
//!
//! 大模型不再强制绑定 DeepSeek：通过配置的 base_url / api_key / model
//! 调用任意 OpenAI 兼容接口。如果开启了电脑操作，AI 回复中的 `[ACTION: ...]`
//! 标记会被解析并执行本地操作（整理桌面、查天气等）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender};

use crate::Shared;
use crate::config::Config;
use crate::computer::{self, ComputerAction};
use crate::deepseek::{ChatMessage, ChatClient};
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
    let mut history = vec![ChatMessage::system(config.system_prompt.clone())];
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
                .show_error("请先在 IslandVoice 配置面板填写大模型 API 密钥");
            shared.set_status("缺少大模型 API 密钥");
            continue;
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }

        shared.notifier.show_thinking(&current);
        shared.set_status("思考中…");
        shared.send(ConfigCommand::Status("思考中…".to_owned()));
        history.push(ChatMessage::user(text.clone()));
        let client = match ChatClient::new(&current) {
            Ok(client) => client,
            Err(error) => {
                shared.notifier.show_error(&error);
                shared.set_status("大模型配置错误");
                continue;
            }
        };
        match client.chat(&history) {
            Ok(reply) => {
                logging::info(&shared.host, &format!("AI 回复：{reply}"));
                // 纯净文本：去掉 ACTION 标记，只用于 TTS 朗读。
                let speak_text = ComputerAction::strip_action_tags(&reply);
                let mut display_reply = reply.clone();
                if current.enable_computer_actions {
                    if let Some(action) = ComputerAction::parse(&reply) {
                        match computer::execute(&action) {
                            Ok(result) => {
                                logging::info(
                                    &shared.host,
                                    &format!("电脑操作 {} 成功：{result}", action.label()),
                                );
                                display_reply = format!(
                                    "{reply}\n\n[已执行：{} - {}]",
                                    action.label(),
                                    result
                                );
                            }
                            Err(error) => {
                                logging::warn(
                                    &shared.host,
                                    &format!("电脑操作 {} 失败：{error}", action.label()),
                                );
                                display_reply = format!(
                                    "{reply}\n\n[执行 {} 失败：{1}]",
                                    action.label(),
                                    error
                                );
                            }
                        }
                    }
                }
                if current.speak_reply {
                    let speak_ms = 1_500 + speak_text.chars().count() as u64 * 180;
                    shared.siri.show_for(speak_ms);
                    shared.mute_listening(std::time::Duration::from_millis(speak_ms));
                    let _ = tts_tx.try_send(Some(speak_text));
                }
                shared.notifier.show_reply(&current, &display_reply);
                shared.set_status("待命");
                shared.send(ConfigCommand::Status("待命".to_owned()));
                history.push(ChatMessage::assistant(reply));
                if history.len() > 9 {
                    history.drain(1..3);
                }
            }
            Err(error) => {
                logging::warn(&shared.host, &format!("大模型请求失败：{error}"));
                shared.set_status("大模型请求失败");
                shared.notifier.show_error(&format!("大模型：{error}"));
                shared.send(ConfigCommand::Status("大模型请求失败".to_owned()));
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
