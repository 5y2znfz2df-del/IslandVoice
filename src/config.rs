//! Persistent plugin configuration.

use serde::{Deserialize, Serialize};
use winisland_plugin_api::sdk::Host;

use crate::logging;

pub const STORE_KEY: &str = "island-voice.config.v1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttMode {
    /// Use the online endpoint when an API key is configured, otherwise Vosk.
    #[default]
    Auto,
    /// Always use the OpenAI-compatible transcription endpoint.
    Online,
    /// Always use the local Vosk model.
    Offline,
}

impl SttMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "自动（有在线密钥则在线）",
            Self::Online => "在线（OpenAI 兼容）",
            Self::Offline => "离线（Vosk）",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub assistant_name: String,
    pub wake_word: String,
    pub api_key: String,
    pub model: String,
    pub deepseek_base_url: String,
    pub http_proxy: String,
    pub auto_start: bool,
    pub show_reply: bool,
    pub speak_reply: bool,
    pub tts_voice: String,
    pub stt_mode: SttMode,
    pub stt_base_url: String,
    pub stt_api_key: String,
    pub stt_model: String,
    pub stt_language: String,
    pub vosk_lib_path: String,
    pub vosk_model_path: String,
    pub stt_vosk_model_path: String,
    /// 允许 AI 执行本地电脑操作（整理桌面、查天气等）。
    pub enable_computer_actions: bool,
    /// 发送给大模型的系统提示词，可自定义助手行为。
    pub system_prompt: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            assistant_name: "IslandVoice".to_owned(),
            wake_word: "hey siri".to_owned(),
            api_key: String::new(),
            model: "deepseek-chat".to_owned(),
            deepseek_base_url: "https://api.deepseek.com".to_owned(),
            http_proxy: String::new(),
            auto_start: true,
            show_reply: true,
            speak_reply: true,
            tts_voice: "zh-CN-XiaoxiaoNeural".to_owned(),
            stt_mode: SttMode::Auto,
            stt_base_url: "https://api.siliconflow.cn/v1".to_owned(),
            stt_api_key: String::new(),
            stt_model: "FunAudioLLM/SenseVoiceSmall".to_owned(),
            stt_language: "zh".to_owned(),
            vosk_lib_path: String::new(),
            vosk_model_path: String::new(),
            stt_vosk_model_path: String::new(),
            enable_computer_actions: false,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
        }
    }
}

const DEFAULT_SYSTEM_PROMPT: &str = "你是运行在 WinIsland 灵动岛上的语音助手 IslandVoice。\
用户通过语音与你交流，你的回答会被朗读或显示在很小的胶囊里。\
请用简洁、自然、口语化的中文回答，默认控制在 100 字以内；\
只有在用户明确要求详细展开时才给出更长的解释。\
不要使用 Markdown 表格、代码块或复杂排版。";

impl Config {
    pub fn sanitized(mut self) -> Self {
        self.assistant_name = self.assistant_name.trim().to_owned();
        if self.assistant_name.is_empty() {
            self.assistant_name = "IslandVoice".to_owned();
        }
        self.wake_word = self.wake_word.trim().to_owned();
        self.api_key = self.api_key.trim().to_owned();
        self.model = self.model.trim().to_owned();
        if self.model.is_empty() {
            self.model = "deepseek-chat".to_owned();
        }
        self.http_proxy = self.http_proxy.trim().to_owned();
        self.deepseek_base_url = self
            .deepseek_base_url
            .trim()
            .trim_end_matches('/')
            .to_owned();
        if self.deepseek_base_url.is_empty() {
            self.deepseek_base_url = "https://api.deepseek.com".to_owned();
        }
        self.stt_base_url = self.stt_base_url.trim().trim_end_matches('/').to_owned();
        self.stt_api_key = self.stt_api_key.trim().to_owned();
        self.stt_model = self.stt_model.trim().to_owned();
        self.stt_language = self.stt_language.trim().to_lowercase();
        self.vosk_lib_path = self.vosk_lib_path.trim().to_owned();
        self.vosk_model_path = self.vosk_model_path.trim().to_owned();
        self
    }

    pub fn stt_uses_online(&self) -> bool {
        match self.stt_mode {
            SttMode::Online => true,
            SttMode::Offline => false,
            SttMode::Auto => !self.stt_api_key.is_empty() && !self.stt_model.is_empty(),
        }
    }

    pub fn wake_enabled(&self) -> bool {
        !self.wake_word.trim().is_empty()
    }

    pub fn engine_needs_restart(&self, other: &Self) -> bool {
        self.wake_word != other.wake_word
            || self.api_key != other.api_key
            || self.model != other.model
            || self.http_proxy != other.http_proxy
            || self.deepseek_base_url != other.deepseek_base_url
            || self.stt_mode != other.stt_mode
            || self.stt_base_url != other.stt_base_url
            || self.stt_api_key != other.stt_api_key
            || self.stt_model != other.stt_model
            || self.stt_language != other.stt_language
            || self.vosk_lib_path != other.vosk_lib_path
            || self.vosk_model_path != other.vosk_model_path
            || self.stt_vosk_model_path != other.stt_vosk_model_path
            || self.tts_voice != other.tts_voice
            || self.enable_computer_actions != other.enable_computer_actions
            || self.system_prompt != other.system_prompt
    }

    pub fn load(host: &Host) -> Self {
        let Ok(store) = host.store() else {
            logging::warn(host, "持久化存储不可用，使用默认配置");
            return Self::default();
        };
        match store.get(STORE_KEY) {
            Ok(Some(bytes)) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(config) => config.sanitized(),
                Err(error) => {
                    logging::warn(host, &format!("配置 JSON 解析失败，使用默认配置：{error}"));
                    Self::default()
                }
            },
            Ok(None) => Self::default(),
            Err(error) => {
                logging::warn(host, &format!("读取配置失败，使用默认配置：{error}"));
                Self::default()
            }
        }
    }

    pub fn save(&self, host: &Host) -> Result<(), String> {
        let store = host.store().map_err(|e| format!("存储接口不可用：{e}"))?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| format!("序列化配置失败：{e}"))?;
        store
            .set(STORE_KEY, &bytes)
            .map_err(|e| format!("写入配置失败：{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let mut config = Config::default();
        config.assistant_name = "小岛".to_owned();
        config.api_key = "sk-test".to_owned();
        config.stt_mode = SttMode::Offline;
        let bytes = serde_json::to_vec(&config).unwrap();
        let restored: Config = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.assistant_name, "小岛");
        assert_eq!(restored.stt_mode, SttMode::Offline);
    }

    #[test]
    fn empty_defaults_are_repaired() {
        let config = Config {
            assistant_name: "  ".to_owned(),
            model: String::new(),
            deepseek_base_url: String::new(),
            ..Config::default()
        }
        .sanitized();
        assert_eq!(config.assistant_name, "IslandVoice");
        assert_eq!(config.model, "deepseek-chat");
        assert_eq!(config.deepseek_base_url, "https://api.deepseek.com");
    }
}
