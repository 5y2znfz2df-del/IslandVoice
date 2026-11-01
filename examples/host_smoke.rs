//! Minimal ABI v2 mock-host smoke test for the built plugin DLL.
//!
//! Build the DLL, then:
//!   cargo build --release
//!   cargo run --example host-smoke -- target/release/island_voice.dll
//!
//! This exercises descriptor loading, create -> shutdown -> destroy, settings
//! page creation, command registration and Store config loading without
//! requiring a running WinIsland instance.

use std::ffi::c_void;

use winisland_plugin_api::types::v2::context::ContextDataV2;
use winisland_plugin_api::types::v2::settings::SettingsPageDataV2;
use winisland_plugin_api::{
    ABI_VERSION_2, CommandApiV2, CommandSpecV2, ContextApiV2, IFACE_COMMAND, IFACE_CONTEXT,
    IFACE_LOG, IFACE_SETTINGS, IFACE_STORE, IFACE_VERSION_1, LogApiV2, PluginCreateInfoV2,
    PluginDescriptorV2, PluginHostV2, PluginStatus, PluginToken, ResourceId, SettingsApiV2,
    StoreApiV2, TablePrefix, Utf8Slice,
};

const CONFIG_JSON: &[u8] =
    br#"{"auto_start":false,"wake_word":"hey siri","model":"deepseek-chat"}"#;
const CONFIG_JSON_AUDIO: &[u8] =
    br#"{"auto_start":true,"wake_word":"hey siri","model":"deepseek-chat"}"#;

fn smoke_config() -> &'static [u8] {
    if std::env::var_os("ISLANDVOICE_SMOKE_AUDIO").is_some() {
        CONFIG_JSON_AUDIO
    } else {
        CONFIG_JSON
    }
}

struct Mock {
    host: PluginHostV2,
    settings: SettingsApiV2,
    store: StoreApiV2,
    log: LogApiV2,
    command: CommandApiV2,
    context: ContextApiV2,
}

unsafe extern "C" fn query(context: *mut c_void, interface: u32, _version: u32) -> *const c_void {
    // SAFETY: context points at the Box<Mock> kept alive for the whole test.
    let mock = unsafe { &*(context as *const Mock) };
    match interface {
        IFACE_SETTINGS => (&mock.settings as *const SettingsApiV2).cast(),
        IFACE_STORE => (&mock.store as *const StoreApiV2).cast(),
        IFACE_LOG => (&mock.log as *const LogApiV2).cast(),
        IFACE_COMMAND => (&mock.command as *const CommandApiV2).cast(),
        IFACE_CONTEXT => (&mock.context as *const ContextApiV2).cast(),
        _ => std::ptr::null(),
    }
}

unsafe extern "C" fn settings_create(
    _ctx: *mut c_void,
    _token: PluginToken,
    data: *const SettingsPageDataV2,
    out: *mut ResourceId,
) -> PluginStatus {
    if data.is_null() || out.is_null() {
        return PluginStatus::InvalidArgument;
    }
    // SAFETY: host output buffer is writable.
    unsafe { out.write(ResourceId::from_raw(11)) };
    PluginStatus::Ok
}

unsafe extern "C" fn settings_update(
    _ctx: *mut c_void,
    _token: PluginToken,
    _id: ResourceId,
    _data: *const SettingsPageDataV2,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn settings_release(
    _ctx: *mut c_void,
    _token: PluginToken,
    _id: ResourceId,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn store_get(
    _ctx: *mut c_void,
    _token: PluginToken,
    _key: Utf8Slice,
    out: *mut u8,
    capacity: u32,
    required: *mut u32,
    found: *mut u8,
) -> PluginStatus {
    if required.is_null() || found.is_null() {
        return PluginStatus::InvalidArgument;
    }
    let config = smoke_config();
    let len = config.len() as u32;
    unsafe {
        required.write(len);
        found.write(1);
    }
    if !out.is_null() {
        if capacity < len {
            return PluginStatus::LimitExceeded;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(config.as_ptr(), out, len as usize);
        }
    }
    PluginStatus::Ok
}

unsafe extern "C" fn store_set(
    _ctx: *mut c_void,
    _token: PluginToken,
    _key: Utf8Slice,
    _value: winisland_plugin_api::ByteSlice,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn store_delete(
    _ctx: *mut c_void,
    _token: PluginToken,
    _key: Utf8Slice,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn log_write(
    _ctx: *mut c_void,
    _token: PluginToken,
    _level: u32,
    _message: Utf8Slice,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn command_register(
    _ctx: *mut c_void,
    _token: PluginToken,
    spec: *const CommandSpecV2,
    out: *mut ResourceId,
) -> PluginStatus {
    if spec.is_null() || out.is_null() {
        return PluginStatus::InvalidArgument;
    }
    unsafe { out.write(ResourceId::from_raw(21)) };
    PluginStatus::Ok
}

unsafe extern "C" fn command_release(
    _ctx: *mut c_void,
    _token: PluginToken,
    _id: ResourceId,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn context_create(
    _ctx: *mut c_void,
    _token: PluginToken,
    _data: *const ContextDataV2,
    out: *mut ResourceId,
) -> PluginStatus {
    unsafe { out.write(ResourceId::from_raw(31)) };
    PluginStatus::Ok
}

unsafe extern "C" fn context_update(
    _ctx: *mut c_void,
    _token: PluginToken,
    _id: ResourceId,
    _data: *const ContextDataV2,
) -> PluginStatus {
    PluginStatus::Ok
}

unsafe extern "C" fn context_release(
    _ctx: *mut c_void,
    _token: PluginToken,
    _id: ResourceId,
) -> PluginStatus {
    PluginStatus::Ok
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/release/island_voice.dll".to_owned());
    // SAFETY: loading the plugin DLL is the explicit purpose of this test.
    let library = unsafe { libloading::Library::new(&path) }.expect("load plugin DLL");
    // SAFETY: documented entry symbol and signature.
    let entry = unsafe {
        library
            .get::<unsafe extern "C" fn() -> *const PluginDescriptorV2>(
                b"winisland_plugin_entry_v2\0",
            )
            .expect("entry symbol")
    };
    // SAFETY: entry returns a static descriptor.
    let descriptor = unsafe { entry() };
    assert!(!descriptor.is_null());
    // SAFETY: descriptor is host-provided/static and complete.
    let descriptor = unsafe { &*descriptor };
    assert_eq!(descriptor.abi_version, ABI_VERSION_2);
    assert!(descriptor.create.is_some());

    let mut mock: Box<Mock> = Box::new(unsafe { std::mem::zeroed() });
    let context = (&mut *mock as *mut Mock).cast::<c_void>();
    mock.host.prefix = TablePrefix {
        struct_size: std::mem::size_of::<PluginHostV2>() as u32,
        version: ABI_VERSION_2,
        context,
    };
    mock.host.host_build = 1;
    mock.host.query = Some(query);

    mock.settings.prefix = TablePrefix {
        struct_size: std::mem::size_of::<SettingsApiV2>() as u32,
        version: IFACE_VERSION_1,
        context,
    };
    mock.settings.create = Some(settings_create);
    mock.settings.update = Some(settings_update);
    mock.settings.release = Some(settings_release);

    mock.store.prefix = TablePrefix {
        struct_size: std::mem::size_of::<StoreApiV2>() as u32,
        version: IFACE_VERSION_1,
        context,
    };
    mock.store.get = Some(store_get);
    mock.store.set = Some(store_set);
    mock.store.delete = Some(store_delete);

    mock.log.prefix = TablePrefix {
        struct_size: std::mem::size_of::<LogApiV2>() as u32,
        version: IFACE_VERSION_1,
        context,
    };
    mock.log.write = Some(log_write);

    mock.command.prefix = TablePrefix {
        struct_size: std::mem::size_of::<CommandApiV2>() as u32,
        version: IFACE_VERSION_1,
        context,
    };
    mock.command.register = Some(command_register);
    mock.command.release = Some(command_release);

    mock.context.prefix = TablePrefix {
        struct_size: std::mem::size_of::<ContextApiV2>() as u32,
        version: IFACE_VERSION_1,
        context,
    };
    mock.context.create = Some(context_create);
    mock.context.update = Some(context_update);
    mock.context.release = Some(context_release);

    let token = unsafe { PluginToken::from_raw(1) };
    let info = PluginCreateInfoV2 {
        struct_size: std::mem::size_of::<PluginCreateInfoV2>() as u32,
        abi_version: ABI_VERSION_2,
        plugin_token: token,
        host_api: &mock.host,
    };
    let mut handle = std::ptr::null_mut();
    let create = descriptor.create.expect("create callback");
    // SAFETY: info and handle live for the call; host table lives in `mock`.
    let status = unsafe { create(&info, &mut handle) };
    assert_eq!(
        status,
        PluginStatus::Ok,
        "create failed: {:?}",
        status.code()
    );
    assert!(!handle.is_null());

    // SAFETY: handle was produced by create.
    let status = unsafe { descriptor.shutdown.expect("shutdown")(handle) };
    assert_eq!(
        status,
        PluginStatus::Ok,
        "shutdown failed: {:?}",
        status.code()
    );
    // SAFETY: destroy is called once after shutdown.
    unsafe {
        descriptor.destroy.expect("destroy")(handle);
    }
    drop(mock);
    println!("host smoke test passed: {path}");
}
