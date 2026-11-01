//! Small shared helpers: string truncation, wake-word matching, paths and URLs.

use std::path::{Path, PathBuf};

pub fn fixed_str(buffer: &[u8]) -> String {
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

pub fn truncate_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = value[..end].to_owned();
    out.push('…');
    out
}

/// Normalize spoken text for fuzzy wake-word matching.
///
/// Keeps letters and numbers (including CJK ideographs), lowercases Latin
/// characters and drops whitespace and punctuation.
pub fn normalize_speech(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// Match a wake word against a (partial or final) recognizer transcript.
///
/// Exact containment is preferred. Because small offline models often
/// transcribe unusual names phonetically (for example `siri` -> `siry`), a
/// conservative edit-distance fallback is also accepted.
pub fn wake_matches(transcript: &str, wake_word: &str) -> bool {
    let heard = normalize_speech(transcript);
    let wanted = normalize_speech(wake_word);
    if wanted.is_empty() || heard.is_empty() {
        return false;
    }
    if heard.contains(&wanted) {
        return true;
    }
    let wanted_chars: Vec<char> = wanted.chars().collect();
    let heard_chars: Vec<char> = heard.chars().collect();
    if wanted_chars.len() < 4 || heard_chars.len() < wanted_chars.len().saturating_sub(1) {
        return false;
    }
    let min_len = wanted_chars.len().saturating_sub(2).max(3);
    let max_len = (wanted_chars.len() + 2).min(heard_chars.len());
    for len in min_len..=max_len {
        let mut start = 0usize;
        while start + len <= heard_chars.len() {
            let window = &heard_chars[start..start + len];
            let distance = levenshtein(window, &wanted_chars);
            let longest = len.max(wanted_chars.len());
            let similarity = 1.0 - distance as f32 / longest as f32;
            if similarity >= 0.8 {
                return true;
            }
            start += 1;
        }
    }
    false
}

pub fn mask_secret(secret: &str) -> String {
    if secret.trim().is_empty() {
        return "未配置".to_owned();
    }
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 4 {
        return "••••".to_owned();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("••••{tail}")
}

pub fn api_url(base: &str, endpoint: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let endpoint = endpoint.trim_start_matches('/');
    if base.ends_with(endpoint) {
        base.to_owned()
    } else {
        format!("{base}/{endpoint}")
    }
}

pub fn appdata_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WinIsland")
        .join("IslandVoice")
}

/// Append a line to the plugin's own diagnostic log.
pub fn debug_line(message: &str) {
    use std::io::Write;
    let dir = appdata_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("debug.log"))
    else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(file, "[{now}] {message}");
}

pub fn module_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HMODULE;
        use windows_sys::Win32::System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW,
        };

        let mut module: HMODULE = std::ptr::null_mut();
        let address = module_dir as *const u16;
        let ok = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address,
                &mut module,
            )
        };
        if ok == 0 {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let len = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
        if len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        Path::new(&path).parent().map(Path::to_path_buf)
    }
    #[cfg(not(windows))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    }
}

pub fn play_cue(ok: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONASTERISK, MB_ICONHAND};
        unsafe {
            MessageBeep(if ok { MB_ICONASTERISK } else { MB_ICONHAND });
        }
    }
    #[cfg(not(windows))]
    {
        let _ = ok;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_word_exact_and_fuzzy() {
        assert!(wake_matches("hey siri", "hey siri"));
        assert!(wake_matches("Hey, Siri!", "hey siri"));
        assert!(wake_matches("hey siry", "hey siri"));
        assert!(!wake_matches("hello there", "hey siri"));
        assert!(wake_matches("你好小岛", "你好小岛"));
    }

    #[test]
    fn truncation_is_utf8_safe() {
        let text = "你好，世界";
        let cut = truncate_bytes(text, 7);
        assert!(text.starts_with(cut.trim_end_matches('…')));
    }

    #[test]
    fn urls_join_without_duplicate_segments() {
        assert_eq!(
            api_url("https://api.deepseek.com", "chat/completions"),
            "https://api.deepseek.com/chat/completions"
        );
        assert_eq!(
            api_url("https://api.deepseek.com/v1/", "/chat/completions"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }
}
