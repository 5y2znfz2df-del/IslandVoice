//! Per-instance shared state used by the plugin, audio worker, pipeline and
//! configuration manager.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam_channel::Sender;
use winisland_plugin_api::sdk::Host;

use crate::audio::Engine;
use crate::config::Config;
use crate::config_window::ConfigUi;
use crate::island::Notifier;
use crate::manager::ConfigCommand;
use crate::settings_page::SettingsPage;

#[derive(Clone, Debug)]
pub struct StatusSnapshot {
    pub state: String,
    pub engine: String,
    pub last_error: String,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            state: "未启动".to_owned(),
            engine: "未加载".to_owned(),
            last_error: String::new(),
        }
    }
}

pub struct Shared {
    pub host: Host,
    pub config: Mutex<Config>,
    pub status: Mutex<StatusSnapshot>,
    pub settings: Mutex<Option<SettingsPage>>,
    pub notifier: Notifier,
    pub engine: Mutex<Option<Engine>>,
    pub ui: ConfigUi,
    pub tx: Sender<ConfigCommand>,
    pub shutdown: AtomicBool,
    pub mute_until: Mutex<Option<Instant>>,
    pub siri: crate::siri_effect::SiriOverlay,
}

impl Shared {
    pub fn send(&self, command: ConfigCommand) {
        let _ = self.tx.try_send(command);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Ignore microphone input while the TTS voice is playing so the assistant
    /// does not hear itself.
    pub fn mute_listening(&self, duration: std::time::Duration) {
        if let Ok(mut until) = self.mute_until.lock() {
            *until = Some(Instant::now() + duration);
        }
    }

    pub fn is_listening_muted(&self) -> bool {
        self.mute_until
            .lock()
            .ok()
            .and_then(|until| *until)
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    pub fn config(&self) -> Config {
        self.config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    pub fn set_config(&self, config: Config) {
        if let Ok(mut slot) = self.config.lock() {
            *slot = config;
        }
    }

    pub fn set_status(&self, state: impl Into<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.state = state.into();
        }
    }

    pub fn set_engine_status(&self, engine: impl Into<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.engine = engine.into();
        }
    }

    pub fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        if let Ok(mut status) = self.status.lock() {
            status.last_error = error.clone();
            status.state = format!("错误：{error}");
        }
        crate::logging::error(&self.host, &error);
        self.notifier.show_error(&error);
    }

    pub fn refresh_settings(&self) {
        let config = self.config();
        let status = self
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default();
        if let Ok(settings) = self.settings.lock() {
            if let Some(page) = settings.as_ref() {
                page.refresh(&config, &status);
            }
        }
    }

    pub fn start_engine(self: &Arc<Self>) {
        let mut slot = match self.engine.lock() {
            Ok(slot) => slot,
            Err(_) => return,
        };
        if slot.is_some() {
            return;
        }
        let config = self.config();
        match Engine::start(self.clone(), config) {
            Ok(engine) => {
                *slot = Some(engine);
            }
            Err(error) => {
                drop(slot);
                self.set_error(error);
            }
        }
    }

    pub fn stop_engine(&self) {
        let engine = self.engine.lock().ok().and_then(|mut slot| slot.take());
        if let Some(engine) = engine {
            engine.stop();
        }
        self.set_status("已停止");
    }

    pub fn toggle_engine(self: &Arc<Self>) {
        let running = self
            .engine
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false);
        if running {
            self.stop_engine();
            self.notifier.show_info("IslandVoice", "语音监听已停止");
        } else {
            self.start_engine();
            self.notifier.show_info("IslandVoice", "语音监听已开启");
        }
        self.refresh_settings();
    }

    pub fn listen_once(self: &Arc<Self>) {
        if self
            .engine
            .lock()
            .map(|slot| slot.is_none())
            .unwrap_or(true)
        {
            self.start_engine();
        }
        if let Ok(engine) = self.engine.lock() {
            if let Some(engine) = engine.as_ref() {
                engine.trigger_manual();
            }
        }
    }

    pub fn test_tts(self: &Arc<Self>) {
        if self
            .engine
            .lock()
            .map(|slot| slot.is_none())
            .unwrap_or(true)
        {
            self.start_engine();
        }
        if let Ok(engine) = self.engine.lock() {
            if let Some(engine) = engine.as_ref() {
                self.siri.show_for(3_000);
                engine.speak("你好，我是 IslandVoice，语音播报正常。");
            }
        }
    }
}
