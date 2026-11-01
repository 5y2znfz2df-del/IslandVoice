//! Declarative WinIsland settings-sidebar page.
//!
//! The official settings API supports sections, labels, switches, selects,
//! steppers and buttons. Text fields are therefore represented as buttons that
//! show the current value and open the native IslandVoice configuration panel.

use std::ffi::c_void;

use crossbeam_channel::Sender;
use winisland_plugin_api::types::v2::settings::{
    SETTINGS_ITEM_BUTTON, SETTINGS_ITEM_GROUP_END, SETTINGS_ITEM_GROUP_START, SETTINGS_ITEM_LABEL,
    SETTINGS_ITEM_SECTION, SETTINGS_ITEM_SWITCH, SettingsChangeV2, SettingsItemV2,
    SettingsPageDataV2,
};
use winisland_plugin_api::{ByteSlice, PluginStatus, ResourceId, str_to_fixed};

use crate::config::Config;
use crate::host::RawHost;
use crate::manager::ConfigCommand;
use crate::util::{fixed_str, mask_secret, truncate_bytes};

const PAGE_KEY: &str = "island-voice-settings";
const PAGE_TITLE: &str = "IslandVoice";
const ICON: &[u8] = include_bytes!("../icon.png");

struct SettingsCallback {
    tx: Sender<ConfigCommand>,
}

unsafe extern "C" fn on_change(
    callback_data: *mut c_void,
    _page_id: ResourceId,
    change: *const SettingsChangeV2,
) -> PluginStatus {
    if callback_data.is_null() || change.is_null() {
        return PluginStatus::InvalidArgument;
    }
    // SAFETY: callback_data points at the boxed SettingsCallback owned by SettingsPage.
    let callback = unsafe { &*callback_data.cast::<SettingsCallback>() };
    // SAFETY: the host passes a complete change record for this synchronous call.
    let change = unsafe { &*change };
    let key = fixed_str(&change.key);
    let value = fixed_str(&change.value);
    let _ = callback
        .tx
        .try_send(ConfigCommand::SettingChanged { key, value });
    PluginStatus::Ok
}

fn item(kind: u32, key: &str, label: &str, value: &str) -> SettingsItemV2 {
    let mut item = SettingsItemV2 {
        kind,
        ..SettingsItemV2::default()
    };
    item.key = str_to_fixed(key);
    item.label = str_to_fixed(label);
    item.value = str_to_fixed(value);
    item
}

fn switch(key: &str, label: &str, value: bool) -> SettingsItemV2 {
    item(
        SETTINGS_ITEM_SWITCH,
        key,
        label,
        if value { "true" } else { "false" },
    )
}

fn button(key: &str, label: &str, value: &str) -> SettingsItemV2 {
    item(SETTINGS_ITEM_BUTTON, key, label, value)
}

fn section(label: &str) -> SettingsItemV2 {
    item(SETTINGS_ITEM_SECTION, "", label, "")
}

fn label(text: &str) -> SettingsItemV2 {
    item(SETTINGS_ITEM_LABEL, "", text, "")
}

fn group_start(label: &str) -> SettingsItemV2 {
    item(SETTINGS_ITEM_GROUP_START, "", label, "")
}

fn group_end(label: &str) -> SettingsItemV2 {
    item(SETTINGS_ITEM_GROUP_END, "", label, "")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PageMode {
    Full,
    Core,
    Minimal,
    Static,
}

impl PageMode {
    fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Core => "core",
            Self::Minimal => "minimal",
            Self::Static => "static",
        }
    }
}

pub struct SettingsPage {
    raw: RawHost,
    id: ResourceId,
    callback: Option<Box<SettingsCallback>>,
    mode: PageMode,
    with_icon: bool,
}

impl SettingsPage {
    pub fn create(
        raw: RawHost,
        config: &Config,
        status: &crate::shared::StatusSnapshot,
        tx: Sender<ConfigCommand>,
        host: &winisland_plugin_api::sdk::Host,
    ) -> Option<Self> {
        let mut callback = Box::new(SettingsCallback { tx });

        // Diagnostic probes isolate host validation: callback, button item,
        // title and key are varied one at a time.
        for (key, title, with_button, with_callback) in [
            ("island-voice-p1", "Probe", false, false),
            ("island-voice-p2", "Probe", false, true),
            ("island-voice-p3", "Probe", true, false),
            ("island-voice-p4", "IslandVoice", false, false),
            ("island-voice-settings", "IslandVoice", false, false),
        ] {
            let mut probe_items = vec![section("Probe")];
            if with_button {
                probe_items.push(button("action-open-config", "打开配置", "打开"));
            }
            let mut probe = SettingsPageDataV2::default();
            probe.key = str_to_fixed(key);
            probe.title = str_to_fixed(title);
            probe.items = probe_items.as_ptr();
            probe.item_count = probe_items.len() as u32;
            if with_callback {
                probe.on_change = Some(on_change);
                probe.callback_data = (&mut *callback as *mut SettingsCallback).cast();
            }
            let (probe_id, probe_status) = raw.create_settings_page(&probe);
            crate::logging::warn(
                host,
                &format!(
                    "设置页探针 {key}：status={} id={} callback={with_callback} button={with_button}",
                    probe_status.code(),
                    probe_id.map(|id| id.get()).unwrap_or(0)
                ),
            );
            if let Some(probe_id) = probe_id {
                let _ = raw.release_settings_page(probe_id);
            }
        }

        let candidates = [
            (PageMode::Full, true),
            (PageMode::Full, false),
            (PageMode::Core, true),
            (PageMode::Core, false),
            (PageMode::Static, true),
            (PageMode::Static, false),
            (PageMode::Minimal, true),
            (PageMode::Minimal, false),
        ];
        for page_key in [PAGE_KEY, "islandvoice", "island_voice"] {
            for (mode, with_icon) in candidates {
                let items = build_items(config, status, mode);
                let mut data = SettingsPageDataV2::default();
                data.key = str_to_fixed(page_key);
                data.title = str_to_fixed(PAGE_TITLE);
                data.icon = if with_icon {
                    ByteSlice::borrowed(ICON)
                } else {
                    ByteSlice::empty()
                };
                data.items = items.as_ptr();
                data.item_count = items.len() as u32;
                data.on_change = Some(on_change);
                data.callback_data = (&mut *callback as *mut SettingsCallback).cast();
                let (id, create_status) = raw.create_settings_page(&data);
                if let Some(id) = id {
                    crate::logging::info(
                        host,
                        &format!(
                            "设置页已创建：key={} mode={} icon={} items={}",
                            page_key,
                            mode.label(),
                            with_icon,
                            items.len()
                        ),
                    );
                    return Some(Self {
                        raw,
                        id,
                        callback: Some(callback),
                        mode,
                        with_icon,
                    });
                }
                crate::logging::warn(
                    host,
                    &format!(
                        "创建设置页失败：key={} mode={} icon={} items={} status={}",
                        page_key,
                        mode.label(),
                        with_icon,
                        items.len(),
                        create_status.code()
                    ),
                );
            }
        }
        None
    }

    pub fn refresh(&self, config: &Config, status: &crate::shared::StatusSnapshot) {
        let Some(callback) = self.callback.as_ref() else {
            return;
        };
        let items = build_items(config, status, self.mode);
        let mut data = SettingsPageDataV2::default();
        data.key = str_to_fixed(PAGE_KEY);
        data.title = str_to_fixed(PAGE_TITLE);
        data.icon = if self.with_icon {
            ByteSlice::borrowed(ICON)
        } else {
            ByteSlice::empty()
        };
        data.items = items.as_ptr();
        data.item_count = items.len() as u32;
        data.on_change = Some(on_change);
        data.callback_data =
            (&**callback as *const SettingsCallback as *mut SettingsCallback).cast::<c_void>();
        let _ = self.raw.update_settings_page(self.id, &data);
    }
}

impl Drop for SettingsPage {
    fn drop(&mut self) {
        let released = self.raw.release_settings_page(self.id);
        let safe_to_drop = matches!(released, Some(PluginStatus::Ok | PluginStatus::StaleHandle));
        if let Some(callback) = self.callback.take() {
            if safe_to_drop {
                drop(callback);
            } else {
                // The host may still invoke the callback after a failed release.
                std::mem::forget(callback);
            }
        }
    }
}

fn build_items(
    config: &Config,
    status: &crate::shared::StatusSnapshot,
    mode: PageMode,
) -> Vec<SettingsItemV2> {
    let proxy = if config.http_proxy.is_empty() {
        "未使用".to_owned()
    } else {
        config.http_proxy.clone()
    };
    let mut items = vec![
        section("IslandVoice"),
        label("语音 AI 助手 · DeepSeek"),
        button("field_assistant_name", "助手名称", &config.assistant_name),
        button("field_wake_word", "唤醒词", &config.wake_word),
        label("大模型服务：DeepSeek（固定）"),
        button("field_api_key", "API 密钥", &mask_secret(&config.api_key)),
        button("field_model", "模型名称", &config.model),
        button("field_http_proxy", "HTTP 代理地址", &proxy),
        group_start("行为"),
        switch(
            "behavior_auto_start",
            "随 WinIsland 启动自动开启语音监听",
            config.auto_start,
        ),
        switch(
            "behavior_show_reply",
            "灵动岛显示 AI 回复",
            config.show_reply,
        ),
        switch("behavior_speak_reply", "朗读 AI 回复", config.speak_reply),
        group_end("行为"),
        group_start("操作"),
        button("action_open_config", "完整配置面板", "打开"),
        button("action_toggle_listening", "语音监听", "开始 / 停止"),
        button("action_listen_once", "立即说话（跳过唤醒词）", "开始"),
        button("action_test_deepseek", "DeepSeek 连接测试", "测试"),
        group_end("操作"),
        label(&format!("状态：{}", truncate_bytes(&status.state, 180))),
        label(&format!(
            "唤醒引擎：{}",
            truncate_bytes(&status.engine, 180)
        )),
        label(&format!(
            "识别方式：{} / 模型：{}",
            if config.stt_uses_online() {
                "在线 STT"
            } else {
                "离线 Vosk"
            },
            truncate_bytes(&config.stt_model, 100)
        )),
        label(&format!(
            "本地操作：{}",
            if config.enable_computer_actions {
                "已开启（整理桌面、查天气等）"
            } else {
                "已关闭"
            }
        )),
    ];
    match mode {
        PageMode::Full => {
            // Keep the page within the host's item budget.
            items.truncate(24);
            items
        }
        PageMode::Core => vec![
            section("IslandVoice"),
            label("语音 AI 助手 · DeepSeek"),
            switch(
                "behavior_auto_start",
                "随 WinIsland 启动自动开启语音监听",
                config.auto_start,
            ),
            switch(
                "behavior_show_reply",
                "灵动岛显示 AI 回复",
                config.show_reply,
            ),
            switch("behavior_speak_reply", "朗读 AI 回复", config.speak_reply),
            button("action_open_config", "完整配置面板", "打开"),
            button("action_listen_once", "立即说话（跳过唤醒词）", "开始"),
            button("action_test_deepseek", "DeepSeek 连接测试", "测试"),
        ],
        PageMode::Minimal => vec![
            section("IslandVoice"),
            button("action-open-config", "完整配置面板", "打开"),
        ],
        PageMode::Static => vec![
            section("IslandVoice"),
            label("语音 AI 助手 · DeepSeek"),
            label("请右键任务栏托盘中的 WinIsland 图标，选择“IslandVoice：配置”打开配置窗口。"),
            label(&format!("状态：{}", truncate_bytes(&status.state, 180))),
        ],
    }
}
