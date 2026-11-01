//! 大模型对话客户端。
//!
//! 支持任意 OpenAI 兼容的 Chat Completions 接口（DeepSeek、硅云、Groq、
//! 本地 vLLM / Ollama 等）。通过配置的 `base_url`、`api_key`、`model`
//! 决定调用哪个服务；不再强制绑定 DeepSeek 官方。

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Config;
use crate::util::api_url;

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

pub struct ChatClient {
    client: Client,
    url: String,
    api_key: String,
    model: String,
}

impl ChatClient {
    pub fn new(config: &Config) -> Result<Self, String> {
        if config.api_key.trim().is_empty() {
            return Err("尚未填写大模型 API 密钥".to_owned());
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
                    "大模型请求超时，请检查网络或代理".to_owned()
                } else {
                    format!("大模型网络请求失败：{error}（国内网络可在配置中填写 HTTP 代理）")
                }
            })?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|error| format!("读取大模型响应失败：{error}"))?;
        if !status.is_success() {
            let short = crate::util::truncate_bytes(text.trim(), 300);
            return Err(match status.as_u16() {
                401 => "大模型 API 密钥无效（401）".to_owned(),
                402 => "大模型账户余额不足（402）".to_owned(),
                429 => "大模型请求过于频繁（429），请稍后重试".to_owned(),
                _ => format!("大模型返回错误 HTTP {}：{}", status.as_u16(), short),
            });
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("大模型响应不是合法 JSON：{error}"))?;
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if content.is_empty() {
            return Err("大模型返回了空内容".to_owned());
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
