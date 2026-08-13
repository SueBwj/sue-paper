//! 分层覆盖窗口：无边框、置顶、逐像素半透明、鼠标键盘完全穿透。
//!
//! 关键窗口扩展样式：
//! - WS_EX_LAYERED      逐像素 alpha 混合（配合 UpdateLayeredWindow）
//! - WS_EX_TRANSPARENT  鼠标事件穿透到下层窗口
//! - WS_EX_TOPMOST      永远置顶
//! - WS_EX_NOACTIVATE   不抢焦点
//! - WS_EX_TOOLWINDOW   不出现在任务栏 / Alt-Tab

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::monitor;
use crate::texture::TILE;

const CLASS_NAME: windows::core::PCWSTR = w!("SuePaperOverlay");

pub struct Overlay {
    pub hwnd: HWND,
    pub rect: RECT,
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub unsafe fn register_class(hinst: HINSTANCE) -> windows::core::Result<()> {
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(overlay_proc),
        hInstance: hinst,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if RegisterClassExW(&wc) == 0 {
        Err(windows::core::Error::from_win32())
    } else {
        Ok(())
    }
}

/// 为每个显示器创建一个覆盖窗口
pub unsafe fn spawn_all(hinst: HINSTANCE) -> windows::core::Result<Vec<Overlay>> {
    let mut out = Vec::new();
    for rect in monitor::monitor_rects()? {
        let ex_style =
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        match CreateWindowExW(
            ex_style,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            hinst,
            None,
        ) {
            Ok(hwnd) => out.push(Overlay { hwnd, rect }),
            Err(error) => {
                destroy_all(&mut out);
                return Err(error);
            }
        }
    }
    Ok(out)
}

/// 把 512x512 纹理平铺块铺满整个窗口，通过 UpdateLayeredWindow 一次性上屏。
/// 静态纹理只调用一次，之后 0% CPU。
pub unsafe fn apply_texture(overlay: &Overlay, tile: &[u8]) -> windows::core::Result<()> {
    if tile.len() != TILE * TILE * 4 {
        return Err(windows::core::Error::new(
            E_INVALIDARG,
            "texture tile has an invalid size",
        ));
    }
    let r = overlay.rect;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return Ok(());
    }
    let hdc_screen = GetDC(HWND::default());
    if hdc_screen.0.is_null() {
        return Err(windows::core::Error::from_win32());
    }
    let hdc_mem = CreateCompatibleDC(hdc_screen);
    if hdc_mem.0.is_null() {
        ReleaseDC(HWND::default(), hdc_screen);
        return Err(windows::core::Error::from_win32());
    }

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // 负数 = 自顶向下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbmp = match CreateDIBSection(
        hdc_screen,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits,
        HANDLE::default(),
        0,
    ) {
        Ok(h) => h,
        Err(e) => {
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(HWND::default(), hdc_screen);
            return Err(e);
        }
    };
    if bits.is_null() {
        let _ = DeleteObject(hbmp);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(HWND::default(), hdc_screen);
        return Err(windows::core::Error::from_win32());
    }

    // 平铺填充
    let (wu, hu, tu) = (w as usize, h as usize, TILE);
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, wu * hu * 4);
    for yy in 0..hu {
        let ty = yy % tu;
        let src_row = &tile[ty * tu * 4..(ty + 1) * tu * 4];
        let dst_row = &mut dst[yy * wu * 4..(yy + 1) * wu * 4];
        for chunk in dst_row.chunks_mut(src_row.len()) {
            chunk.copy_from_slice(&src_row[..chunk.len()]);
        }
    }

    let old = SelectObject(hdc_mem, hbmp);
    if old.0.is_null() || old.0 as isize == -1 {
        let _ = DeleteObject(hbmp);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(HWND::default(), hdc_screen);
        return Err(windows::core::Error::from_win32());
    }
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let size = SIZE { cx: w, cy: h };
    let dst_pos = POINT {
        x: r.left,
        y: r.top,
    };
    let src_pos = POINT { x: 0, y: 0 };

    let updated = UpdateLayeredWindow(
        overlay.hwnd,
        hdc_screen,
        Some(&dst_pos),
        Some(&size),
        hdc_mem,
        Some(&src_pos),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    SelectObject(hdc_mem, old);
    let _ = DeleteObject(hbmp);
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(HWND::default(), hdc_screen);
    updated
}

pub unsafe fn set_visible(overlay: &Overlay, visible: bool) {
    let cmd = if visible { SW_SHOWNA } else { SW_HIDE };
    let _ = ShowWindow(overlay.hwnd, cmd);
}

pub unsafe fn ensure_topmost(overlay: &Overlay) -> windows::core::Result<()> {
    SetWindowPos(
        overlay.hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
    )
}

pub unsafe fn destroy_all(overlays: &mut Vec<Overlay>) {
    for o in overlays.drain(..) {
        let _ = DestroyWindow(o.hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_malformed_texture_before_calling_gdi() {
        let overlay = Overlay {
            hwnd: HWND::default(),
            rect: RECT::default(),
        };
        assert!(unsafe { apply_texture(&overlay, &[]) }.is_err());
    }
}
