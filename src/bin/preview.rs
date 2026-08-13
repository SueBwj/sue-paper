//! 纹理预览工具：生成各预设的平铺块，叠加在模拟书页（白底+文字行）上输出 BMP。
//! 用法: cargo run --release --bin preview <输出目录>

#[path = "../texture.rs"]
mod texture;

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use texture::{generate_tile, KINDS, KIND_NAMES, TILE};

fn write_bmp(path: impl AsRef<Path>, w: usize, h: usize, rgb: &[u8]) -> io::Result<()> {
    let stride = (w * 3 + 3) & !3;
    let img_size = stride * h;
    let file_size = 54 + img_size;
    let mut buf = Vec::with_capacity(file_size);
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&54u32.to_le_bytes());
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(w as i32).to_le_bytes());
    buf.extend_from_slice(&(h as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&[0; 24]);
    // bottom-up
    let mut row = vec![0u8; stride];
    for y in (0..h).rev() {
        for x in 0..w {
            let i = (y * w + x) * 3;
            row[x * 3] = rgb[i + 2]; // B
            row[x * 3 + 1] = rgb[i + 1]; // G
            row[x * 3 + 2] = rgb[i]; // R
        }
        buf.extend_from_slice(&row);
    }
    let mut file = File::create(path)?;
    file.write_all(&buf)
}

/// 模拟书页：白底 + 深灰"文字行"
fn book_page(w: usize, h: usize) -> Vec<u8> {
    let mut px = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            // 文字行：每 14px 一行，行内 6px 高的深灰段（带词间隔）
            let in_line_band = y % 14 >= 3 && y % 14 < 9;
            let in_word = (x % 37) < 31;
            let v: u8 = if in_line_band && in_word && x > 20 && x < w - 20 {
                60
            } else {
                250
            };
            let i = (y * w + x) * 3;
            px[i] = v;
            px[i + 1] = v;
            px[i + 2] = v;
        }
    }
    px
}

/// 把 BGRA 预乘纹理平铺叠加到 RGB 底图上
fn composite(base: &mut [u8], w: usize, h: usize, tile: &[u8]) {
    for y in 0..h {
        for x in 0..w {
            let t = ((y % TILE) * TILE + (x % TILE)) * 4;
            let a = tile[t + 3] as u32;
            let i = (y * w + x) * 3;
            // tile 为 BGRA 预乘，base 为 RGB：R 通道对应 tile[t+2]
            for c in 0..3 {
                let premul = tile[t + (2 - c)] as u32; // color*alpha
                base[i + c] = ((base[i + c] as u32 * (255 - a) + premul * 255) / 255) as u8;
            }
        }
    }
}

fn main() -> io::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&dir)?;
    let (w, h) = (768usize, 512usize);
    for (kind, name) in KINDS.iter().zip(KIND_NAMES.iter()) {
        let tile = generate_tile(*kind, 20);
        let mut page = book_page(w, h);
        composite(&mut page, w, h, &tile);
        let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let path = format!("{dir}/{safe}.bmp");
        write_bmp(&path, w, h, &page)?;

        // 3x 放大裁切（从文字区取 192x192，最近邻放大，便于观察颗粒）
        let (cw, ch, zoom) = (192usize, 192usize, 3usize);
        let mut crop = vec![0u8; cw * zoom * ch * zoom * 3];
        for y in 0..ch * zoom {
            for x in 0..cw * zoom {
                let si = ((40 + y / zoom) * w + (60 + x / zoom)) * 3;
                let di = (y * cw * zoom + x) * 3;
                crop[di..di + 3].copy_from_slice(&page[si..si + 3]);
            }
        }
        let zpath = format!("{dir}/{safe}_zoom.bmp");
        write_bmp(&zpath, cw * zoom, ch * zoom, &crop)?;
        println!("wrote {path} + zoom");
    }
    Ok(())
}
