//! IslandVoice — a voice AI assistant plugin for WinIsland.
//!
//! Pipeline: microphone -> Vosk offline wake word -> command recording ->
//! online STT (or offline Vosk) -> DeepSeek chat -> island notification.

mod audio;
mod config;
mod config_window;
mod computer;
mod deepseek;
mod host;
mod island;
mod logging;
mod manager;
mod pipeline;
mod sapi_stt;
mod settings_page;
mod shared;
mod siri_effect;
mod stt;
mod tts;
mod util;
mod vosk;

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::unbounded;
use winisland_plugin_api::sdk::CallbackResource;
use winisland_plugin_api::sdk::Host;
use winisland_plugin_api::{
    ABI_VERSION_2, CAP_COMMAND, CAP_CONTEXT, CAP_SETTINGS, CAP_STORE, COMMAND_ENABLED,
    COMMAND_TRAY, CommandSpecV2, EVENT_COMMAND, IFACE_COMMAND, IFACE_CONTEXT, IFACE_SETTINGS,
    IFACE_STORE, MOD_ALT, MOD_CONTROL, PluginCreateInfoV2, PluginDescriptorV2,
    PluginDescriptorV2 as Descriptor, PluginHandleV2, PluginMetadataC, PluginStatus, PluginToken,
    str_to_fixed,
};

use crate::config::Config;
use crate::config_window::ConfigUi;
use crate::host::RawHost;
use crate::island::Notifier;
use crate::manager::ConfigCommand;
use crate::settings_page::SettingsPage;
use crate::shared::{Shared, StatusSnapshot};

static DESCRIPTOR: PluginDescriptorV2 = PluginDescriptorV2 {
    struct_size: std::mem::size_of::<PluginDescriptorV2>() as u32,
    abi_version: ABI_VERSION_2,
    capabilities: CAP_CONTEXT | CAP_SETTINGS | CAP_STORE | CAP_COMMAND,
    metadata: PluginMetadataC::new(
        "island-voice",
        "IslandVoice",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        env!("CARGO_PKG_DESCRIPTION"),
    ),
    create: Some(create),
    shutdown: Some(shutdown),
    destroy: Some(destroy),
    on_tick: None,
};

struct Plugin {
    shared: Arc<Shared>,
    manager: Option<JoinHandle<()>>,
    commands: Vec<CallbackResource>,
}

unsafe extern "C" fn create(
    info: *const PluginCreateInfoV2,
    out_handle: *mut PluginHandleV2,
) -> PluginStatus {
    if info.is_null() || out_handle.is_null() {
        return PluginStatus::InvalidArgument;
    }
    // SAFETY: the host supplies a readable create-info header.
    let info = unsafe { &*info };
    if info.struct_size < std::mem::size_of::<PluginCreateInfoV2>() as u32
        || info.abi_version != ABI_VERSION_2
        || info.plugin_token == PluginToken::INVALID
    {
        return PluginStatus::UnsupportedVersion;
    }
    let result = catch_unwind(AssertUnwindSafe(|| create_inner(info)));
    match result {
        Ok(Ok(plugin)) => {
            // SAFETY: the host treats the pointer as opaque until destroy.
            unsafe { out_handle.write(Box::into_raw(Box::new(plugin)).cast::<c_void>()) };
            PluginStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => PluginStatus::Internal,
    }
}

fn create_inner(info: &PluginCreateInfoV2) -> Result<Plugin, PluginStatus> {
    // SAFETY: validated by the caller; the host table lives for the instance.
    let host = unsafe { Host::from_raw(info.host_api, info.plugin_token) }
        .map_err(|_| PluginStatus::InvalidArgument)?;
    // SAFETY: the same host table is used for the raw service wrapper.
    let raw = unsafe { RawHost::from_raw(info.host_api, info.plugin_token) }
        .ok_or(PluginStatus::UnsupportedVersion)?;

    let config = Config::load(&host);
    logging::warn(
        &host,
        &format!(
            "host interfaces: settings={} command={} store={} context={}",
            host.supports(IFACE_SETTINGS),
            host.supports(IFACE_COMMAND),
            host.supports(IFACE_STORE),
            host.supports(IFACE_CONTEXT)
        ),
    );
    let status = StatusSnapshot::default();
    let (tx, rx) = unbounded::<ConfigCommand>();
    let shared = Arc::new(Shared {
        host: host.clone(),
        config: std::sync::Mutex::new(config.clone()),
        status: std::sync::Mutex::new(status.clone()),
        settings: std::sync::Mutex::new(None),
        notifier: Notifier::new(raw),
        engine: std::sync::Mutex::new(None),
        ui: ConfigUi::default(),
        tx: tx.clone(),
        shutdown: std::sync::atomic::AtomicBool::new(false),
        mute_until: std::sync::Mutex::new(None),
        siri: crate::siri_effect::SiriOverlay::new(),
    });

    if let Some(page) = SettingsPage::create(raw, &config, &status, tx.clone(), &host) {
        if let Ok(mut slot) = shared.settings.lock() {
            *slot = Some(page);
        }
    } else {
        logging::warn(&host, "无法创建设置页（host 可能不支持 SettingsApi）");
    }

    let commands = register_commands(&host, &tx);

    let manager_shared = shared.clone();
    let manager = std::thread::Builder::new()
        .name("island-voice-manager".to_owned())
        .spawn(move || manager::run(rx, manager_shared))
        .map_err(|error| {
            logging::error(&host, &format!("创建配置管理线程失败：{error}"));
            PluginStatus::Internal
        })?;

    if config.auto_start {
        let _ = tx.try_send(ConfigCommand::ToggleListening);
    } else {
        shared.set_status("已加载，语音监听未开启");
        shared.refresh_settings();
    }
    if config.api_key.trim().is_empty() {
        // The WinIsland 2.0 settings page currently rejects our page shape;
        // make sure the user can always reach the native configuration panel.
        let _ = tx.try_send(ConfigCommand::OpenConfig);
    }
    if std::env::var_os("ISLANDVOICE_TTS_TEST").is_some() {
        let _ = tx.try_send(ConfigCommand::TestTts);
    }
    if std::env::var_os("ISLANDVOICE_LISTEN_TEST").is_some() {
        let _ = tx.try_send(ConfigCommand::ListenOnce);
    }
    logging::info(
        &host,
        &format!(
            "IslandVoice {} 已加载（唤醒词：{}）",
            env!("CARGO_PKG_VERSION"),
            if config.wake_word.is_empty() {
                "未设置"
            } else {
                &config.wake_word
            }
        ),
    );

    Ok(Plugin {
        shared,
        manager: Some(manager),
        commands,
    })
}

#[derive(Clone, Copy)]
enum CommandAction {
    ListenOnce,
    ToggleListening,
    OpenConfig,
    TestTts,
}

fn register_commands(
    host: &Host,
    tx: &crossbeam_channel::Sender<ConfigCommand>,
) -> Vec<CallbackResource> {
    let mut commands = Vec::new();
    let Ok(api) = host.commands() else {
        logging::warn(host, "host 不支持 CommandApi，跳过托盘命令注册");
        return commands;
    };

    for (key, title, hotkey, action) in [
        (
            "listen-once",
            "IslandVoice：立即说话",
            0x56u32,
            CommandAction::ListenOnce,
        ),
        (
            "toggle-listening",
            "IslandVoice：开始 / 停止监听",
            0x4Au32,
            CommandAction::ToggleListening,
        ),
        (
            "open-config",
            "IslandVoice：配置",
            0u32,
            CommandAction::OpenConfig,
        ),
        (
            "test-tts",
            "IslandVoice：测试朗读",
            0u32,
            CommandAction::TestTts,
        ),
    ] {
        let mut registered = false;
        for (flags, modifiers, keycode, label) in [
            (
                COMMAND_ENABLED | COMMAND_TRAY,
                MOD_CONTROL | MOD_ALT,
                hotkey,
                "tray+hotkey",
            ),
            (COMMAND_ENABLED | COMMAND_TRAY, 0, 0, "tray"),
            (COMMAND_ENABLED, 0, 0, "enabled"),
        ] {
            let mut spec = CommandSpecV2 {
                flags,
                hotkey_modifiers: modifiers,
                hotkey_key: keycode,
                ..CommandSpecV2::default()
            };
            spec.key = str_to_fixed(key);
            spec.title = str_to_fixed(title);
            let sender = tx.clone();
            match api.register(spec, move |event| {
                if event.kind == EVENT_COMMAND {
                    let _ = sender.try_send(match action {
                        CommandAction::ListenOnce => ConfigCommand::ListenOnce,
                        CommandAction::ToggleListening => ConfigCommand::ToggleListening,
                        CommandAction::OpenConfig => ConfigCommand::OpenConfig,
                        CommandAction::TestTts => ConfigCommand::TestTts,
                    });
                }
            }) {
                Ok(resource) => {
                    commands.push(resource);
                    logging::warn(host, &format!("注册命令 {key} 成功（{label}）"));
                    registered = true;
                    break;
                }
                Err(error) => {
                    logging::warn(host, &format!("注册命令 {key} 失败（{label}）：{error}"))
                }
            }
        }
        if !registered {
            logging::warn(host, &format!("注册命令 {key} 全部变体均失败"));
        }
    }
    commands
}

unsafe extern "C" fn shutdown(handle: PluginHandleV2) -> PluginStatus {
    if handle.is_null() {
        return PluginStatus::InvalidArgument;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the handle came from create and has not been destroyed.
        let plugin = unsafe { &mut *handle.cast::<Plugin>() };
        plugin
            .shared
            .shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
        plugin.shared.ui.close();
        let _ = plugin.shared.tx.try_send(ConfigCommand::Shutdown);
        if let Some(manager) = plugin.manager.take() {
            let _ = manager.join();
        }
        plugin.shared.stop_engine();
        plugin.commands.clear();
        if let Ok(mut settings) = plugin.shared.settings.lock() {
            let _ = settings.take();
        }
        plugin.shared.notifier.clear();
        PluginStatus::Ok
    }));
    match result {
        Ok(status) => status,
        Err(_) => PluginStatus::Internal,
    }
}

unsafe extern "C" fn destroy(handle: PluginHandleV2) {
    if !handle.is_null() {
        // SAFETY: create returned this pointer and destroy is called once.
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(handle.cast::<Plugin>()));
        }));
    }
}

/// # Safety
/// WinIsland calls this exported symbol using the ABI v2 entry signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winisland_plugin_entry_v2() -> *const PluginDescriptorV2 {
    &DESCRIPTOR
}

// Keep the descriptor type import meaningful for readers of the ABI section.
const _: fn() -> &'static Descriptor = || &DESCRIPTOR;

/// Hidden diagnostic hook used by `examples/vosk-check.rs` and by maintainers
/// verifying a downloaded Vosk runtime without starting the full plugin.
#[doc(hidden)]
pub fn debug_transcribe_vosk(
    lib_path: &str,
    model_path: &str,
    pcm: &[i16],
) -> Result<String, String> {
    let config = Config {
        vosk_lib_path: lib_path.to_owned(),
        vosk_model_path: model_path.to_owned(),
        stt_vosk_model_path: model_path.to_owned(),
        stt_language: "zh".to_owned(),
        ..Config::default()
    };
    let runtime = vosk::VoskRuntime::load_stt(&config)?;
    runtime.transcribe(pcm)
}

/// Hidden diagnostic hook for the streaming wake-word path.
#[doc(hidden)]
pub fn debug_wake_check(
    lib_path: &str,
    model_path: &str,
    wake_word: &str,
    pcm: &[i16],
) -> Result<String, String> {
    let config = Config {
        vosk_lib_path: lib_path.to_owned(),
        vosk_model_path: model_path.to_owned(),
        wake_word: wake_word.to_owned(),
        ..Config::default()
    };
    let runtime = vosk::VoskRuntime::load(&config)?;
    let mut recognizer = runtime.wake_recognizer()?;
    for chunk in pcm.chunks(1_600) {
        let ended = recognizer.accept(chunk);
        let text = if ended {
            recognizer.result_text()
        } else {
            recognizer.partial_text()
        };
        if util::wake_matches(&text, wake_word) {
            return Ok(text);
        }
    }
    Ok(String::new())
}
