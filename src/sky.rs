// 空: 昼夜サイクル、スクリーン空グラデーション、太陽・月、雲レイヤー

use crate::noise::fbm;
use macroquad::models::Vertex;
use macroquad::prelude::*;
use std::f32::consts::TAU;

pub const CLOUD_Y: f32 = 100.0;
const CELL: f32 = 12.0;
const CLOUD_R: i64 = 30; // セル数の半径

pub struct DayState {
    pub sun_dir: Vec3,
    pub light: Vec3,     // 全体光(シェーダに渡す)
    pub fog_color: Vec3, // 地平線色
    pub top: Color,
    pub horizon: Color,
    pub lf: f32, // 昼の強さ 0..1
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

/// 時刻(秒)から空の状態を計算。1サイクル600秒
pub fn day_state(time: f64) -> DayState {
    let tday = ((time / 600.0) + 0.06).fract() as f32;
    let a = tday * TAU;
    let sun_dir = vec3(a.cos() * 0.9, a.sin(), 0.42).normalize();
    let lf = smoothstep(-0.06, 0.22, sun_dir.y);
    let warm = (-(sun_dir.y / 0.13) * (sun_dir.y / 0.13)).exp(); // 朝夕の赤み

    let day_top = vec3(0.34, 0.55, 0.92);
    let day_hor = vec3(0.66, 0.80, 0.94);
    let night_top = vec3(0.013, 0.02, 0.055);
    let night_hor = vec3(0.04, 0.06, 0.12);

    let mut top = mix3(night_top, day_top, lf);
    let mut hor = mix3(night_hor, day_hor, lf);
    let sunset = vec3(0.93, 0.52, 0.32);
    hor = mix3(hor, sunset, warm * 0.65);
    top = mix3(top, sunset, warm * 0.12);

    let light = mix3(vec3(0.15, 0.18, 0.30), vec3(1.0, 0.98, 0.93), lf);
    let light = light * mix3(Vec3::ONE, vec3(1.05, 0.88, 0.74), warm * 0.5);

    DayState {
        sun_dir,
        light,
        fog_color: hor,
        top: Color::new(top.x, top.y, top.z, 1.0),
        horizon: Color::new(hor.x, hor.y, hor.z, 1.0),
        lf,
    }
}

fn vcol(c: Color) -> [u8; 4] {
    [
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
        255,
    ]
}

/// 2Dの空グラデーション(視線ピッチで地平線位置を動かす)
pub fn draw_gradient(pitch: f32, fovy: f32, top: Color, horizon: Color) {
    let w = screen_width();
    let h = screen_height();
    let yh = (h * (0.5 + pitch.tan() / (2.0 * (fovy * 0.5).tan()))).clamp(-2.0 * h, 3.0 * h);
    let ground = Color::new(horizon.r * 0.5, horizon.g * 0.52, horizon.b * 0.55, 1.0);

    let v = |x: f32, y: f32, c: Color| Vertex {
        position: vec3(x, y, 0.0),
        uv: vec2(0.0, 0.0),
        color: vcol(c),
        normal: Vec4::ZERO,
    };
    let y0 = yh - 1.1 * h;
    let y2 = yh + 1.1 * h;
    let mesh = Mesh {
        vertices: vec![
            // 上空 → 地平線
            v(0.0, y0, top),
            v(w, y0, top),
            v(w, yh, horizon),
            v(0.0, yh, horizon),
            // 地平線 → 下(地面の靄)
            v(0.0, yh, horizon),
            v(w, yh, horizon),
            v(w, y2, ground),
            v(0.0, y2, ground),
        ],
        indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        texture: None,
    };
    draw_mesh(&mesh);
}

fn project(m: Mat4, p: Vec3) -> Option<(f32, f32)> {
    let clip = m * p.extend(1.0);
    if clip.w < 0.1 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.x.abs() > 2.0 || ndc.y.abs() > 2.0 {
        return None;
    }
    Some((
        (ndc.x + 1.0) * 0.5 * screen_width(),
        (1.0 - ndc.y) * 0.5 * screen_height(),
    ))
}

/// 太陽と月(地形より先に描いて隠れるようにする)
pub fn draw_celestial(cam: &Camera3D, eye: Vec3, ds: &DayState) {
    let m = cam.matrix();
    let h = screen_height();
    let warm = (-(ds.sun_dir.y / 0.13) * (ds.sun_dir.y / 0.13)).exp();

    // 太陽(マインクラフト風の四角。淡い四角ハロ+本体)
    let sun_a = (ds.sun_dir.y * 8.0 + 1.0).clamp(0.0, 1.0);
    if sun_a > 0.0 {
        if let Some((x, y)) = project(m, eye + ds.sun_dir * 600.0) {
            let c = mix3(vec3(1.0, 0.96, 0.82), vec3(1.0, 0.55, 0.28), warm);
            let s = h * 0.10;
            for (r, a) in [(1.6, 0.14), (1.0, 0.96)] {
                let half = s * r * 0.5;
                draw_rectangle(
                    x - half,
                    y - half,
                    s * r,
                    s * r,
                    Color::new(c.x, c.y, c.z, a * sun_a),
                );
            }
        }
    }

    // 月(四角)
    let moon_a = ((-ds.sun_dir.y) * 8.0 + 0.4).clamp(0.0, 1.0);
    if moon_a > 0.0 {
        if let Some((x, y)) = project(m, eye - ds.sun_dir * 600.0) {
            let s = h * 0.072;
            for (r, a) in [(1.4, 0.10), (1.0, 0.92)] {
                let half = s * r * 0.5;
                draw_rectangle(
                    x - half,
                    y - half,
                    s * r,
                    s * r,
                    Color::new(0.82, 0.86, 0.95, a * moon_a),
                );
            }
        }
    }
}

pub struct Clouds {
    pub meshes: Vec<Mesh>,
    key: (i64, i64, i64),
    seed: u32,
}

impl Clouds {
    pub fn new(seed: u32) -> Clouds {
        let mut c = Clouds {
            meshes: Vec::new(),
            key: (i64::MAX, 0, 0),
            seed,
        };
        c.update(Vec3::ZERO, 0.0);
        c
    }

    /// プレイヤー位置と時刻に応じてメッシュを更新し、描画オフセットを返す
    pub fn update(&mut self, player: Vec3, time: f32) -> Vec3 {
        let drift = time * 1.0; // 雲の流れ(ブロック/秒)
        let drift_cell = (drift / CELL).floor() as i64;
        let frac = drift - drift_cell as f32 * CELL;
        let pcx = (player.x / CELL).floor() as i64;
        let pcz = (player.z / CELL).floor() as i64;
        let key = (pcx, pcz, drift_cell);
        if key != self.key {
            self.key = key;
            self.rebuild(pcx, pcz, drift_cell);
        }
        vec3(frac, 0.0, 0.0)
    }

    fn rebuild(&mut self, pcx: i64, pcz: i64, drift_cell: i64) {
        let mut verts: Vec<Vertex> = Vec::new();
        let mut idx: Vec<u16> = Vec::new();
        self.meshes.clear();
        for dz in -CLOUD_R..=CLOUD_R {
            for dx in -CLOUD_R..=CLOUD_R {
                let gx = pcx + dx;
                let gz = pcz + dz;
                // ノイズはドリフト分ずらした格子で参照(時間で形が流れる)
                let n = fbm(
                    (gx - drift_cell) as f32 * 0.11,
                    gz as f32 * 0.11,
                    3,
                    self.seed ^ 0xC10D,
                );
                if n < 0.28 {
                    continue;
                }
                // macroquadは1ドローコールあたり頂点10000/インデックス5000で
                // 黙ってクランプするため、上限内で複数メッシュに分割する
                if verts.len() + 4 > 3200 {
                    self.meshes.push(Mesh {
                        vertices: std::mem::take(&mut verts),
                        indices: std::mem::take(&mut idx),
                        texture: None,
                    });
                }
                let x = gx as f32 * CELL;
                let z = gz as f32 * CELL;
                let base = verts.len() as u16;
                for (cx, cz) in [(0.0, 0.0), (0.0, CELL), (CELL, CELL), (CELL, 0.0)] {
                    verts.push(Vertex {
                        position: vec3(x + cx, CLOUD_Y, z + cz),
                        uv: vec2(0.0, 0.0),
                        color: [255, 255, 255, 255],
                        normal: Vec4::ZERO,
                    });
                }
                idx.extend([0, 1, 2, 0, 2, 3].iter().map(|o| base + o));
            }
        }
        if !verts.is_empty() {
            self.meshes.push(Mesh {
                vertices: verts,
                indices: idx,
                texture: None,
            });
        }
    }
}
