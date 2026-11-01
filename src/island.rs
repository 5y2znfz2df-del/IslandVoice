//! Rich island notifications built on the ABI v2 Context service.
//!
//! A high-priority context with `CONTEXT_FLAG_SHOW_COMPACT` is exactly what the
//! island uses for short-lived alerts and the compact capsule summary. The
//! plugin keeps one live context at a time and replaces it whenever a new
//! status/reply arrives.

use std::sync::Mutex;

use winisland_plugin_api::ResourceId;

use crate::config::Config;
use crate::host::RawHost;
use crate::util::{truncate_bytes, truncate_chars};

pub struct Notifier {
    raw: RawHost,
    current: Mutex<Option<ResourceId>>,
}

impl Notifier {
    pub fn new(raw: RawHost) -> Self {
        Self {
            raw,
            current: Mutex::new(None),
        }
    }

    fn show(&self, title: &str, body: &str, timeout_ms: u32) {
        let compact = truncate_chars(body, 44);
        let body = truncate_bytes(body, 480);
        let Ok(mut current) = self.current.lock() else {
            return;
        };
        if let Some(id) = current.take() {
            self.raw.release_context(id);
        }
        *current = self.raw.show_context(title, &body, &compact, timeout_ms);
    }

    pub fn show_thinking(&self, config: &Config) {
        if !config.show_reply {
            return;
        }
        self.show(&config.assistant_name, "正在思考…", 60_000);
    }

    pub fn show_reply(&self, config: &Config, reply: &str) {
        if !config.show_reply {
            return;
        }
        self.show(&config.assistant_name, reply, 20_000);
    }

    pub fn show_info(&self, title: &str, body: &str) {
        self.show(title, body, 10_000);
    }

    pub fn show_error(&self, message: &str) {
        self.show("IslandVoice", &format!("⚠ {message}"), 12_000);
    }

    pub fn clear(&self) {
        if let Ok(mut current) = self.current.lock() {
            if let Some(id) = current.take() {
                self.raw.release_context(id);
            }
        }
    }
}

impl Drop for Notifier {
    fn drop(&mut self) {
        self.clear();
    }
}
