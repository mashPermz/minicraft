// 依存クレートなしの2D/3Dパーリンノイズ + fBm + 整数ハッシュ

use std::f32::consts::TAU;

pub fn hash_u32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    h
}

/// 座標とシードから [0,1) の決定的な乱数
pub fn hash01(x: i32, z: i32, seed: u32) -> f32 {
    let h = hash_u32(
        seed ^ (x as u32).wrapping_mul(0x9e37_79b1) ^ (z as u32).wrapping_mul(0x85eb_ca77),
    );
    (h >> 8) as f32 / 16_777_216.0
}

pub fn hash01_3d(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let h = hash_u32(
        seed ^ (x as u32).wrapping_mul(0x9e37_79b1)
            ^ (y as u32).wrapping_mul(0xc2b2_ae35)
            ^ (z as u32).wrapping_mul(0x85eb_ca77),
    );
    (h >> 8) as f32 / 16_777_216.0
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn grad(ix: i32, iz: i32, seed: u32) -> (f32, f32) {
    let a = hash01(ix, iz, seed) * TAU;
    (a.cos(), a.sin())
}

/// おおよそ [-1, 1] のパーリンノイズ
pub fn perlin(x: f32, z: f32, seed: u32) -> f32 {
    let x0 = x.floor();
    let z0 = z.floor();
    let xi = x0 as i32;
    let zi = z0 as i32;
    let fx = x - x0;
    let fz = z - z0;
    let dot = |gx: i32, gz: i32| -> f32 {
        let (vx, vz) = grad(xi + gx, zi + gz, seed);
        vx * (fx - gx as f32) + vz * (fz - gz as f32)
    };
    let u = fade(fx);
    let v = fade(fz);
    let a = lerp(dot(0, 0), dot(1, 0), u);
    let b = lerp(dot(0, 1), dot(1, 1), u);
    lerp(a, b, v) * 1.41
}

/// 3D勾配(立方体エッジ12方向から選択)
fn grad3(ix: i32, iy: i32, iz: i32, seed: u32) -> (f32, f32, f32) {
    const G: [(f32, f32, f32); 12] = [
        (1.0, 1.0, 0.0),
        (-1.0, 1.0, 0.0),
        (1.0, -1.0, 0.0),
        (-1.0, -1.0, 0.0),
        (1.0, 0.0, 1.0),
        (-1.0, 0.0, 1.0),
        (1.0, 0.0, -1.0),
        (-1.0, 0.0, -1.0),
        (0.0, 1.0, 1.0),
        (0.0, -1.0, 1.0),
        (0.0, 1.0, -1.0),
        (0.0, -1.0, -1.0),
    ];
    let h = hash_u32(
        seed ^ (ix as u32).wrapping_mul(0x9e37_79b1)
            ^ (iy as u32).wrapping_mul(0xc2b2_ae35)
            ^ (iz as u32).wrapping_mul(0x85eb_ca77),
    );
    G[(h % 12) as usize]
}

/// おおよそ [-1, 1] の3Dパーリンノイズ(洞窟生成用)
pub fn perlin3(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let z0 = z.floor();
    let (xi, yi, zi) = (x0 as i32, y0 as i32, z0 as i32);
    let (fx, fy, fz) = (x - x0, y - y0, z - z0);
    let dot = |gx: i32, gy: i32, gz: i32| -> f32 {
        let (vx, vy, vz) = grad3(xi + gx, yi + gy, zi + gz, seed);
        vx * (fx - gx as f32) + vy * (fy - gy as f32) + vz * (fz - gz as f32)
    };
    let u = fade(fx);
    let v = fade(fy);
    let w = fade(fz);
    let x00 = lerp(dot(0, 0, 0), dot(1, 0, 0), u);
    let x10 = lerp(dot(0, 1, 0), dot(1, 1, 0), u);
    let x01 = lerp(dot(0, 0, 1), dot(1, 0, 1), u);
    let x11 = lerp(dot(0, 1, 1), dot(1, 1, 1), u);
    lerp(lerp(x00, x10, v), lerp(x01, x11, v), w) * 1.15
}

/// fBm(オクターブ合成)、おおよそ [-1, 1]
pub fn fbm(x: f32, z: f32, octaves: u32, seed: u32) -> f32 {
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        sum += perlin(x * freq, z * freq, seed.wrapping_add(i.wrapping_mul(0x65))) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}
