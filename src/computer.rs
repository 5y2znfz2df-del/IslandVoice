//! 本地电脑操作模块。
//!
//! 提供与桌面、天气、应用程序等交互的能力，让 IslandVoice 不仅能“对话”，
//! 还能“做事”。所有操作均在本地执行，不上传用户文件。

use std::process::Command;
use std::time::Duration;

use reqwest::blocking::Client;

/// 本地操作的类型，用于 pipeline 解析 AI 回复中的动作标记，或直接通过命令触发。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComputerAction {
    /// 整理桌面：按文件类型将桌面文件归入子文件夹。
    OrganizeDesktop,
    /// 查看天气：需要城市名。
    Weather { city: String },
    /// 打开应用程序。
    OpenApp { name: String },
    /// 打开网页浏览器。
    OpenBrowser,
    /// 打开计算器。
    OpenCalculator,
    /// 未知/不支持的动作。
    Unknown(String),
}

impl ComputerAction {
    /// 从 AI 回复文本中提取动作标记，例如 `[ACTION: organize_desktop]` 或 `[ACTION: weather:北京]`。
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        for marker in ["[ACTION:", "[action:", "[动作:"] {
            let start = text.find(marker)?;
            let after = &text[start + marker.len()..];
            let end = after.find(']')?;
            let payload = &after[..end].trim();
            return Some(Self::parse_payload(payload));
        }
        None
    }

    fn parse_payload(payload: &str) -> Self {
        let parts: Vec<&str> = payload.splitn(2, ':').collect();
        match parts.get(0).map(|s| s.trim()) {
            Some("organize_desktop") | Some("整理桌面") => ComputerAction::OrganizeDesktop,
            Some("weather") | Some("天气") => {
                let city = parts.get(1).map(|s| s.trim()).unwrap_or("").to_owned();
                if city.is_empty() {
                    ComputerAction::Unknown(payload.to_owned())
                } else {
                    ComputerAction::Weather { city }
                }
            }
            Some("open_app") | Some("打开应用") => {
                let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_owned();
                if name.is_empty() {
                    ComputerAction::Unknown(payload.to_owned())
                } else {
                    ComputerAction::OpenApp { name }
                }
            }
            Some("open_browser") | Some("打开浏览器") => ComputerAction::OpenBrowser,
            Some("open_calculator") | Some("打开计算器") => ComputerAction::OpenCalculator,
            _ => ComputerAction::Unknown(payload.to_owned()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ComputerAction::OrganizeDesktop => "整理桌面",
            ComputerAction::Weather { .. } => "查看天气",
            ComputerAction::OpenApp { .. } => "打开应用",
            ComputerAction::OpenBrowser => "打开浏览器",
            ComputerAction::OpenCalculator => "打开计算器",
            ComputerAction::Unknown(_) => "未知动作",
        }
    }

    /// 从文本中移除 ACTION 标记，返回纯净文本，供 TTS 朗读使用。
    pub fn strip_action_tags(text: &str) -> String {
        let mut result = String::new();
        let mut pos = 0;
        let bytes = text.as_bytes();
        while pos < bytes.len() {
            if bytes[pos] == b'[' {
                let mut end = pos + 1;
                while end < bytes.len() && bytes[end] != b']' {
                    end += 1;
                }
                if end < bytes.len() {
                    end += 1; // 跳过 ]
                    pos = end;
                    continue;
                }
            }
            result.push(bytes[pos] as char);
            pos += 1;
        }
        result.trim().to_owned()
    }
}

/// 执行一个本地动作，返回人类可读的结果描述。
pub fn execute(action: &ComputerAction) -> Result<String, String> {
    match action {
        ComputerAction::OrganizeDesktop => organize_desktop(),
        ComputerAction::Weather { city } => get_weather(city),
        ComputerAction::OpenApp { name } => open_app(name),
        ComputerAction::OpenBrowser => open_browser(),
        ComputerAction::OpenCalculator => open_calculator(),
        ComputerAction::Unknown(text) => Err(format!("不支持的动作：{text}")),
    }
}

/// 整理桌面：按扩展名将桌面文件移入对应子文件夹。
fn organize_desktop() -> Result<String, String> {
    let desktop = dirs::desktop_dir()
        .ok_or_else(|| "未找到桌面文件夹".to_owned())?;
    let mut moved = 0usize;
    let mut skipped = 0usize;

    for entry in std::fs::read_dir(&desktop).map_err(|e| format!("读取桌面失败：{e}"))? {
        let entry = entry.map_err(|e| format!("读取条目失败：{e}"))?;
        let path = entry.path();
        if path.is_dir() {
            skipped += 1;
            continue;
        }
        let extension = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "其他".to_owned());
        let target_dir = desktop.join(&extension);
        std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建文件夹 {extension} 失败：{e}"))?;
        let target = target_dir.join(path.file_name().unwrap());
        if let Err(e) = std::fs::rename(&path, &target) {
            crate::util::debug_line(&format!("移动文件失败：{e}"));
            skipped += 1;
            continue;
        }
        moved += 1;
    }
    Ok(format!("桌面整理完成：移动 {moved} 个文件，跳过 {skipped} 个。"))
}

/// 查看天气：使用 wttr.in 公共接口，无需密钥。
fn get_weather(city: &str) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("IslandVoice/1.1.2")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;
    let url = format!("https://wttr.in/{city}?format=%l:+%c,+%t,+%h,+%w");
    let text = client
        .get(&url)
        .send()
        .map_err(|e| format!("天气请求失败：{e}"))?
        .text()
        .map_err(|e| format!("读取天气响应失败：{e}"))?;
    let text = text.trim();
    if text.is_empty() {
        return Err("天气服务返回空内容".to_owned());
    }
    Ok(format!("天气：{text}"))
}

/// 打开应用程序：通过 `start` 命令让 Windows 打开。
fn open_app(name: &str) -> Result<String, String> {
    Command::new("cmd")
        .args(["/C", "start", "", name])
        .spawn()
        .map_err(|e| format!("启动应用失败：{e}"))?;
    Ok(format!("已打开应用：{name}"))
}

/// 打开默认浏览器。
fn open_browser() -> Result<String, String> {
    Command::new("cmd")
        .args(["/C", "start", "", "https://www.baidu.com"])
        .spawn()
        .map_err(|e| format!("打开浏览器失败：{e}"))?;
    Ok("已打开浏览器".to_owned())
}

/// 打开计算器。
fn open_calculator() -> Result<String, String> {
    Command::new("cmd")
        .args(["/C", "start", "calc"])
        .spawn()
        .map_err(|e| format!("打开计算器失败：{e}"))?;
    Ok("已打开计算器".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_organize() {
        assert_eq!(ComputerAction::parse("[ACTION: organize_desktop]"), Some(ComputerAction::OrganizeDesktop));
        assert_eq!(ComputerAction::parse("[动作: 整理桌面]"), Some(ComputerAction::OrganizeDesktop));
    }

    #[test]
    fn parse_weather() {
        assert_eq!(
            ComputerAction::parse("[ACTION: weather:北京]"),
            Some(ComputerAction::Weather { city: "北京".to_owned() })
        );
        assert_eq!(
            ComputerAction::parse("[action: weather:上海]"),
            Some(ComputerAction::Weather { city: "上海".to_owned() })
        );
    }

    #[test]
    fn parse_unknown() {
        assert_eq!(
            ComputerAction::parse("[ACTION: foo:bar]"),
            Some(ComputerAction::Unknown("foo:bar".to_owned()))
        );
    }

    #[test]
    fn parse_none() {
        assert_eq!(ComputerAction::parse("你好，今天天气怎么样"), None);
        assert_eq!(ComputerAction::parse(""), None);
    }
}
