//! Writes an ICO file into a Windows executable as RT_ICON/RT_GROUP_ICON resources.

use std::error::Error;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW,
};
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
use windows::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_ICON};

struct IconFrame {
    directory_fields: [u8; 8],
    data: Vec<u8>,
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn parse_ico(bytes: &[u8]) -> Result<Vec<IconFrame>, Box<dyn Error>> {
    if read_u16(bytes, 0) != Some(0) || read_u16(bytes, 2) != Some(1) {
        return Err("not a Windows icon file".into());
    }
    let count = read_u16(bytes, 4).ok_or("truncated icon header")? as usize;
    if count == 0 || bytes.len() < 6 + count * 16 {
        return Err("icon has no complete directory entries".into());
    }

    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let entry = 6 + index * 16;
        let directory_fields = bytes[entry..entry + 8].try_into()?;
        let size = read_u32(bytes, entry + 8).ok_or("truncated icon size")? as usize;
        let offset = read_u32(bytes, entry + 12).ok_or("truncated icon offset")? as usize;
        let end = offset
            .checked_add(size)
            .ok_or("icon frame range overflow")?;
        let data = bytes
            .get(offset..end)
            .ok_or("icon frame lies outside the file")?;
        frames.push(IconFrame {
            directory_fields,
            data: data.to_vec(),
        });
    }
    Ok(frames)
}

fn integer_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

fn group_icon_data(frames: &[IconFrame]) -> Vec<u8> {
    let mut group = Vec::with_capacity(6 + frames.len() * 14);
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    for (index, frame) in frames.iter().enumerate() {
        group.extend_from_slice(&frame.directory_fields);
        group.extend_from_slice(&(frame.data.len() as u32).to_le_bytes());
        group.extend_from_slice(&((index + 1) as u16).to_le_bytes());
    }
    group
}

fn embed_icon(executable: &Path, icon: &Path) -> Result<(), Box<dyn Error>> {
    let frames = parse_ico(&std::fs::read(icon)?)?;
    let executable_wide: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let update = unsafe { BeginUpdateResourceW(PCWSTR(executable_wide.as_ptr()), false)? };

    let result = (|| -> windows::core::Result<()> {
        for (index, frame) in frames.iter().enumerate() {
            unsafe {
                UpdateResourceW(
                    update,
                    RT_ICON,
                    integer_resource((index + 1) as u16),
                    0,
                    Some(frame.data.as_ptr().cast::<c_void>()),
                    frame.data.len() as u32,
                )?;
            }
        }
        let group = group_icon_data(&frames);
        unsafe {
            UpdateResourceW(
                update,
                RT_GROUP_ICON,
                integer_resource(1),
                0,
                Some(group.as_ptr().cast::<c_void>()),
                group.len() as u32,
            )
        }
    })();

    if let Err(error) = result {
        let _ = unsafe { EndUpdateResourceW(update, true) };
        return Err(error.into());
    }
    unsafe {
        EndUpdateResourceW(update, false)?;
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let executable = args.next().ok_or("missing executable path")?;
    let icon = args.next().ok_or("missing icon path")?;
    if args.next().is_some() {
        return Err("usage: embed_icon <executable> <icon.ico>".into());
    }
    embed_icon(Path::new(&executable), Path::new(&icon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_project_icon_and_builds_a_group_resource() {
        let frames = parse_ico(include_bytes!("../../assets/sue-paper.ico")).unwrap();
        assert_eq!(frames.len(), 7);
        assert!(frames.iter().all(|frame| !frame.data.is_empty()));
        assert_eq!(group_icon_data(&frames).len(), 6 + frames.len() * 14);
    }
}
