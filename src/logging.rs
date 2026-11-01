//! Host logging helper.

use winisland_plugin_api::sdk::Host;

pub const ERROR: u32 = 1;
pub const WARN: u32 = 2;
pub const INFO: u32 = 3;

pub fn write(_host: &Host, level: u32, message: &str) {
    // Host log calls from audio/worker threads can stall under load; keep a
    // local append-only file instead. It is also easier for users to send us.
    crate::util::debug_line(&format!("[{level}] {message}"));
}

pub fn error(host: &Host, message: &str) {
    write(host, ERROR, message);
}

pub fn warn(host: &Host, message: &str) {
    write(host, WARN, message);
}

pub fn info(host: &Host, message: &str) {
    write(host, INFO, message);
}
