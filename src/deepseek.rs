//! DeepSeek chat-completions client.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Config;
use crate::util::api_url;

pub const SYSTEM_PROMPT: &str = "你是运行在 WinIsland 灵动岛上的语音助手 IslandVoice。\
用户通过语音与你交流，你的回答会被朗读或显示在很小的胶囊里。\
请用简洁、自然、口语化的中文回答，默认控制在 100 字以内；\
只有在用户明确要求详细展开时才给出更长的解释。\
不要使用 Markdown 表格、代码块或复杂排版。";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_owned(),
            content: content.into(),
        }
    }
}

pub fn http_client(config: &Config, timeout: Duration) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(8))
        .user_agent(concat!("IslandVoice/", env!("CARGO_PKG_VERSION")));
    let proxy = config.http_proxy.trim();
    if !proxy.is_empty() {
        let lower = proxy.to_ascii_lowercase();
        if !(lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("socks5://")
            || lower.starts_with("socks5h://"))
        {
            return Err(
                "代理地址必须以 http://、https://、socks5:// 或 socks5h:// 开头".to_owned(),
            );
        }
        let proxy = reqwest::Proxy::all(proxy).map_err(|error| format!("代理地址无效：{error}"))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|error| format!("创建 HTTP 客户端失败：{error}"))
}

pub struct DeepSeekClient {
    client: Client,
    url: String,
    api_key: String,
    model: String,
}

impl DeepSeekClient {
    pub fn new(config: &Config) -> Result<Self, String> {
        if config.api_key.trim().is_empty() {
            return Err("尚未填写 DeepSeek API 密钥".to_owned());
        }
        let client = http_client(config, Duration::from_secs(30))?;
        let url = api_url(&config.deepseek_base_url, "chat/completions");
        Ok(Self {
            client,
            url,
            api_key: config.api_key.clone(),
            model: config.model.clone(),
        })
    }

    pub fn chat(&self, messages: &[ChatMessage]) -> Result<String, String> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
            "temperature": 0.7,
            "max_tokens": 768,
        });
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| {
                if error.is_timeout() {
                    "DeepSeek 请求超时，请检查网络或代理".to_owned()
                } else {
                    format!("DeepSeek 网络请求失败：{error}（国内网络可在配置中填写 HTTP 代理）")
                }
            })?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|error| format!("读取 DeepSeek 响应失败：{error}"))?;
        if !status.is_success() {
            let short = crate::util::truncate_bytes(text.trim(), 300);
            return Err(match status.as_u16() {
                401 => "DeepSeek API 密钥无效（401）".to_owned(),
                402 => "DeepSeek 账户余额不足（402）".to_owned(),
                429 => "DeepSeek 请求过于频繁（429），请稍后重试".to_owned(),
                _ => format!("DeepSeek 返回错误 HTTP {}：{}", status.as_u16(), short),
            });
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("DeepSeek 响应不是合法 JSON：{error}"))?;
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if content.is_empty() {
            return Err("DeepSeek 返回了空内容".to_owned());
        }
        Ok(content)
    }

    pub fn test(&self) -> Result<String, String> {
        let messages = vec![
            ChatMessage::system("你是一个连接测试助手，只回复 OK。"),
            ChatMessage::user("ping"),
        ];
        self.chat(&messages)
    }
}
