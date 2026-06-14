// プロシージャル生成テクスチャアトラス(外部アセット不要)
// 8x8タイル、各16x16px → 128x128

use crate::noise::{hash01, hash_u32};
use macroquad::prelude::*;

pub const ATLAS_TILES: u32 = 8;
pub const TILE_PX: u32 = 16;
pub const ATLAS_PX: u32 = ATLAS_TILES * TILE_PX;

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba(r, g, b, a)
}

/// ピクセル単位の決定的ノイズ [0,1)
fn px_noise(x: u32, y: u32, salt: u32) -> f32 {
    hash01(x as i32, y as i32, salt)
}

/// 基本色に±variで明度ゆらぎを加える
fn speckle(base: (u8, u8, u8), vari: i32, x: u32, y: u32, salt: u32) -> Color {
    let n = (px_noise(x, y, salt) * 2.0 - 1.0) * vari as f32;
    let c = |v: u8| (v as f32 + n).clamp(0.0, 255.0) as u8;
    rgba(c(base.0), c(base.1), c(base.2), 255)
}

fn grass_top(x: u32, y: u32) -> Color {
    let n = px_noise(x, y, 11);
    if n > 0.88 {
        rgba(140, 200, 90, 255)
    } else if n > 0.5 {
        rgba(106, 170, 70, 255)
    } else {
        rgba(92, 152, 60, 255)
    }
}

fn dirt(x: u32, y: u32, salt: u32) -> Color {
    let n = px_noise(x, y, salt);
    if n > 0.85 {
        rgba(155, 118, 83, 255)
    } else if n > 0.4 {
        rgba(134, 96, 67, 255)
    } else {
        rgba(115, 82, 58, 255)
    }
}

fn grass_side(x: u32, y: u32) -> Color {
    // 上端に草、ぎざぎざの境界
    let edge = 3 + (px_noise(x, 0, 23) * 2.0) as u32;
    if y < edge {
        grass_top(x, y)
    } else {
        dirt(x, y, 12)
    }
}

fn stone(x: u32, y: u32) -> Color {
    // 粗いまだら + 細かいノイズ
    let blotch = px_noise(x / 3, y / 3, 31) * 18.0 - 9.0;
    let n = (px_noise(x, y, 32) * 2.0 - 1.0) * 10.0 + blotch;
    let v = (127.0 + n).clamp(0.0, 255.0) as u8;
    rgba(v, v, (v as f32 * 1.02).min(255.0) as u8, 255)
}

fn sand(x: u32, y: u32) -> Color {
    speckle((218, 205, 158), 12, x, y, 41)
}

fn log_side(x: u32, y: u32) -> Color {
    // 縦縞の樹皮
    let col = px_noise(x, 0, 51);
    let n = px_noise(x, y, 52);
    if n > 0.92 {
        rgba(70, 52, 32, 255)
    } else if col > 0.5 {
        rgba(109, 84, 50, 255)
    } else {
        rgba(88, 66, 40, 255)
    }
}

fn log_top(x: u32, y: u32) -> Color {
    // 年輪(同心の正方形)
    let dx = (x as f32 - 7.5).abs();
    let dy = (y as f32 - 7.5).abs();
    let d = dx.max(dy) as u32;
    if d >= 7 {
        log_side(x, y)
    } else if d.is_multiple_of(2) {
        speckle((186, 152, 98), 8, x, y, 53)
    } else {
        speckle((158, 124, 76), 8, x, y, 54)
    }
}

fn leaves(x: u32, y: u32) -> Color {
    let n = px_noise(x, y, 61);
    if n < 0.2 {
        rgba(0, 0, 0, 0) // 透かし穴
    } else if n > 0.85 {
        rgba(72, 130, 48, 255)
    } else if n > 0.5 {
        rgba(54, 110, 40, 255)
    } else {
        rgba(42, 92, 32, 255)
    }
}

fn water(x: u32, y: u32) -> Color {
    // 横方向の波筋
    let wave = px_noise((x + y * 5) / 4, y, 71);
    if wave > 0.8 {
        rgba(92, 148, 224, 255)
    } else {
        speckle((52, 108, 198), 8, x, y, 72)
    }
}

fn snow_top(x: u32, y: u32) -> Color {
    speckle((238, 244, 250), 6, x, y, 81)
}

fn snow_side(x: u32, y: u32) -> Color {
    let edge = 4 + (px_noise(x, 0, 82) * 2.0) as u32;
    if y < edge {
        snow_top(x, y)
    } else {
        dirt(x, y, 83)
    }
}

fn plank(x: u32, y: u32) -> Color {
    // 横板 + 板ごとの継ぎ目
    let row = y / 4;
    let seam_y = y % 4 == 3;
    let joint = {
        let off = hash_u32(row.wrapping_mul(977)) % 16;
        (x + off).is_multiple_of(8)
    };
    if seam_y || joint {
        rgba(126, 96, 58, 255)
    } else {
        let grain = (px_noise(x, y, 91) * 14.0 - 7.0) as i32;
        let c = |v: i32| (v + grain).clamp(0, 255) as u8;
        rgba(c(178), c(140), c(88), 255)
    }
}

fn cobble(x: u32, y: u32) -> Color {
    // 丸石: 4x4セルの石 + 目地
    let jx = (px_noise(x / 4, y / 4, 101) * 2.0) as u32;
    let mortar = (x + jx).is_multiple_of(4) || (y + jx).is_multiple_of(4);
    if mortar {
        rgba(86, 86, 88, 255)
    } else {
        let cell = px_noise(x / 4, y / 4, 102) * 30.0 - 15.0;
        let n = (px_noise(x, y, 103) * 2.0 - 1.0) * 8.0 + cell;
        let v = (138.0 + n).clamp(0.0, 255.0) as u8;
        rgba(v, v, v, 255)
    }
}

fn glass(x: u32, y: u32) -> Color {
    let frame = x == 0 || y == 0 || x == 15 || y == 15;
    if frame {
        rgba(200, 228, 235, 255)
    } else if x + y >= 8 && x + y <= 10 && x < 9 {
        rgba(228, 244, 248, 120) // 斜めのハイライト
    } else {
        rgba(0, 0, 0, 0)
    }
}

fn coal(x: u32, y: u32) -> Color {
    // 石炭鉱石: 石ベース + 黒い塊
    let blob = px_noise(x / 2, y / 2, 111);
    if blob > 0.82 {
        let n = (px_noise(x, y, 112) * 16.0) as u8;
        rgba(30 + n, 30 + n, 32 + n, 255)
    } else {
        stone(x, y)
    }
}

fn torch(x: u32, y: u32) -> Color {
    // 中央の柄 + 先端の炎(まわりは透明)
    if (7..=8).contains(&x) && (6..16).contains(&y) {
        let n = (px_noise(x, y, 121) * 16.0 - 8.0) as i32;
        let c = |v: i32| (v + n).clamp(0, 255) as u8;
        rgba(c(120), c(92), c(56), 255)
    } else if (7..=8).contains(&x) && (2..=3).contains(&y) {
        rgba(255, 236, 140, 255) // 炎の芯
    } else if (6..=9).contains(&x) && (3..=5).contains(&y) {
        rgba(244, 160, 54, 255) // 炎の外側
    } else {
        rgba(0, 0, 0, 0)
    }
}

fn tall_grass(x: u32, y: u32) -> Color {
    // 高さの違う草の葉が縦に並ぶ(列ごとに有無と高さを決める)
    if px_noise(x, 1, 132) < 0.32 {
        return rgba(0, 0, 0, 0);
    }
    let top = 4 + (px_noise(x, 0, 131) * 8.0) as u32;
    if y < top {
        return rgba(0, 0, 0, 0);
    }
    let n = px_noise(x, y, 133);
    if n > 0.6 {
        rgba(96, 160, 66, 255)
    } else {
        rgba(74, 134, 52, 255)
    }
}

/// 茎 + 花弁(花弁色と芯色を指定)
fn flower(x: u32, y: u32, petal: (u8, u8, u8), core: (u8, u8, u8)) -> Color {
    let (cx, cy) = (7i32, 4i32);
    let dx = x as i32 - cx;
    let dy = y as i32 - cy;
    if dx.abs() <= 1 && dy.abs() <= 1 && dx.abs() + dy.abs() <= 1 {
        return rgba(core.0, core.1, core.2, 255);
    }
    if dx.abs() + dy.abs() <= 3 && dx.abs() <= 2 && dy.abs() <= 2 {
        return rgba(petal.0, petal.1, petal.2, 255);
    }
    // 茎と葉
    if x == 7 && (7..16).contains(&y) {
        return rgba(58, 118, 44, 255);
    }
    if y == 10 && (5..=6).contains(&x) {
        return rgba(74, 138, 52, 255);
    }
    rgba(0, 0, 0, 0)
}

fn flower_red(x: u32, y: u32) -> Color {
    flower(x, y, (208, 54, 46), (244, 200, 90))
}

fn flower_yellow(x: u32, y: u32) -> Color {
    flower(x, y, (236, 198, 60), (170, 120, 36))
}

type TileFn = fn(u32, u32) -> Color;

pub fn build_atlas() -> Texture2D {
    let mut img = Image::gen_image_color(ATLAS_PX as u16, ATLAS_PX as u16, BLANK);
    let tiles: [(u32, u32, TileFn); 19] = [
        (0, 0, grass_top),
        (1, 0, grass_side),
        (2, 0, |x, y| dirt(x, y, 12)),
        (3, 0, stone),
        (0, 1, sand),
        (1, 1, log_side),
        (2, 1, log_top),
        (3, 1, leaves),
        (0, 2, water),
        (1, 2, snow_top),
        (2, 2, snow_side),
        (3, 2, plank),
        (0, 3, cobble),
        (1, 3, glass),
        (2, 3, coal),
        (3, 3, torch),
        (4, 0, tall_grass),
        (5, 0, flower_red),
        (6, 0, flower_yellow),
    ];
    for (tx, ty, f) in tiles {
        for py in 0..TILE_PX {
            for px in 0..TILE_PX {
                img.set_pixel(tx * TILE_PX + px, ty * TILE_PX + py, f(px, py));
            }
        }
    }
    let tex = Texture2D::from_image(&img);
    tex.set_filter(FilterMode::Nearest);
    tex
}

/// タイル座標 → UV範囲(にじみ防止に半ピクセル内側)
pub fn tile_uv(tile: (u32, u32)) -> (f32, f32, f32, f32) {
    let s = TILE_PX as f32 / ATLAS_PX as f32;
    let half = 0.5 / ATLAS_PX as f32;
    let u0 = tile.0 as f32 * s + half;
    let v0 = tile.1 as f32 * s + half;
    (u0, v0, u0 + s - 2.0 * half, v0 + s - 2.0 * half)
}
