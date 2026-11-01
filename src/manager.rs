//! Serialized configuration and engine control.
//!
//! Settings callbacks, the native configuration window and command hotkeys all
//! post into this one manager thread. That keeps host API calls, persistence and
//! engine restarts off the audio thread and out of callback reentrancy.

use std::sync::Arc;

use crossbeam_channel::{Receiver, RecvTimeoutError};

use crate::Shared;
use crate::config::Config;
use crate::deepseek::ChatClient;
use crate::logging;

pub enum ConfigCommand {
    SettingChanged { key: String, value: String },
    Replace(Config),
    ToggleListening,
    ListenOnce,
    OpenConfig,
    TestTts,
    Status(String),
    Shutdown,
}

pub fn run(rx: Receiver<ConfigCommand>, shared: Arc<Shared>) {
    let mut config = shared.config();
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(ConfigCommand::SettingChanged { key, value }) => {
                handle_setting(&shared, &mut config, &key, &value);
            }
            Ok(ConfigCommand::Replace(next)) => {
                let next = next.sanitized();
                let old = config.clone();
                let running = shared
                    .engine
                    .lock()
                    .map(|slot| slot.is_some())
                    .unwrap_or(false);
                let restart = running && next.engine_needs_restart(&old);
                config = next;
                persist(&shared, &config);
                shared.set_config(config.clone());
                shared.refresh_settings();
                if !config.auto_start {
                    if running {
                        shared.stop_engine();
                    }
                } else if restart {
                    shared.stop_engine();
                    shared.start_engine();
                } else if !running {
                    shared.start_engine();
                }
                shared.notifier.show_info("IslandVoice", "配置已保存");
            }
            Ok(ConfigCommand::ToggleListening) => {
                shared.toggle_engine();
            }
            Ok(ConfigCommand::ListenOnce) => {
                shared.listen_once();
            }
            Ok(ConfigCommand::OpenConfig) => {
                shared.ui.open(shared.clone(), None);
            }
            Ok(ConfigCommand::TestTts) => {
                shared.test_tts();
            }
            Ok(ConfigCommand::Status(status)) => {
                let changed = shared
                    .status
                    .lock()
                    .map(|current| current.state != status)
                    .unwrap_or(true);
                if changed {
                    shared.set_status(status);
                    shared.refresh_settings();
                }
            }
            Ok(ConfigCommand::Shutdown) => {
                shared.stop_engine();
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                if shared.is_shutting_down() {
                    shared.stop_engine();
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                shared.stop_engine();
                break;
            }
        }
    }
}

fn handle_setting(shared: &Arc<Shared>, config: &mut Config, key: &str, value: &str) {
    if let Some(field) = key.strip_prefix("field_") {
        shared.ui.open(shared.clone(), Some(field.to_owned()));
        return;
    }
    match key {
        "behavior_auto_start" => {
            let enabled = parse_bool(value);
            config.auto_start = enabled;
            persist(shared, config);
            shared.set_config(config.clone());
            shared.refresh_settings();
            if enabled {
                shared.start_engine();
            } else {
                shared.stop_engine();
            }
        }
        "behavior_show_reply" => {
            config.show_reply = parse_bool(value);
            persist(shared, config);
            shared.set_config(config.clone());
            shared.refresh_settings();
        }
        "behavior_speak_reply" => {
            config.speak_reply = parse_bool(value);
            persist(shared, config);
            shared.set_config(config.clone());
            shared.refresh_settings();
        }
        "action_open_config" => {
            shared.ui.open(shared.clone(), None);
        }
        "action_toggle_listening" => shared.toggle_engine(),
        "action_listen_once" => shared.listen_once(),
        "action_test_deepseek" => test_deepseek(shared, config),
        other => {
            logging::warn(&shared.host, &format!("未知设置项：{other}"));
        }
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn persist(shared: &Shared, config: &Config) {
    if let Err(error) = config.save(&shared.host) {
        logging::error(&shared.host, &format!("配置保存失败：{error}"));
        shared
            .notifier
            .show_error(&format!("配置保存失败：{error}"));
    }
}

fn test_deepseek(shared: &Shared, config: &Config) {
    if config.api_key.trim().is_empty() {
        shared.notifier.show_error("请先填写 DeepSeek API 密钥");
        return;
    }
    shared.set_status("正在测试 DeepSeek 连接…");
    shared.refresh_settings();
    match ChatClient::new(config).and_then(|client| client.test()) {
        Ok(reply) => {
            shared.notifier.show_info(
                "大模型连接成功",
                &format!(
                    "模型 {} 正常响应：{}",
                    config.model,
                    reply.chars().take(40).collect::<String>()
                ),
            );
            shared.set_status("大模型连接正常");
        }
        Err(error) => {
            shared
                .notifier
                .show_error(&format!("大模型连接失败：{error}"));
            shared.set_status("大模型连接失败");
        }
    }
    shared.refresh_settings();
}
