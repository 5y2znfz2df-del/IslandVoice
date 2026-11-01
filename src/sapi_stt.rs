//! Offline Mandarin speech recognition through the Windows SAPI recognizer.
//!
//! Windows ships a zh-CN dictation recognizer on Chinese systems; it is fast
//! and needs no model download. We feed the recorded utterance as a WAV file
//! to `System.Speech.Recognition` in a hidden PowerShell process and read the
//! recognized text back as UTF-8.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::stt::wav_bytes;

const SCRIPT: &str = r#"
param([string]$Wave)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$r = New-Object System.Speech.Recognition.SpeechRecognitionEngine([System.Globalization.CultureInfo]::GetCultureInfo('zh-CN'))
$r.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
$r.SetInputToWaveFile($Wave)
$parts = New-Object System.Collections.Generic.List[string]
while ($true) {
    $res = $r.Recognize()
    if ($null -eq $res) { break }
    if ($res.Text) { $parts.Add($res.Text) }
}
$r.Dispose()
[Console]::Out.Write(($parts -join ''))
"#;

pub fn recognize(pcm: &[i16]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let wav = wav_bytes(pcm, 16_000);
    let dir = std::env::temp_dir();
    let wav_path = dir.join("islandvoice_zh.wav");
    let script_path = dir.join("islandvoice_zh.ps1");
    std::fs::write(&wav_path, wav).map_err(|error| format!("写入录音失败：{error}"))?;
    let mut script = std::fs::File::create(&script_path)
        .map_err(|error| format!("写入识别脚本失败：{error}"))?;
    script
        .write_all(SCRIPT.as_bytes())
        .map_err(|error| format!("写入识别脚本失败：{error}"))?;
    drop(script);

    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .arg("-Wave")
        .arg(&wav_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("启动 Windows 中文识别失败：{error}"))?;

    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() > Duration::from_secs(12) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Windows 中文识别超时".to_owned());
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(error) => return Err(format!("Windows 中文识别等待失败：{error}")),
        }
    }

    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let text = stdout.trim().to_owned();
    if text.is_empty() {
        return Err(format!(
            "Windows 中文识别没有结果{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!("：{}", crate::util::truncate_bytes(stderr.trim(), 300))
            }
        ));
    }
    Ok(text)
}
