//! 系统托盘图标 + 右键菜单

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::texture::KIND_NAMES;
use crate::{
    ID_EXCLUDE, ID_EXIT, ID_INT_BASE, ID_SNOOZE_15, ID_SNOOZE_5, ID_TEX_BASE, ID_TOGGLE,
    INTENSITIES,
};

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_UID: u32 = 1;
const CONTROL_CLASS: PCWSTR = w!("SuePaperControl");
const ICON_SIZE: i32 = 64;
const ICON_BGRA: &[u8; ICON_SIZE as usize * ICON_SIZE as usize * 4] =
    include_bytes!("../assets/logo-s-64.bgra");

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 创建隐藏的 control 窗口（接收托盘消息、定时器、WM_DISPLAYCHANGE）
pub unsafe fn create_app_icon() -> windows::core::Result<HICON> {
    let hdc = GetDC(HWND::default());
    if hdc.0.is_null() {
        return Err(windows::core::Error::from_win32());
    }

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = ICON_SIZE;
    bmi.bmiHeader.biHeight = -ICON_SIZE;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let color = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0);
    ReleaseDC(HWND::default(), hdc);
    let color = color?;
    if bits.is_null() {
        let _ = DeleteObject(color);
        return Err(windows::core::Error::from_win32());
    }
    std::ptr::copy_nonoverlapping(ICON_BGRA.as_ptr(), bits.cast(), ICON_BGRA.len());

    let mask_bits = [0u8; ICON_SIZE as usize * ICON_SIZE as usize / 8];
    let mask = CreateBitmap(ICON_SIZE, ICON_SIZE, 1, 1, Some(mask_bits.as_ptr().cast()));
    if mask.0.is_null() {
        let _ = DeleteObject(color);
        return Err(windows::core::Error::from_win32());
    }
    let info = ICONINFO {
        fIcon: TRUE,
        hbmMask: mask,
        hbmColor: color,
        ..Default::default()
    };
    let icon = CreateIconIndirect(&info);
    let _ = DeleteObject(mask);
    let _ = DeleteObject(color);
    icon
}

pub unsafe fn create_control_window(
    wndproc: WNDPROC,
    icon: HICON,
    hinst: HINSTANCE,
) -> windows::core::Result<HWND> {
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: wndproc,
        hInstance: hinst,
        hIcon: icon,
        hIconSm: icon,
        lpszClassName: CONTROL_CLASS,
        ..Default::default()
    };
    if RegisterClassExW(&wc) == 0 {
        return Err(windows::core::Error::from_win32());
    }
    CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CONTROL_CLASS,
        w!("Sue-Paper"),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        None,
        None,
        hinst,
        None,
    )
}

pub unsafe fn add_icon(hwnd: HWND, hicon: HICON) -> windows::core::Result<()> {
    let mut tip = [0u16; 128];
    let tip_text = wide("Sue-Paper 纸张质感");
    let n = tip_text.len().min(127);
    tip[..n].copy_from_slice(&tip_text[..n]);
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: hicon,
        szTip: tip,
        ..Default::default()
    };
    Shell_NotifyIconW(NIM_ADD, &nid).ok()?;
    let version = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        Anonymous: NOTIFYICONDATAW_0 {
            uVersion: NOTIFYICON_VERSION_4,
        },
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_SETVERSION, &version);
    Ok(())
}

pub unsafe fn remove_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

fn append(menu: HMENU, id: u16, text: &str, checked: bool, grayed: bool) {
    let text = wide(text);
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if grayed {
        flags |= MF_GRAYED;
    }
    unsafe {
        let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(text.as_ptr()));
    }
}

/// 弹出托盘菜单，返回用户选择的命令 ID（0 = 未选择）。
/// fg_name 必须在弹菜单之前捕获（弹菜单会把前台切到本进程）。
pub unsafe fn show_menu(
    owner: HWND,
    enabled: bool,
    tex_idx: usize,
    int_idx: usize,
    fg_name: Option<&str>,
) -> u32 {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return 0,
    };

    append(
        menu,
        ID_TOGGLE,
        if enabled {
            "✔ 启用纸张纹理"
        } else {
            "　启用纸张纹理"
        },
        enabled,
        false,
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

    // 纹理子菜单
    if let Ok(sub) = CreatePopupMenu() {
        for (i, name) in KIND_NAMES.iter().enumerate() {
            append(sub, ID_TEX_BASE + i as u16, name, i == tex_idx, false);
        }
        let label = wide("纹理");
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(label.as_ptr()));
    }

    // 强度子菜单
    if let Ok(sub) = CreatePopupMenu() {
        for (i, v) in INTENSITIES.iter().enumerate() {
            append(
                sub,
                ID_INT_BASE + i as u16,
                &format!("{v}%"),
                i == int_idx,
                false,
            );
        }
        let label = wide("强度");
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(label.as_ptr()));
    }

    // 打盹子菜单
    if let Ok(sub) = CreatePopupMenu() {
        append(sub, ID_SNOOZE_5, "暂停 5 分钟", false, false);
        append(sub, ID_SNOOZE_15, "暂停 15 分钟", false, false);
        let label = wide("打盹");
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(label.as_ptr()));
    }

    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    match fg_name {
        Some(name) => append(
            menu,
            ID_EXCLUDE,
            &format!("排除当前应用 ({name})"),
            false,
            false,
        ),
        None => append(menu, ID_EXCLUDE, "排除当前应用", false, true),
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    append(menu, ID_EXIT, "退出", false, false);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // 必须先置前台，菜单才能在点击外部时正常关闭
    let _ = SetForegroundWindow(owner);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        0,
        owner,
        None,
    );
    let _ = PostMessageW(owner, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    cmd.0 as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_logo_creates_a_native_icon() {
        let icon = unsafe { create_app_icon() }.unwrap();
        assert!(!icon.0.is_null());
        unsafe {
            let _ = DestroyIcon(icon);
        }
    }
}
