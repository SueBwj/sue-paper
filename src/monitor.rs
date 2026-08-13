//! 显示器枚举

use windows::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

/// 枚举所有显示器的矩形区域（物理像素坐标，需进程 DPI aware）
pub fn monitor_rects() -> windows::core::Result<Vec<RECT>> {
    unsafe extern "system" fn callback(
        _hmon: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let rects = &mut *(data.0 as *mut Vec<RECT>);
        rects.push(*rect);
        TRUE
    }

    let mut rects: Vec<RECT> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(callback),
            LPARAM(&mut rects as *mut Vec<RECT> as isize),
        )
        .ok()?;
    }
    Ok(rects)
}
