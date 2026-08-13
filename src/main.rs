//! Sue-Paper：Windows 屏幕纸张纹理覆盖工具
//! 核心功能包括全屏程序生成纸张纹理、鼠标键盘穿透、
//! 托盘控制、应用排除列表、打盹、多显示器。

#![windows_subsystem = "windows"]

mod exclusion;
mod monitor;
mod overlay;
mod settings;
mod texture;
mod tray;

use std::cell::RefCell;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use settings::Settings;
use texture::{generate_tile, TextureKind, KINDS};

pub const ID_TOGGLE: u16 = 1001;
pub const ID_EXIT: u16 = 1002;
pub const ID_TEX_BASE: u16 = 1100; // 1100..1103
pub const ID_INT_BASE: u16 = 1200; // 1200..1203
pub const ID_SNOOZE_5: u16 = 1301;
pub const ID_SNOOZE_15: u16 = 1302;
pub const ID_EXCLUDE: u16 = 1401;

pub const INTENSITIES: [u32; 4] = [15, 20, 25, 30];

const TIMER_POLL: usize = 1; // 前台应用轮询
const TIMER_SNOOZE: usize = 2; // 打盹结束

struct App {
    settings: Settings,
    overlays: Vec<overlay::Overlay>,
    texture_tile: Vec<u8>,
    tray_icon: Option<HICON>,
    instance_mutex: Option<HANDLE>,
    foreground_hook: Option<HWINEVENTHOOK>,
    snoozed: bool,
    hidden_by_exclusion: bool,
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App {
        settings: Settings::default(),
        overlays: Vec::new(),
        texture_tile: Vec::new(),
        tray_icon: None,
        instance_mutex: None,
        foreground_hook: None,
        snoozed: false,
        hidden_by_exclusion: false,
    });
}

fn tex_idx(kind: TextureKind) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(0)
}

fn int_idx(intensity: u32) -> usize {
    INTENSITIES
        .iter()
        .position(|v| *v == intensity)
        .unwrap_or(1)
}

fn taskbar_created_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(windows::core::w!("TaskbarCreated")) })
}

fn save_settings(settings: &Settings) {
    if let Err(error) = settings.save() {
        eprintln!("save settings failed: {error}");
    }
}

fn overlay_should_be_visible(enabled: bool, snoozed: bool, hidden_by_exclusion: bool) -> bool {
    enabled && !snoozed && !hidden_by_exclusion
}

/// 重新生成纹理并应用到所有覆盖窗口
fn refresh_textures() {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        app.texture_tile = generate_tile(app.settings.texture, app.settings.intensity);
        let App {
            overlays,
            texture_tile,
            ..
        } = &*app;
        unsafe {
            for overlay in overlays {
                if let Err(error) = overlay::apply_texture(overlay, texture_tile) {
                    eprintln!("apply overlay texture failed: {error}");
                }
            }
        }
    });
}

/// 根据 enabled / snoozed / exclusion 决定覆盖层可见性
fn apply_visibility() {
    APP.with(|app| {
        let app = app.borrow();
        let visible =
            overlay_should_be_visible(app.settings.enabled, app.snoozed, app.hidden_by_exclusion);
        unsafe {
            for o in &app.overlays {
                overlay::set_visible(o, visible);
            }
        }
    });
}

fn ensure_overlays_topmost() {
    APP.with(|app| {
        let app = app.borrow();
        if !overlay_should_be_visible(app.settings.enabled, app.snoozed, app.hidden_by_exclusion) {
            return;
        }
        for overlay in &app.overlays {
            if let Err(error) = unsafe { overlay::ensure_topmost(overlay) } {
                eprintln!("restore overlay z-order failed: {error}");
            }
        }
    });
}

/// 轮询前台应用，命中排除列表时隐藏纹理
fn poll_exclusion() {
    let fg = exclusion::foreground_process_name();
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let excluded = fg
            .as_deref()
            .map(|name| app.settings.exclusions.iter().any(|e| e == name))
            .unwrap_or(false);
        if excluded != app.hidden_by_exclusion {
            app.hidden_by_exclusion = excluded;
            drop(app);
            apply_visibility();
        }
    });
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object_id: i32,
    _child_id: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    poll_exclusion();
    ensure_overlays_topmost();
}

/// 显示器变化时重建覆盖层
fn rebuild_overlays() {
    let hinst = match unsafe { GetModuleHandleW(PCWSTR::null()) } {
        Ok(module) => HINSTANCE(module.0),
        Err(error) => {
            eprintln!("get module handle failed: {error}");
            return;
        }
    };
    match unsafe { overlay::spawn_all(hinst) } {
        Ok(mut new_overlays) => {
            let prepared = APP.with(|app| {
                let app = app.borrow();
                new_overlays.iter().try_for_each(|new_overlay| unsafe {
                    overlay::apply_texture(new_overlay, &app.texture_tile)
                })
            });
            if let Err(error) = prepared {
                unsafe { overlay::destroy_all(&mut new_overlays) };
                eprintln!("prepare replacement overlays failed: {error}");
                return;
            }
            APP.with(|app| {
                let mut app = app.borrow_mut();
                let mut old_overlays = std::mem::replace(&mut app.overlays, new_overlays);
                unsafe { overlay::destroy_all(&mut old_overlays) };
            });
            apply_visibility();
        }
        Err(error) => eprintln!("rebuild overlays failed: {error}"),
    }
}

fn handle_command(hwnd: HWND, cmd: u32, fg_name: Option<String>) {
    let cmd = cmd as u16;
    match cmd {
        ID_TOGGLE => {
            APP.with(|app| {
                let mut app = app.borrow_mut();
                app.settings.enabled = !app.settings.enabled;
                save_settings(&app.settings);
            });
            apply_visibility();
        }
        ID_EXIT => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        ID_SNOOZE_5 | ID_SNOOZE_15 => {
            let mins = if cmd == ID_SNOOZE_5 { 5 } else { 15 };
            APP.with(|app| app.borrow_mut().snoozed = true);
            apply_visibility();
            unsafe {
                if SetTimer(hwnd, TIMER_SNOOZE, mins * 60_000, None) == 0 {
                    APP.with(|app| app.borrow_mut().snoozed = false);
                    apply_visibility();
                    eprintln!(
                        "create snooze timer failed: {}",
                        windows::core::Error::from_win32()
                    );
                }
            }
        }
        ID_EXCLUDE => {
            if let Some(name) = fg_name {
                let own = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_lowercase()));
                if Some(&name) != own.as_ref() {
                    APP.with(|app| {
                        let mut app = app.borrow_mut();
                        if !app.settings.exclusions.contains(&name) {
                            app.settings.exclusions.push(name);
                            save_settings(&app.settings);
                        }
                    });
                }
            }
        }
        c if (ID_TEX_BASE..ID_TEX_BASE + KINDS.len() as u16).contains(&c) => {
            let kind = KINDS[(c - ID_TEX_BASE) as usize];
            let changed = APP.with(|app| {
                let mut app = app.borrow_mut();
                if app.settings.texture == kind {
                    return false;
                }
                app.settings.texture = kind;
                save_settings(&app.settings);
                true
            });
            if changed {
                refresh_textures();
                apply_visibility();
            }
        }
        c if (ID_INT_BASE..ID_INT_BASE + INTENSITIES.len() as u16).contains(&c) => {
            let v = INTENSITIES[(c - ID_INT_BASE) as usize];
            let changed = APP.with(|app| {
                let mut app = app.borrow_mut();
                if app.settings.intensity == v {
                    return false;
                }
                app.settings.intensity = v;
                save_settings(&app.settings);
                true
            });
            if changed {
                refresh_textures();
                apply_visibility();
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = taskbar_created_message();
    if taskbar_created != 0 && msg == taskbar_created {
        APP.with(|app| {
            if let Some(icon) = app.borrow().tray_icon {
                if let Err(error) = tray::add_icon(hwnd, icon) {
                    eprintln!("restore tray icon failed: {error}");
                }
            }
        });
        return LRESULT(0);
    }

    match msg {
        tray::WM_TRAY => {
            let event = lparam.0 as u32 & 0xffff;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                // 必须在弹菜单之前捕获前台应用（弹菜单会切前台）
                let fg = exclusion::foreground_process_name();
                let (enabled, ti, ii) = APP.with(|app| {
                    let app = app.borrow();
                    (
                        app.settings.enabled,
                        tex_idx(app.settings.texture),
                        int_idx(app.settings.intensity),
                    )
                });
                let cmd = tray::show_menu(hwnd, enabled, ti, ii, fg.as_deref());
                if cmd != 0 {
                    handle_command(hwnd, cmd, fg);
                }
            }
            LRESULT(0)
        }
        WM_TIMER => match wparam.0 {
            TIMER_POLL => {
                poll_exclusion();
                ensure_overlays_topmost();
                LRESULT(0)
            }
            TIMER_SNOOZE => {
                let _ = KillTimer(hwnd, TIMER_SNOOZE);
                APP.with(|app| app.borrow_mut().snoozed = false);
                apply_visibility();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        },
        WM_DISPLAYCHANGE => {
            rebuild_overlays();
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, TIMER_POLL);
            let _ = KillTimer(hwnd, TIMER_SNOOZE);
            tray::remove_icon(hwnd);
            APP.with(|app| {
                let mut app = app.borrow_mut();
                overlay::destroy_all(&mut app.overlays);
                if let Some(icon) = app.tray_icon.take() {
                    let _ = DestroyIcon(icon);
                }
                if let Some(mutex) = app.instance_mutex.take() {
                    let _ = CloseHandle(mutex);
                }
                if let Some(hook) = app.foreground_hook.take() {
                    let _ = UnhookWinEvent(hook);
                }
            });
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run() -> windows::core::Result<()> {
    unsafe {
        let instance_mutex = CreateMutexW(
            None,
            false,
            windows::core::w!("Local\\SuePaper.SingleInstance"),
        )?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(instance_mutex);
            return Ok(());
        }
        APP.with(|app| app.borrow_mut().instance_mutex = Some(instance_mutex));

        // 多显示器不同 DPI 时保证拿到物理像素坐标
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        APP.with(|app| app.borrow_mut().settings = Settings::load());
        let _ = taskbar_created_message();

        let hinst = HINSTANCE(GetModuleHandleW(PCWSTR::null())?.0);
        overlay::register_class(hinst)?;
        let icon = tray::create_app_icon()
            .or_else(|_| LoadIconW(HINSTANCE::default(), IDI_APPLICATION))?;
        let hwnd = match tray::create_control_window(Some(wndproc), icon, hinst) {
            Ok(hwnd) => hwnd,
            Err(error) => {
                let _ = DestroyIcon(icon);
                return Err(error);
            }
        };
        if let Err(error) = tray::add_icon(hwnd, icon) {
            let _ = DestroyWindow(hwnd);
            let _ = DestroyIcon(icon);
            return Err(error);
        }
        APP.with(|app| app.borrow_mut().tray_icon = Some(icon));

        let overlays = match overlay::spawn_all(hinst) {
            Ok(overlays) => overlays,
            Err(error) => {
                let _ = DestroyWindow(hwnd);
                return Err(error);
            }
        };
        APP.with(|app| app.borrow_mut().overlays = overlays);
        refresh_textures();
        apply_visibility();

        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            HMODULE::default(),
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let poll_interval = if hook.0.is_null() {
            500
        } else {
            APP.with(|app| app.borrow_mut().foreground_hook = Some(hook));
            2_000
        };
        if SetTimer(hwnd, TIMER_POLL, poll_interval, None) == 0 {
            let _ = DestroyWindow(hwnd);
            return Err(windows::core::Error::from_win32());
        }

        let mut msg = MSG::default();
        loop {
            let status = GetMessageW(&mut msg, HWND::default(), 0, 0).0;
            if status == -1 {
                let error = windows::core::Error::from_win32();
                let _ = DestroyWindow(hwnd);
                return Err(error);
            }
            if status == 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Sue-Paper failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::overlay_should_be_visible;

    #[test]
    fn visibility_respects_every_suppression_state() {
        assert!(overlay_should_be_visible(true, false, false));
        assert!(!overlay_should_be_visible(false, false, false));
        assert!(!overlay_should_be_visible(true, true, false));
        assert!(!overlay_should_be_visible(true, false, true));
        assert!(!overlay_should_be_visible(false, true, true));
    }
}
