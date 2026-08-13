//! 程序化纸张纹理生成：多 octave 值噪声（视觉效果等同 feTurbulence 分形噪声）。
//! 输出 512x512 无缝平铺块，BGRA 预乘 alpha，供 UpdateLayeredWindow 直接使用。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureKind {
    ClassicMatte,
    WhisperWeave,
    Parchment,
    VellumMist,
}

pub const KINDS: [TextureKind; 4] = [
    TextureKind::ClassicMatte,
    TextureKind::WhisperWeave,
    TextureKind::Parchment,
    TextureKind::VellumMist,
];

pub const KIND_NAMES: [&str; 4] = [
    "Classic Matte 经典哑光",
    "Whisper Weave 织物纹理",
    "Sunbaked Parchment 羊皮纸",
    "Vellum Mist 薄纱雾面",
];

/// 平铺块边长（像素）
pub const TILE: usize = 512;

/// 确定性晶格哈希 -> [0, 1)
fn hash(seed: u32, x: u32, y: u32) -> f32 {
    let mut h = seed ^ x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// 可平铺值噪声：晶格在 (wx, wy) 上取模环绕，坐标 fx∈[0,wx), fy∈[0,wy)
fn noise2(seed: u32, wx: u32, wy: u32, fx: f32, fy: f32) -> f32 {
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = smooth(fx - x0);
    let ty = smooth(fy - y0);
    let x0 = (x0 as u32) % wx;
    let y0 = (y0 as u32) % wy;
    let x1 = (x0 + 1) % wx;
    let y1 = (y0 + 1) % wy;
    let a = hash(seed, x0, y0);
    let b = hash(seed, x1, y0);
    let c = hash(seed, x0, y1);
    let d = hash(seed, x1, y1);
    a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty
}

/// 分形叠加。aspect 为 (水平, 垂直) 方向的基础晶格倍数比（整数以保证无缝平铺），
/// 用于织物等各向异性纹理。
fn fractal(seed: u32, base: u32, octaves: u32, aspect: (u32, u32), x: usize, y: usize) -> f32 {
    let mut sum = 0.0f32;
    let mut amp = 1.0f32;
    let mut norm = 0.0f32;
    let mut wx = base * aspect.0;
    let mut wy = base * aspect.1;
    for o in 0..octaves {
        let fx = x as f32 / TILE as f32 * wx as f32;
        let fy = y as f32 / TILE as f32 * wy as f32;
        sum += amp * noise2(seed.wrapping_add(o.wrapping_mul(7919)), wx, wy, fx, fy);
        norm += amp;
        amp *= 0.5;
        wx *= 2;
        wy *= 2;
    }
    sum / norm
}

/// 生成 512x512 BGRA 预乘 alpha 平铺块。
/// intensity: 目标平均不透明度，百分比（15..30）
///
/// 纸质纹理设计（参考读书软件的纸张模拟）：
/// - 颜色噪声而非透明度噪声：每像素颜色围绕中灰双向波动（暗纤维/亮间隙），
///   白底上显现深色纤维斑，黑底上显现浅斑，灰底上双向 —— 这是"纸感"的来源；
///   若像旧版那样"固定浅色 + alpha 波动"，只能整体抬亮，屏幕就成了灰雾。
/// - 颗粒特征为 2–4px 软边团块（值噪声），1px 白噪声在 100% 缩放下不可分辨，
///   只会积分成灰雾。
/// - 平均效果仅是轻微压白抬黑（对比度衰减），均值接近中灰，不产生色偏。
pub fn generate_tile(kind: TextureKind, intensity: u32) -> Vec<u8> {
    let mut out = vec![0u8; TILE * TILE * 4];
    let base = intensity as f32 / 100.0;

    for y in 0..TILE {
        for x in 0..TILE {
            // (噪声值∈[0,1], 平均色 RGB, 颜色波动幅度)
            let (n, mean_rgb, spread): (f32, (f32, f32, f32), f32) = match kind {
                // 经典哑光：2.5px 纤维团块为主 + 8px 中层 + 微弱低频底子
                TextureKind::ClassicMatte => {
                    let n =
                        0.55 * noise2(
                            11,
                            200,
                            200,
                            x as f32 / TILE as f32 * 200.0,
                            y as f32 / TILE as f32 * 200.0,
                        ) + 0.25
                            * noise2(
                                12,
                                64,
                                64,
                                x as f32 / TILE as f32 * 64.0,
                                y as f32 / TILE as f32 * 64.0,
                            )
                            + 0.20 * fractal(13, 8, 3, (1, 1), x, y);
                    (n, (132.0, 130.0, 126.0), 0.50)
                }
                // 织物：双向拉伸纤维（经纬编织）
                TextureKind::WhisperWeave => {
                    let n =
                        0.45 * noise2(
                            23,
                            80,
                            240,
                            x as f32 / TILE as f32 * 80.0,
                            y as f32 / TILE as f32 * 240.0,
                        ) + 0.45
                            * noise2(
                                29,
                                240,
                                80,
                                x as f32 / TILE as f32 * 240.0,
                                y as f32 / TILE as f32 * 80.0,
                            )
                            + 0.10 * fractal(31, 8, 2, (1, 1), x, y);
                    (n, (130.0, 128.0, 124.0), 0.50)
                }
                // 羊皮纸：4px 团块 + 强中频斑驳，暖琥珀色调
                TextureKind::Parchment => {
                    let n =
                        0.35 * noise2(
                            41,
                            128,
                            128,
                            x as f32 / TILE as f32 * 128.0,
                            y as f32 / TILE as f32 * 128.0,
                        ) + 0.40 * fractal(43, 4, 4, (1, 1), x, y)
                            + 0.25 * fractal(47, 16, 3, (1, 1), x, y);
                    (n, (150.0, 118.0, 72.0), 0.60)
                }
                // 薄纱雾面：低频柔和为主 + 一层薄团块
                TextureKind::VellumMist => {
                    let n =
                        0.25 * noise2(
                            53,
                            128,
                            128,
                            x as f32 / TILE as f32 * 128.0,
                            y as f32 / TILE as f32 * 128.0,
                        ) + 0.75 * fractal(59, 4, 3, (1, 1), x, y);
                    (n, (140.0, 140.0, 140.0), 0.35)
                }
            };

            // alpha：均值 = intensity，叠加轻微密度起伏（纸面厚薄不均）
            let density = 0.85 + 0.30 * fractal(67, 8, 2, (1, 1), x, y);
            let a = (base * density).clamp(0.0, 1.0);

            // 颜色围绕 mean 双向波动：n=0.5 时等于 mean，两端 ±spread
            let dev = spread * (2.0 * n - 1.0);
            let r = (mean_rgb.0 * (1.0 + dev)).clamp(0.0, 255.0);
            let g = (mean_rgb.1 * (1.0 + dev)).clamp(0.0, 255.0);
            let b = (mean_rgb.2 * (1.0 + dev)).clamp(0.0, 255.0);

            let i = (y * TILE + x) * 4;
            out[i] = (b * a) as u8; // B（预乘）
            out[i + 1] = (g * a) as u8; // G
            out[i + 2] = (r * a) as u8; // R
            out[i + 3] = (a * 255.0) as u8; // A
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tiles_are_deterministic_and_premultiplied() {
        for kind in KINDS {
            let first = generate_tile(kind, 20);
            let second = generate_tile(kind, 20);
            assert_eq!(first, second);
            assert_eq!(first.len(), TILE * TILE * 4);
            for pixel in first.chunks_exact(4) {
                let alpha = pixel[3];
                assert!(pixel[0] <= alpha);
                assert!(pixel[1] <= alpha);
                assert!(pixel[2] <= alpha);
            }
        }
    }

    #[test]
    fn excessive_intensity_is_safely_clamped() {
        let tile = generate_tile(TextureKind::ClassicMatte, u32::MAX);
        assert_eq!(tile.len(), TILE * TILE * 4);
        assert!(tile.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX));
    }
}
