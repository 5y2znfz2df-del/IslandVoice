//! Speech-to-text: OpenAI-compatible online endpoint with an offline Vosk
//! fallback. Online providers that work with this module include SiliconFlow
//! (SenseVoice / Whisper), Groq (Whisper) and a local whisper.cpp server.

use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::multipart::{Form, Part};
use serde_json::Value;

use crate::config::{Config, SttMode};
use crate::deepseek::http_client;
use crate::util::api_url;
use crate::vosk::VoskRuntime;

pub fn transcribe(
    config: &Config,
    pcm: &[i16],
    vosk: Option<&Arc<VoskRuntime>>,
) -> Result<String, String> {
    if pcm.is_empty() {
        return Err("录音为空".to_owned());
    }
    if !matches!(config.stt_mode, SttMode::Online)
        && config
            .stt_language
            .trim()
            .to_ascii_lowercase()
            .starts_with("zh")
    {
        let vosk_text = vosk
            .and_then(|runtime| runtime.transcribe(pcm).ok())
            .unwrap_or_default();
        let vosk_cjk = vosk_text.chars().filter(|c| is_cjk(*c)).count();
        crate::util::debug_line(&format!(
            "STT 中文模型：{:?} (cjk={vosk_cjk})",
            vosk_text.trim()
        ));
        if vosk_cjk > 0 {
            return Ok(vosk_text);
        }
        let sapi_text = match crate::sapi_stt::recognize(pcm) {
            Ok(text) => text,
            Err(error) => {
                crate::util::debug_line(&format!("SAPI 中文识别失败：{error}"));
                String::new()
            }
        };
        crate::util::debug_line(&format!("STT SAPI 兜底：{:?}", sapi_text.trim()));
        if !sapi_text.trim().is_empty() {
            return Ok(sapi_text);
        }
        if !vosk_text.trim().is_empty() {
            return Ok(vosk_text);
        }
    }
    match config.stt_mode {
        SttMode::Online => transcribe_online(config, pcm),
        SttMode::Offline => transcribe_offline(vosk, pcm),
        SttMode::Auto => {
            if config.stt_uses_online() {
                match transcribe_online(config, pcm) {
                    Ok(text) => Ok(text),
                    Err(online_error) => {
                        if vosk.is_some() {
                            transcribe_offline(vosk, pcm).map_err(|offline_error| {
                                format!(
                                    "在线识别失败：{online_error}；离线识别也失败：{offline_error}"
                                )
                            })
                        } else {
                            Err(online_error)
                        }
                    }
                }
            } else {
                transcribe_offline(vosk, pcm)
            }
        }
    }
}

fn transcribe_offline(vosk: Option<&Arc<VoskRuntime>>, pcm: &[i16]) -> Result<String, String> {
    let vosk = vosk.ok_or_else(|| {
        "离线识别不可用：请先安装 Vosk 运行时和模型（见 docs/INSTALL.md）".to_owned()
    })?;
    vosk.transcribe(pcm)
}

fn transcribe_online(config: &Config, pcm: &[i16]) -> Result<String, String> {
    if config.stt_base_url.trim().is_empty() {
        return Err("未填写在线语音识别地址".to_owned());
    }
    if config.stt_model.trim().is_empty() {
        return Err("未填写在线语音识别模型".to_owned());
    }
    let client = http_client(config, Duration::from_secs(60))?;
    let url = api_url(&config.stt_base_url, "audio/transcriptions");
    let wav = wav_bytes(pcm, 16_000);
    let file = Part::bytes(wav)
        .file_name("speech.wav")
        .mime_str("audio/wav")
        .map_err(|error| format!("构造上传数据失败：{error}"))?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", config.stt_model.clone());
    let language = config.stt_language.trim();
    if !language.is_empty() && language != "auto" {
        form = form.text("language", language.to_owned());
    }
    let mut request = client.post(url).multipart(form);
    if !config.stt_api_key.trim().is_empty() {
        request = request.bearer_auth(config.stt_api_key.trim());
    }
    let response = request
        .send()
        .map_err(|error| format!("在线识别请求失败：{error}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("读取在线识别响应失败：{error}"))?;
    if !status.is_success() {
        return Err(format!(
            "在线识别返回 HTTP {}：{}",
            status.as_u16(),
            crate::util::truncate_bytes(body.trim(), 300)
        ));
    }
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("在线识别响应不是合法 JSON：{error}"))?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/result/0/text").and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_owned();
    if text.is_empty() {
        return Err("在线识别没有返回文字".to_owned());
    }
    Ok(text)
}

pub fn wav_bytes(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_correct() {
        let pcm = vec![0i16; 1600];
        let wav = wav_bytes(&pcm, 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + pcm.len() * 2);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(
            u32::from_le_bytes(wav[40..44].try_into().unwrap()),
            (pcm.len() * 2) as u32
        );
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}
