//! Free, offline wake-word / speech recognition through the Vosk C API.
//!
//! Vosk is Apache-2.0 licensed. The official Windows runtime is loaded
//! dynamically from `libvosk.dll`, so the plugin itself has no build-time C
//! dependency and can degrade gracefully when the runtime is absent.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW, LoadLibraryW,
};

use crate::config::Config;
use crate::util::{appdata_dir, module_dir, wide};
type ModelNew = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type ModelFree = unsafe extern "C" fn(*mut c_void);
type RecognizerNew = unsafe extern "C" fn(*mut c_void, f32) -> *mut c_void;
type AcceptWaveform = unsafe extern "C" fn(*mut c_void, *const c_char, i32) -> i32;
type RecognizerResult = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type RecognizerFree = unsafe extern "C" fn(*mut c_void);
type SetLogLevel = unsafe extern "C" fn(i32);

struct VoskApi {
    module: HMODULE,
    model_new: ModelNew,
    model_free: ModelFree,
    recognizer_new: RecognizerNew,
    accept_waveform: AcceptWaveform,
    result: RecognizerResult,
    partial_result: RecognizerResult,
    final_result: RecognizerResult,
    recognizer_free: RecognizerFree,
}

// SAFETY: Vosk's runtime is thread-safe for independent recognizers sharing an
// immutable model, and the HMODULE is kept alive for the whole runtime.
unsafe impl Send for VoskApi {}
unsafe impl Sync for VoskApi {}

impl Drop for VoskApi {
    fn drop(&mut self) {
        if !self.module.is_null() {
            // SAFETY: the module was loaded by VoskApi::load.
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }
}

impl VoskApi {
    fn load(config: &Config) -> Result<Self, String> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if !config.vosk_lib_path.trim().is_empty() {
            candidates.push(PathBuf::from(config.vosk_lib_path.trim()));
        }
        let data = appdata_dir();
        candidates.push(data.join("vosk").join("libvosk.dll"));
        candidates.push(data.join("libvosk.dll"));
        if let Some(dir) = module_dir() {
            candidates.push(dir.join("runtime").join("libvosk.dll"));
            candidates.push(dir.join("libvosk.dll"));
        }
        candidates.push(PathBuf::from("libvosk.dll"));

        let mut last_error = String::new();
        for path in &candidates {
            if !path.is_file() {
                continue;
            }
            match unsafe { Self::load_from_path(path) } {
                Ok(api) => return Ok(api),
                Err(error) => last_error = error,
            }
        }
        // Let the Windows loader search its normal paths (including PATH).
        match unsafe { Self::load_from_path(Path::new("libvosk.dll")) } {
            Ok(api) => Ok(api),
            Err(error) => Err(if last_error.is_empty() {
                format!(
                    "未找到 libvosk.dll（{error}）。请运行 scripts/fetch-vosk.ps1 或手动放置运行时。"
                )
            } else {
                format!("libvosk.dll 加载失败：{last_error}；最后错误：{error}")
            }),
        }
    }

    unsafe fn load_from_path(path: &Path) -> Result<Self, String> {
        let wide_path = wide(&path.to_string_lossy());
        // SAFETY: the path is NUL terminated and the flag makes dependent DLLs
        // resolve from the runtime directory.
        let mut module = unsafe {
            LoadLibraryExW(
                wide_path.as_ptr(),
                std::ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if module.is_null() {
            let name = wide("libvosk.dll");
            // SAFETY: fallback to the loader search path.
            module = unsafe { LoadLibraryW(name.as_ptr()) };
        }
        if module.is_null() {
            return Err(format!(
                "LoadLibraryExW 失败（{}）",
                std::io::Error::last_os_error()
            ));
        }
        let api = Self {
            module,
            model_new: unsafe { resolve(module, b"vosk_model_new\0")? },
            model_free: unsafe { resolve(module, b"vosk_model_free\0")? },
            recognizer_new: unsafe { resolve(module, b"vosk_recognizer_new\0")? },
            accept_waveform: unsafe { resolve(module, b"vosk_recognizer_accept_waveform\0")? },
            result: unsafe { resolve(module, b"vosk_recognizer_result\0")? },
            partial_result: unsafe { resolve(module, b"vosk_recognizer_partial_result\0")? },
            final_result: unsafe { resolve(module, b"vosk_recognizer_final_result\0")? },
            recognizer_free: unsafe { resolve(module, b"vosk_recognizer_free\0")? },
        };
        if let Ok(set_log_level) =
            unsafe { resolve::<SetLogLevel>(module, b"vosk_set_log_level\0") }
        {
            // SAFETY: documented Vosk entry point; -1 silences the Kaldi log.
            unsafe { set_log_level(-1) };
        }
        Ok(api)
    }
}

unsafe fn resolve<T: Copy>(module: HMODULE, name: &[u8]) -> Result<T, String> {
    // SAFETY: the caller passes a NUL-terminated symbol name.
    let proc = unsafe { GetProcAddress(module, name.as_ptr()) };
    let Some(proc) = proc else {
        return Err(format!(
            "libvosk.dll 缺少导出 {}",
            String::from_utf8_lossy(name).trim_end_matches('\0')
        ));
    };
    Ok(unsafe { std::mem::transmute_copy(&proc) })
}

struct VoskModel {
    api: Arc<VoskApi>,
    pointer: *mut c_void,
}

// SAFETY: a Vosk model is immutable after loading; recognizers are per-thread.
unsafe impl Send for VoskModel {}
unsafe impl Sync for VoskModel {}

impl Drop for VoskModel {
    fn drop(&mut self) {
        // SAFETY: pointer was created by vosk_model_new.
        unsafe { (self.api.model_free)(self.pointer) };
    }
}

pub struct VoskRecognizer {
    api: Arc<VoskApi>,
    _model: Arc<VoskModel>,
    pointer: *mut c_void,
}

impl VoskRecognizer {
    fn new(model: Arc<VoskModel>) -> Result<Self, String> {
        // SAFETY: model pointer stays alive through the recognizer's `_model`.
        let pointer = unsafe { (model.api.recognizer_new)(model.pointer, 16_000.0) };
        if pointer.is_null() {
            return Err("创建 Vosk recognizer 失败".to_owned());
        }
        Ok(Self {
            api: model.api.clone(),
            _model: model,
            pointer,
        })
    }

    /// Feed 16 kHz mono PCM. Returns true when Vosk detected an utterance end.
    pub fn accept(&mut self, pcm: &[i16]) -> bool {
        if pcm.is_empty() {
            return false;
        }
        let bytes = unsafe { std::slice::from_raw_parts(pcm.as_ptr().cast::<u8>(), pcm.len() * 2) };
        // SAFETY: pointer is a live recognizer and bytes are valid for this call.
        unsafe {
            (self.api.accept_waveform)(
                self.pointer,
                bytes.as_ptr().cast::<c_char>(),
                bytes.len() as i32,
            ) == 1
        }
    }

    pub fn result_text(&self) -> String {
        self.read(self.api.result)
    }

    pub fn partial_text(&self) -> String {
        self.read(self.api.partial_result)
    }

    pub fn final_text(&self) -> String {
        self.read(self.api.final_result)
    }

    fn read(&self, function: RecognizerResult) -> String {
        // SAFETY: the returned C string is owned by the recognizer and valid
        // until the next recognizer call, which cannot happen concurrently.
        let pointer = unsafe { function(self.pointer) };
        if pointer.is_null() {
            return String::new();
        }
        let text = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        parse_text(&text)
    }
}

impl Drop for VoskRecognizer {
    fn drop(&mut self) {
        // SAFETY: pointer was created by vosk_recognizer_new.
        unsafe { (self.api.recognizer_free)(self.pointer) };
    }
}

fn parse_text(json: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return String::new();
    };
    value
        .get("text")
        .or_else(|| value.get("partial"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub struct VoskRuntime {
    model: Arc<VoskModel>,
    pub model_dir: PathBuf,
}

impl VoskRuntime {
    pub fn load(config: &Config) -> Result<Arc<Self>, String> {
        let model_dir = resolve_model_dir(config)?;
        Self::load_dir(config, model_dir)
    }

    /// Load the Chinese speech-to-text model while keeping the English wake
    /// model. Falls back to the wake model when no Chinese model is installed.
    pub fn load_stt(config: &Config) -> Result<Arc<Self>, String> {
        if !config.stt_vosk_model_path.trim().is_empty() {
            return Self::load_dir(config, PathBuf::from(config.stt_vosk_model_path.trim()));
        }
        if let Some(dir) = find_chinese_model(config) {
            return Self::load_dir(config, dir);
        }
        Self::load(config)
    }

    fn load_dir(config: &Config, model_dir: PathBuf) -> Result<Arc<Self>, String> {
        let api = Arc::new(VoskApi::load(config)?);
        let path = CString::new(model_dir.to_string_lossy().as_bytes())
            .map_err(|_| "模型路径包含非法字符".to_owned())?;
        // SAFETY: path is NUL terminated and stays alive for the call.
        let pointer = unsafe { (api.model_new)(path.as_ptr()) };
        if pointer.is_null() {
            return Err(format!(
                "Vosk 模型加载失败：{}（请确认模型目录完整）",
                model_dir.display()
            ));
        }
        let model = Arc::new(VoskModel {
            api: api.clone(),
            pointer,
        });
        Ok(Arc::new(Self { model, model_dir }))
    }

    pub fn wake_recognizer(&self) -> Result<VoskRecognizer, String> {
        VoskRecognizer::new(self.model.clone())
    }

    pub fn stt_recognizer(&self) -> Result<VoskRecognizer, String> {
        VoskRecognizer::new(self.model.clone())
    }

    /// Transcribe a complete 16 kHz mono utterance with the offline model.
    pub fn transcribe(&self, pcm: &[i16]) -> Result<String, String> {
        if pcm.is_empty() {
            return Err("离线识别：音频为空".to_owned());
        }
        let mut recognizer = self.stt_recognizer()?;
        for chunk in pcm.chunks(4_000) {
            recognizer.accept(chunk);
        }
        let text = recognizer.final_text();
        if text.is_empty() {
            return Err("离线识别没有结果".to_owned());
        }
        Ok(text)
    }

    pub fn description(&self) -> String {
        let name = self
            .model_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("vosk");
        format!("Vosk · {name}")
    }
}

fn find_chinese_model(config: &Config) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let data = appdata_dir();
    roots.push(data.join("models"));
    if let Some(dir) = module_dir() {
        roots.push(dir.join("models"));
        roots.push(dir.join("runtime").join("models"));
    }
    roots.push(PathBuf::from("models"));
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || !is_model_dir(&path) {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if name.contains("cn") || name.contains("zh") || name.contains("chinese") {
                return Some(path);
            }
        }
    }
    let _ = config;
    None
}

fn resolve_model_dir(config: &Config) -> Result<PathBuf, String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if !config.vosk_model_path.trim().is_empty() {
        roots.push(PathBuf::from(config.vosk_model_path.trim()));
    }
    let data = appdata_dir();
    roots.push(data.join("models"));
    if let Some(dir) = module_dir() {
        roots.push(dir.join("models"));
        roots.push(dir.join("runtime").join("models"));
    }
    roots.push(PathBuf::from("models"));

    let ascii_wake = config.wake_word.is_ascii();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in &roots {
        if !root.exists() {
            continue;
        }
        if is_model_dir(root) {
            candidates.push(root.clone());
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && is_model_dir(&path) {
                candidates.push(path);
            }
        }
    }
    candidates.sort_by_key(|path| language_score(path, ascii_wake));
    candidates.dedup();
    candidates.into_iter().next().ok_or_else(|| {
        "未找到 Vosk 模型。请运行 scripts/fetch-vosk.ps1，或在配置面板中填写模型目录。".to_owned()
    })
}

fn is_model_dir(path: &Path) -> bool {
    path.join("am").join("final.mdl").is_file()
        || path.join("conf").join("model.conf").is_file()
        || path.join("graph").join("HCLr.fst").is_file()
}

fn language_score(path: &Path, ascii_wake: bool) -> i32 {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let english = name.contains("en-us") || name.contains("en_us") || name.contains("english");
    let chinese = name.contains("cn") || name.contains("zh") || name.contains("chinese");
    if ascii_wake {
        if english {
            0
        } else if chinese {
            2
        } else {
            1
        }
    } else if chinese {
        0
    } else if english {
        2
    } else {
        1
    }
}
