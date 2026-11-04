//! iOS 27 style Siri wake effect.
//!
//! A click-through layered popup at the top of the primary monitor. It grows
//! out of the Dynamic Island, stays visible while listening, and shrinks back
//! when the recording ends.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, bounded};
use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassW, SM_CXSCREEN,
    SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const WIDTH: i32 = 480;
const HEIGHT: i32 = 340;
const START_CY: f32 = 30.0;
const END_CY: f32 = 192.0;
const START_R: f32 = 28.0;
const END_R: f32 = 118.0;
const ENTRANCE_SECONDS: f32 = 0.55;
const EXIT_SECONDS: f32 = 0.38;
const CLASS_NAME: &str = "IslandVoiceSiriOverlay";

enum Control {
    Show(Option<u64>),
    Hide,
    Stop,
}

pub struct SiriOverlay {
    tx: Sender<Control>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SiriOverlay {
    pub fn new() -> Self {
        let (tx, rx) = bounded::<Control>(8);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = thread::Builder::new()
            .name("island-voice-siri".to_owned())
            .spawn(move || run(rx, thread_stop))
            .ok();
        Self { tx, stop, handle }
    }

    /// Show and keep the orb until `hide()` (used while listening).
    pub fn show(&self) {
        let _ = self.tx.try_send(Control::Show(None));
    }

    /// Show for a fixed time (used while TTS is speaking).
    pub fn show_for(&self, millis: u64) {
        let _ = self.tx.try_send(Control::Show(Some(millis)));
    }

    pub fn hide(&self) {
        let _ = self.tx.try_send(Control::Hide);
    }
}

impl Drop for SiriOverlay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.try_send(Control::Stop);
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

fn run(rx: crossbeam_channel::Receiver<Control>, stop: Arc<AtomicBool>) {
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(WIDTH);
    let width = screen_w;
    let hwnd = match create_window(width) {
        Some(hwnd) => hwnd,
        None => {
            crate::util::debug_line("Siri overlay: create window failed");
            return;
        }
    };
    let screen_dc = unsafe { GetDC(std::ptr::null_mut()) };
    let mem_dc = unsafe { CreateCompatibleDC(screen_dc) };
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -HEIGHT;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = unsafe {
        CreateDIBSection(
            mem_dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        )
    };
    if dib.is_null() || bits.is_null() {
        crate::util::debug_line("Siri overlay: DIB creation failed");
        unsafe {
            DeleteDC(mem_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            DestroyWindow(hwnd);
        }
        return;
    }
    unsafe {
        SelectObject(mem_dc, dib);
    }
    let pt_dst = POINT { x: 0, y: 0 };
    let size = SIZE {
        cx: width,
        cy: HEIGHT,
    };
    let pt_src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let pixels =
        unsafe { std::slice::from_raw_parts_mut(bits as *mut u32, (width * HEIGHT) as usize) };

    let mut visible = false;
    let mut shown_at = Instant::now();
    let mut hide_at: Option<Instant> = None;
    let mut closing_at: Option<Instant> = None;

    while !stop.load(Ordering::Relaxed) {
        if !visible {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Control::Show(hold)) => {
                    visible = true;
                    shown_at = Instant::now();
                    hide_at = hold.map(|ms| Instant::now() + Duration::from_millis(ms));
                    closing_at = None;
                    unsafe {
                        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    }
                }
                Ok(Control::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Ok(Control::Hide) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
            continue;
        }

        // Drain commands without blocking the animation.
        loop {
            match rx.try_recv() {
                Ok(Control::Show(hold)) => {
                    shown_at = Instant::now();
                    hide_at = hold.map(|ms| Instant::now() + Duration::from_millis(ms));
                    closing_at = None;
                }
                Ok(Control::Hide) => {
                    if closing_at.is_none() {
                        closing_at = Some(Instant::now());
                    }
                }
                Ok(Control::Stop) | Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
            }
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }

        if closing_at.is_none() && hide_at.is_some_and(|at| Instant::now() >= at) {
            closing_at = Some(Instant::now());
        }
        let t = shown_at.elapsed().as_secs_f32();
        let closing = closing_at.map(|at| at.elapsed().as_secs_f32());
        render(pixels, width, t, closing);
        unsafe {
            UpdateLayeredWindow(
                hwnd, screen_dc, &pt_dst, &size, mem_dc, &pt_src, 0, &blend, ULW_ALPHA,
            );
        }

        if closing.is_some_and(|elapsed| elapsed >= EXIT_SECONDS) {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
            visible = false;
            closing_at = None;
            hide_at = None;
        }
        thread::sleep(Duration::from_millis(33));
    }

    unsafe {
        DeleteObject(dib);
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);
        DestroyWindow(hwnd);
    }
}

fn render(pixels: &mut [u32], width: i32, t: f32, closing: Option<f32>) {
    let w = width as usize;
    let h = HEIGHT as usize;
    let cx = w as f32 * 0.5;
    let (progress, opacity) = if let Some(elapsed) = closing {
        let q = (elapsed / EXIT_SECONDS).clamp(0.0, 1.0);
        (1.0 - ease_in_cubic(q), 1.0 - q)
    } else {
        let q = (t / ENTRANCE_SECONDS).clamp(0.0, 1.0);
        (ease_out_back(q).clamp(0.0, 1.06), (q * 3.0).clamp(0.0, 1.0))
    };
    let cy = START_CY + (END_CY - START_CY) * progress;
    let radius = START_R + (END_R - START_R) * progress;
    let pulse = 1.0 + 0.008 * (t * 1.9).sin();
    let r = radius * pulse;
    let inv_r = 1.0 / r.max(1.0);

    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let n = (dx * dx + dy * dy).sqrt() * inv_r;
            let mut color = [0.0f32; 3];
            let mut alpha = 0.0f32;

            if n <= 1.0 {
                let nx = dx * inv_r;
                let ny = dy * inv_r;

                // 主体：黑色渐变液态玻璃。
                // 顶部偏冷灰蓝、底部接近纯黑，形成柔和的竖向渐变。
                let v = ((ny + 1.0) * 0.5).clamp(0.0, 1.0);
                let grad = v.powf(0.85);
                let top = [0.115f32, 0.125, 0.165];
                let bottom = [0.006f32, 0.007, 0.012];
                color = [
                    top[0] + (bottom[0] - top[0]) * grad,
                    top[1] + (bottom[1] - top[1]) * grad,
                    top[2] + (bottom[2] - top[2]) * grad,
                ];

                // 左上大范围柔光，模拟玻璃表面的漫反射。
                let lx = nx + 0.62;
                let ly = ny + 0.62;
                let sheen = (-(lx * lx + ly * ly) * 1.15).exp() * 0.10;
                color = blend_add(color, [0.62, 0.72, 0.95], sheen);

                // 高光点：左上方更集中。
                let hx = nx + 0.46;
                let hy = ny + 0.46;
                let highlight = (-(hx * hx) * 5.2 - (hy * hy) * 8.5).exp();
                if highlight > 0.01 {
                    color = blend_add(color, [0.90, 0.95, 1.0], highlight * 0.62);
                }

                // 右侧微弱反光，增加球体体积感。
                let sx = nx - 0.55;
                let sy = ny + 0.10;
                let right = (-(sx * sx) * 7.0 - (sy * sy) * 3.0).exp() * 0.08;
                color = blend_add(color, [0.55, 0.62, 0.85], right);

                // 越靠近轮廓越暗：玻璃厚度的吸收。
                let rim_dark = smoothstep(0.70, 1.0, n) * 0.55;
                color = [
                    color[0] * (1.0 - rim_dark),
                    color[1] * (1.0 - rim_dark),
                    color[2] * (1.0 - rim_dark * 0.85),
                ];

                // 液态玻璃色散环：轮廓内一圈彩虹折射。
                let bezel = smoothstep(0.84, 0.995, n);
                if bezel > 0.001 {
                    let hue = (t * 38.0 + nx * 75.0 - ny * 55.0).rem_euclid(360.0);
                    let (hr, hg, hb) = hue_rgb(hue);
                    let lower = smoothstep(-0.75, 0.35, ny);
                    let strength = bezel * (0.16 + 0.28 * lower);
                    color = blend_add(color, [hr, hg, hb], strength);
                }

                // 极细的轮廓亮边。
                let edge = smoothstep(0.955, 0.985, n) * (1.0 - smoothstep(0.985, 1.0, n));
                if edge > 0.001 {
                    color = blend_add(color, [0.78, 0.86, 1.0], edge * 0.50);
                }

                // 底部玻璃厚度：一条柔和的亮弧。
                let caustic_y = ny - 0.62;
                let caustic = (-(caustic_y * caustic_y) * 18.0).exp()
                    * (-(nx * nx) * 2.2).exp()
                    * smoothstep(0.35, 0.75, n)
                    * 0.10;
                if caustic > 0.001 {
                    color = blend_add(color, [0.50, 0.60, 0.90], caustic);
                }

                alpha = 0.985;
                if n > 0.965 {
                    alpha *= ((1.0 - n) / 0.035).clamp(0.0, 1.0);
                }
            }

            // 外发光：非常克制的冷色光晕。
            if (1.0..1.12).contains(&n) {
                let strength = (1.0 - (n - 1.0) / 0.12).max(0.0).powi(3);
                color = blend_add(color, [0.35, 0.42, 0.65], strength * 0.10);
                alpha = (alpha + strength * 0.05).min(1.0);
            }

            alpha *= opacity;
            let a = (alpha * 255.0) as u32;
            let r = (color[0].clamp(0.0, 1.0) * alpha * 255.0) as u32;
            let g = (color[1].clamp(0.0, 1.0) * alpha * 255.0) as u32;
            let b = (color[2].clamp(0.0, 1.0) * alpha * 255.0) as u32;
            pixels[y * w + x] = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ease_out_back(p: f32) -> f32 {
    const C1: f32 = 1.701_58;
    const C3: f32 = C1 + 1.0;
    let q = p - 1.0;
    1.0 + C3 * q * q * q + C1 * q * q
}

fn ease_in_cubic(p: f32) -> f32 {
    p * p * p
}

fn blend_add(base: [f32; 3], add: [f32; 3], strength: f32) -> [f32; 3] {
    [
        (base[0] + add[0] * strength).min(1.0),
        (base[1] + add[1] * strength).min(1.0),
        (base[2] + add[2] * strength).min(1.0),
    ]
}

fn hue_rgb(hue: f32) -> (f32, f32, f32) {
    let h = (hue % 360.0 + 360.0) % 360.0 / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    }
}

fn create_window(width: i32) -> Option<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_wide = crate::util::wide(CLASS_NAME);
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinstance;
        wc.lpszClassName = class_wide.as_ptr();
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_wide.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            width,
            HEIGHT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() { None } else { Some(hwnd) }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
