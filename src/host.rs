//! Thin wrapper over the ABI v2 host table.
//!
//! The Rust SDK exposes convenient wrappers for most services, but declarative
//! settings pages and rich context notifications still require the raw tables.
//! This wrapper validates the table layout the same way the SDK does and is
//! valid only while the plugin instance is alive.

use std::ptr::NonNull;

use winisland_plugin_api::types::v2::context::{
    CONTEXT_FLAG_SHOW_COMPACT, ContextDataV2, PRIORITY_HIGH,
};
use winisland_plugin_api::types::v2::settings::SettingsPageDataV2;
use winisland_plugin_api::{
    ABI_VERSION_2, ContextApiV2, IFACE_CONTEXT, IFACE_SETTINGS, IFACE_VERSION_1, PluginHostV2,
    PluginStatus, PluginToken, ResourceId, SettingsApiV2, TablePrefix, str_to_fixed,
};

#[derive(Clone, Copy)]
pub struct RawHost {
    table: NonNull<PluginHostV2>,
    token: PluginToken,
}

// SAFETY: the host table is immutable for the plugin lifetime and every service
// method is documented as safe to call from plugin-owned worker threads.
unsafe impl Send for RawHost {}
unsafe impl Sync for RawHost {}

impl RawHost {
    /// # Safety
    /// `table` must point at the ABI v2 host table supplied to `create`, and it
    /// must stay alive until the plugin's `shutdown` has returned.
    pub unsafe fn from_raw(table: *const PluginHostV2, token: PluginToken) -> Option<Self> {
        if table.is_null() || token == PluginToken::INVALID {
            return None;
        }
        // SAFETY: the caller guarantees a readable host table header.
        let prefix = unsafe { std::ptr::read_unaligned(table.cast::<TablePrefix>()) };
        if prefix.struct_size < std::mem::size_of::<PluginHostV2>() as u32
            || prefix.version != ABI_VERSION_2
            || prefix.context.is_null()
        {
            return None;
        }
        // SAFETY: the size check covers the complete table.
        let host = unsafe { std::ptr::read_unaligned(table) };
        host.query?;
        Some(Self {
            table: unsafe { NonNull::new_unchecked(table.cast_mut()) },
            token,
        })
    }

    fn query<T: Copy>(&self, interface: u32) -> Option<T> {
        // SAFETY: validated on construction and still inside the plugin lifetime.
        let host = unsafe { self.table.as_ref() };
        let query = host.query?;
        // SAFETY: the query function and instance context are host owned.
        let pointer = unsafe { query(host.prefix.context, interface, IFACE_VERSION_1) };
        if pointer.is_null() {
            return None;
        }
        // SAFETY: every service table starts with a readable prefix.
        let prefix = unsafe { std::ptr::read_unaligned(pointer.cast::<TablePrefix>()) };
        if prefix.struct_size < std::mem::size_of::<T>() as u32
            || prefix.version != IFACE_VERSION_1
            || prefix.context != host.prefix.context
        {
            return None;
        }
        // SAFETY: the validated size covers the whole copyable table.
        Some(unsafe { std::ptr::read_unaligned(pointer.cast::<T>()) })
    }

    /// Create a high priority notification/compact-island context.
    pub fn show_context(
        &self,
        title: &str,
        body: &str,
        compact: &str,
        timeout_ms: u32,
    ) -> Option<ResourceId> {
        let table = self.query::<ContextApiV2>(IFACE_CONTEXT)?;
        let create = table.create?;
        let mut data = ContextDataV2 {
            priority: PRIORITY_HIGH,
            flags: CONTEXT_FLAG_SHOW_COMPACT,
            timeout_ms,
            ..ContextDataV2::default()
        };
        data.title = str_to_fixed(title);
        data.body = str_to_fixed(body);
        data.compact_text = str_to_fixed(compact);
        let mut id = ResourceId::INVALID;
        // SAFETY: data and id live for the synchronous host call; the host copies them.
        let status = unsafe { create(table.prefix.context, self.token, &data, &mut id) };
        (status == PluginStatus::Ok && id != ResourceId::INVALID).then_some(id)
    }

    pub fn release_context(&self, id: ResourceId) {
        if id == ResourceId::INVALID {
            return;
        }
        let Some(table) = self.query::<ContextApiV2>(IFACE_CONTEXT) else {
            return;
        };
        let Some(release) = table.release else {
            return;
        };
        // SAFETY: the id was created by this plugin and the host validates ownership.
        let _ = unsafe { release(table.prefix.context, self.token, id) };
    }

    pub fn create_settings_page(
        &self,
        data: &SettingsPageDataV2,
    ) -> (Option<ResourceId>, PluginStatus) {
        let Some(table) = self.query::<SettingsApiV2>(IFACE_SETTINGS) else {
            return (None, PluginStatus::CapabilityMissing);
        };
        let Some(create) = table.create else {
            return (None, PluginStatus::UnsupportedVersion);
        };
        let mut id = ResourceId::INVALID;
        // SAFETY: data (including the items array) stays alive during the call.
        let status = unsafe { create(table.prefix.context, self.token, data, &mut id) };
        (
            (status == PluginStatus::Ok && id != ResourceId::INVALID).then_some(id),
            status,
        )
    }

    pub fn update_settings_page(&self, id: ResourceId, data: &SettingsPageDataV2) -> bool {
        let Some(table) = self.query::<SettingsApiV2>(IFACE_SETTINGS) else {
            return false;
        };
        let Some(update) = table.update else {
            return false;
        };
        // SAFETY: the page id is owned by this plugin and the items are live.
        unsafe { update(table.prefix.context, self.token, id, data) == PluginStatus::Ok }
    }

    pub fn release_settings_page(&self, id: ResourceId) -> Option<PluginStatus> {
        let table = self.query::<SettingsApiV2>(IFACE_SETTINGS)?;
        let release = table.release?;
        // SAFETY: the id is owned by this plugin and release waits for callbacks.
        Some(unsafe { release(table.prefix.context, self.token, id) })
    }
}
