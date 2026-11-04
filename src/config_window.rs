//! Native Win32 configuration panel.
//!
//! WinIsland's declarative settings API has switches, selects, steppers and
//! buttons but no text-input row. IslandVoice therefore opens a small native
//! panel for text fields while keeping the standard sidebar entry and switches.
//! The panel is created on its own thread, never on the audio or host worker.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, GetSysColorBrush, HGDIOBJ, SetBkMode,
    UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, LoadCursorW, MSG, MessageBoxW, PostMessageW,
    PostQuitMessage, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_SETFOCUS, WM_SETFONT,
    WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use crate::Shared;
use crate::config::{Config, SttMode};
use crate::manager::ConfigCommand;
use crate::util::wide;

const ID_SAVE: usize = 1001;
const ID_CANCEL: usize = 1002;
const WM_APP_OPEN: u32 = 0x8000 + 1;

const CLASS_NAME: &str = "IslandVoiceConfigWindow";

pub struct ConfigUi {
    hwnd: Arc<AtomicIsize>,
    closing: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Default for ConfigUi {
    fn default() -> Self {
        Self {
            hwnd: Arc::new(AtomicIsize::new(0)),
            closing: Arc::new(AtomicBool::new(false)),
            handle: Mutex::new(None),
        }
    }
}

impl ConfigUi {
    pub fn open(&self, shared: Arc<Shared>, focus: Option<String>) {
        if self.closing.load(Ordering::SeqCst) {
            return;
        }
        let existing = self.hwnd.load(Ordering::SeqCst);
        if existing != 0 {
            let hwnd = existing as HWND;
            if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(hwnd) } != 0 {
                unsafe {
                    SetForegroundWindow(hwnd);
                    PostMessageW(hwnd, WM_APP_OPEN, 0, 0);
                }
                return;
            }
        }
        let mut slot = match self.handle.lock() {
            Ok(slot) => slot,
            Err(_) => return,
        };
        if self.closing.load(Ordering::SeqCst) {
            return;
        }
        if let Some(handle) = slot.take() {
            let _ = handle.join();
        }
        let hwnd_slot = self.hwnd.clone();
        let closing = self.closing.clone();
        match thread::Builder::new()
            .name("island-voice-config-ui".to_owned())
            .spawn(move || {
                run_window(shared, focus, hwnd_slot, closing);
            }) {
            Ok(handle) => *slot = Some(handle),
            Err(error) => {
                let _ = error;
            }
        }
    }

    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let hwnd = self.hwnd.swap(0, Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Ok(mut slot) = self.handle.lock() {
            self.closing.store(true, Ordering::SeqCst);
            if let Some(handle) = slot.take() {
                if handle.thread().id() != thread::current().id() {
                    let _ = handle.join();
                }
            }
        }
    }
}

impl Drop for ConfigUi {
    fn drop(&mut self) {
        self.close();
    }
}

struct WindowState {
    shared: Arc<Shared>,
    assistant: HWND,
    wake: HWND,
    api_key: HWND,
    model: HWND,
    base_url: HWND,
    proxy: HWND,
    stt_mode: HWND,
    stt_base: HWND,
    stt_key: HWND,
    stt_model: HWND,
    stt_lang: HWND,
    vosk_model: HWND,
    vosk_lib: HWND,
    auto_start: HWND,
    show_reply: HWND,
    speak_reply: HWND,
    enable_actions: HWND,
    initial_focus: Option<String>,
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as usize;
            if id == ID_SAVE {
                if let Some(state) = state(hwnd) {
                    match read_config(state) {
                        Ok(config) => {
                            let _ = state.shared.tx.try_send(ConfigCommand::Replace(config));
                            unsafe {
                                DestroyWindow(hwnd);
                            }
                        }
                        Err(error) => {
                            let text = wide(&error);
                            let caption = wide("IslandVoice 配置");
                            unsafe {
                                MessageBoxW(hwnd, text.as_ptr(), caption.as_ptr(), 0x0000_0010);
                            }
                        }
                    }
                }
                return 0;
            }
            if id == ID_CANCEL {
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            0
        }
        WM_APP_OPEN => {
            if let Some(state) = state(hwnd) {
                if let Some(focus) = state.initial_focus.take() {
                    focus_field(state, &focus);
                }
            }
            unsafe {
                SetForegroundWindow(hwnd);
            }
            0
        }
        WM_CTLCOLORSTATIC => {
            unsafe {
                SetBkMode(lparam as *mut c_void, 1);
                GetSysColorBrush(COLOR_WINDOW) as LRESULT
            }
        }
        WM_SETFOCUS => {
            if let Some(state) = state(hwnd) {
                if let Some(focus) = state.initial_focus.take() {
                    focus_field(state, &focus);
                }
            }
            0
        }
        WM_DESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if pointer != 0 {
                unsafe {
                    drop(Box::from_raw(pointer as *mut WindowState));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
            }
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn state(hwnd: HWND) -> Option<&'static mut WindowState> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if pointer == 0 {
        None
    } else {
        Some(unsafe { &mut *(pointer as *mut WindowState) })
    }
}

fn run_window(
    shared: Arc<Shared>,
    focus: Option<String>,
    hwnd_slot: Arc<AtomicIsize>,
    closing: Arc<AtomicBool>,
) {
    let class_name = wide(CLASS_NAME);
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let wndclass = WNDCLASSW {
        style: 0x0002 | 0x0001,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };
    unsafe {
        RegisterClassW(&wndclass);
    }

    let config = shared.config();
    let scale = dpi_scale();
    let width = scaled(720.0, scale);
    let height = scaled(820.0, scale);
    let x = (unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CXSCREEN) }
        - width)
        / 2;
    let y = (unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CYSCREEN) }
        - height)
        / 2;
    let title = wide("IslandVoice 配置");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x.max(0),
            y.max(0),
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return;
    }
    hwnd_slot.store(hwnd as isize, Ordering::SeqCst);

    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    let state = Box::new(create_controls(hwnd, &config, font, shared, focus));
    let pointer = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, pointer as isize);
        if closing.load(Ordering::SeqCst) {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        } else {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            SetForegroundWindow(hwnd);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                0x0001 | 0x0002 | 0x0004,
            );
        }
    }

    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    hwnd_slot.store(0, Ordering::SeqCst);
}

fn create_controls(
    hwnd: HWND,
    config: &Config,
    font: HGDIOBJ,
    shared: Arc<Shared>,
    initial_focus: Option<String>,
) -> WindowState {
    let scale = dpi_scale();
    let margin = scaled(16.0, scale);
    let label_width = scaled(150.0, scale);
    let field_x = margin + label_width;
    let field_width = scaled(520.0, scale);
    let row = scaled(24.0, scale);
    let mut y = scaled(12.0, scale);
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };

    add_label(
        hwnd,
        instance,
        font,
        "IslandVoice 配置",
        margin,
        y,
        scaled(520.0, scale),
        scaled(20.0, scale),
        true,
    );
    y += scaled(24.0, scale);
    add_label(
        hwnd,
        instance,
        font,
        "语音 AI 助手 · DeepSeek",
        margin,
        y,
        scaled(520.0, scale),
        scaled(20.0, scale),
        false,
    );
    y += scaled(18.0, scale);

    let assistant = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.assistant_name,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "助手名称",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let wake = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.wake_word,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "唤醒词（示例：heySiri）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    add_label(
        hwnd,
        instance,
        font,
        "大模型服务",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "DeepSeek（固定）",
        field_x,
        y + scaled(4.0, scale),
        field_width,
        row,
        false,
    );
    y += row;
    let api_key = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.api_key,
        true,
    );
    add_label(
        hwnd,
        instance,
        font,
        "API 密钥",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let model = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.model,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "模型名称（如 deepseek-chat / gpt-3.5-turbo）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let base_url = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.deepseek_base_url,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "大模型地址（OpenAI 兼容）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let proxy = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.http_proxy,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "HTTP 代理地址（可选）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row + scaled(6.0, scale);

    add_label(
        hwnd,
        instance,
        font,
        "语音识别（高级）",
        margin,
        y,
        scaled(520.0, scale),
        scaled(20.0, scale),
        true,
    );
    y += scaled(20.0, scale);

    let stt_mode = add_combo(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        config.stt_mode,
    );
    add_label(
        hwnd,
        instance,
        font,
        "识别方式",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let stt_base = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.stt_base_url,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "STT 地址（OpenAI 兼容）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let stt_key = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.stt_api_key,
        true,
    );
    add_label(
        hwnd,
        instance,
        font,
        "STT 密钥",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let stt_model = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.stt_model,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "STT 模型",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let stt_lang = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.stt_language,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "识别语言（zh / en / auto）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let vosk_model = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.vosk_model_path,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "Vosk 模型目录（留空自动）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row;
    let vosk_lib = add_edit(
        hwnd,
        instance,
        font,
        field_x,
        y,
        field_width,
        row,
        &config.vosk_lib_path,
        false,
    );
    add_label(
        hwnd,
        instance,
        font,
        "libvosk.dll 路径（留空自动）",
        margin,
        y + scaled(4.0, scale),
        label_width,
        row,
        false,
    );
    y += row + scaled(6.0, scale);

    add_label(
        hwnd,
        instance,
        font,
        "行为",
        margin,
        y,
        scaled(520.0, scale),
        scaled(20.0, scale),
        true,
    );
    y += scaled(20.0, scale);
    let auto_start = add_checkbox(
        hwnd,
        instance,
        font,
        "随 WinIsland 启动自动开启语音监听",
        margin,
        y,
        scaled(520.0, scale),
        config.auto_start,
    );
    y += scaled(22.0, scale);
    let show_reply = add_checkbox(
        hwnd,
        instance,
        font,
        "灵动岛显示 AI 回复",
        margin,
        y,
        scaled(520.0, scale),
        config.show_reply,
    );
    y += scaled(22.0, scale);
    let speak_reply = add_checkbox(
        hwnd,
        instance,
        font,
        "朗读 AI 回复（Windows 语音）",
        margin,
        y,
        scaled(520.0, scale),
        config.speak_reply,
    );
    y += scaled(22.0, scale);
    let enable_actions = add_checkbox(
        hwnd,
        instance,
        font,
        "允许 AI 执行本地电脑操作（整理桌面、查天气等）",
        margin,
        y,
        scaled(520.0, scale),
        config.enable_computer_actions,
    );
    y += scaled(26.0, scale);

    let _hint = add_label(
        hwnd,
        instance,
        font,
        "离线唤醒需要 Vosk；在线 STT 填 SiliconFlow / Groq / OpenAI 兼容地址。密钥只保存在 WinIsland 插件存储中。",
        margin,
        y,
        scaled(520.0, scale),
        scaled(30.0, scale),
        false,
    );
    y += scaled(34.0, scale);

    add_button(
        hwnd,
        instance,
        font,
        "保存",
        field_x,
        y,
        scaled(100.0, scale),
        scaled(32.0, scale),
        ID_SAVE,
        true,
    );
    add_button(
        hwnd,
        instance,
        font,
        "取消",
        field_x + scaled(112.0, scale),
        y,
        scaled(100.0, scale),
        scaled(32.0, scale),
        ID_CANCEL,
        false,
    );

    let desired_height = y + scaled(50.0, scale);
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            scaled(720.0, scale),
            desired_height,
            0x0002 | 0x0004 | 0x0010,
        );
    }

    WindowState {
        shared,
        assistant,
        wake,
        api_key,
        model,
        base_url,
        proxy,
        stt_mode,
        stt_base,
        stt_key,
        stt_model,
        stt_lang,
        vosk_model,
        vosk_lib,
        auto_start,
        show_reply,
        speak_reply,
        enable_actions,
        initial_focus,
    }
}

fn add_label(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    font: HGDIOBJ,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    bold: bool,
) -> HWND {
    let class = wide("STATIC");
    let text = wide(text);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            width,
            height,
            parent,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    set_font(hwnd, font, bold);
    hwnd
}

fn add_edit(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    font: HGDIOBJ,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    value: &str,
    password: bool,
) -> HWND {
    let class = wide("EDIT");
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32;
    if password {
        style |= ES_PASSWORD as u32;
    }
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            style,
            x,
            y,
            width,
            height,
            parent,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    set_font(hwnd, font, false);
    let text = wide(value);
    unsafe {
        SetWindowTextW(hwnd, text.as_ptr());
    }
    hwnd
}

fn add_combo(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    font: HGDIOBJ,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    selected: SttMode,
) -> HWND {
    let class = wide("COMBOBOX");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
            x,
            y,
            width,
            height,
            parent,
            0 as usize as *mut c_void,
            instance,
            std::ptr::null(),
        )
    };
    set_font(hwnd, font, false);
    for text in [
        SttMode::Auto.label(),
        SttMode::Online.label(),
        SttMode::Offline.label(),
    ] {
        let text = wide(text);
        unsafe {
            SendMessageW(hwnd, CB_ADDSTRING, 0, text.as_ptr() as LPARAM);
        }
    }
    let index = match selected {
        SttMode::Auto => 0,
        SttMode::Online => 1,
        SttMode::Offline => 2,
    };
    unsafe {
        SendMessageW(hwnd, CB_SETCURSEL, index, 0);
    }
    hwnd
}

fn add_checkbox(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    font: HGDIOBJ,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    checked: bool,
) -> HWND {
    let class = wide("BUTTON");
    let text = wide(text);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            x,
            y,
            width,
            scaled(24.0, dpi_scale()),
            parent,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    set_font(hwnd, font, false);
    unsafe {
        SendMessageW(hwnd, BM_SETCHECK, usize::from(checked), 0);
    }
    hwnd
}

fn add_button(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    font: HGDIOBJ,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    default: bool,
) -> HWND {
    let class = wide("BUTTON");
    let text = wide(text);
    let style = WS_CHILD
        | WS_VISIBLE
        | WS_TABSTOP
        | if default {
            BS_DEFPUSHBUTTON as u32
        } else {
            BS_PUSHBUTTON as u32
        };
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            parent,
            id as usize as *mut c_void,
            instance,
            std::ptr::null(),
        )
    };
    set_font(hwnd, font, false);
    hwnd
}

fn set_font(hwnd: HWND, font: HGDIOBJ, _bold: bool) {
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
    }
}

fn dpi_scale() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    (dpi as f32 / 96.0).clamp(1.0, 1.6)
}

fn scaled(value: f32, scale: f32) -> i32 {
    (value * scale).round() as i32
}

fn get_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
        .trim()
        .to_owned()
}

fn checked(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, BM_GETCHECK, 0, 0) == 1 }
}

fn read_config(state: &WindowState) -> Result<Config, String> {
    let mut config = state.shared.config();
    config.assistant_name = get_text(state.assistant);
    config.wake_word = get_text(state.wake);
    config.api_key = get_text(state.api_key);
    config.model = get_text(state.model);
    config.deepseek_base_url = get_text(state.base_url);
    config.http_proxy = get_text(state.proxy);
    config.stt_base_url = get_text(state.stt_base);
    config.stt_api_key = get_text(state.stt_key);
    config.stt_model = get_text(state.stt_model);
    config.stt_language = get_text(state.stt_lang);
    config.vosk_model_path = get_text(state.vosk_model);
    config.vosk_lib_path = get_text(state.vosk_lib);
    config.auto_start = checked(state.auto_start);
    config.show_reply = checked(state.show_reply);
    config.speak_reply = checked(state.speak_reply);
    config.enable_computer_actions = checked(state.enable_actions);
    config.stt_mode = match unsafe { SendMessageW(state.stt_mode, CB_GETCURSEL, 0, 0) } {
        1 => SttMode::Online,
        2 => SttMode::Offline,
        _ => SttMode::Auto,
    };
    if config.assistant_name.is_empty() {
        config.assistant_name = "IslandVoice".to_owned();
    }
    if config.model.is_empty() {
        config.model = "deepseek-chat".to_owned();
    }
    let online_required = config.stt_mode == SttMode::Online
        || (config.stt_mode == SttMode::Auto && !config.stt_api_key.is_empty());
    if online_required && config.stt_base_url.is_empty() {
        return Err("在线识别需要填写 STT 地址".to_owned());
    }
    if !config.http_proxy.is_empty() {
        let lower = config.http_proxy.to_ascii_lowercase();
        if !(lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("socks5://")
            || lower.starts_with("socks5h://"))
        {
            return Err(
                "代理地址必须以 http://、https://、socks5:// 或 socks5h:// 开头".to_owned(),
            );
        }
    }
    Ok(config.sanitized())
}

fn focus_field(state: &WindowState, focus: &str) {
    let target = match focus {
        "assistant_name" => state.assistant,
        "wake_word" => state.wake,
        "api_key" => state.api_key,
        "model" => state.model,
        "http_proxy" => state.proxy,
        "stt_base_url" => state.stt_base,
        "stt_api_key" => state.stt_key,
        "stt_model" => state.stt_model,
        "stt_language" => state.stt_lang,
        "vosk_model_path" => state.vosk_model,
        "vosk_lib_path" => state.vosk_lib,
        _ => state.assistant,
    };
    unsafe {
        SetFocus(target);
    }
}
