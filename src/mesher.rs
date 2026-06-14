// チャンクメッシュ生成: 隣接面カリング + 頂点AO + 面方向の陰影 + 深さ陰影
// 頂点色のRに空光の明るさ、Gに松明光を焼き込み、シェーダ側で合成する

use crate::blocks::Block;
use crate::textures::tile_uv;
use crate::world::{World, CH, CS};
use macroquad::models::Vertex;
use macroquad::prelude::*;

const SNAP: i32 = CS + 2; // 周囲1ブロックを含むスナップショット幅

struct Snapshot {
    blocks: Vec<u8>,
    light: Vec<u8>,
    heights: Vec<i16>,
}

impl Snapshot {
    /// lx, lz は -1..=CS の範囲
    #[inline]
    fn get(&self, lx: i32, y: i32, lz: i32) -> Block {
        if y < 0 {
            return Block::Stone;
        }
        if y >= CH {
            return Block::Air;
        }
        Block::from_u8(self.blocks[((((lx + 1) * SNAP) + (lz + 1)) * CH + y) as usize])
    }

    /// ブロック光(松明)0..15
    #[inline]
    fn light(&self, lx: i32, y: i32, lz: i32) -> u8 {
        if !(0..CH).contains(&y) {
            return 0;
        }
        self.light[((((lx + 1) * SNAP) + (lz + 1)) * CH + y) as usize]
    }

    #[inline]
    fn height(&self, lx: i32, lz: i32) -> i32 {
        self.heights[(((lx + 1) * SNAP) + (lz + 1)) as usize] as i32
    }
}

fn snapshot(world: &World, cx: i32, cz: i32) -> Snapshot {
    let mut blocks = vec![0u8; (SNAP * SNAP * CH) as usize];
    let mut light = vec![0u8; (SNAP * SNAP * CH) as usize];
    let mut heights = vec![0i16; (SNAP * SNAP) as usize];
    for sx in -1..=CS {
        for sz in -1..=CS {
            let wx = cx * CS + sx;
            let wz = cz * CS + sz;
            let key = (wx.div_euclid(CS), wz.div_euclid(CS));
            let Some(c) = world.chunks.get(&key) else {
                continue;
            };
            let (lx, lz) = (wx.rem_euclid(CS), wz.rem_euclid(CS));
            let dst = ((((sx + 1) * SNAP) + (sz + 1)) * CH) as usize;
            for y in 0..CH {
                blocks[dst + y as usize] = c.get(lx, y, lz) as u8;
                light[dst + y as usize] = c.light_at(lx, y, lz);
            }
            heights[(((sx + 1) * SNAP) + (sz + 1)) as usize] = c.height(lx, lz) as i16;
        }
    }
    Snapshot {
        blocks,
        light,
        heights,
    }
}

// 面定義: 法線、4頂点オフセット(反時計回り)、陰影
const FACES: [([i32; 3], [[i32; 3]; 4], f32); 6] = [
    ([0, 1, 0], [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]], 1.0),
    ([0, -1, 0], [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], 0.55),
    ([1, 0, 0], [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]], 0.76),
    ([-1, 0, 0], [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], 0.76),
    ([0, 0, 1], [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], 0.88),
    ([0, 0, -1], [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]], 0.88),
];

const AO_LUT: [f32; 4] = [0.48, 0.69, 0.85, 1.0];

struct Bufs {
    v: Vec<Vertex>,
    i: Vec<u16>,
    out: Vec<Mesh>,
    atlas: Texture2D,
}

impl Bufs {
    fn new(atlas: &Texture2D) -> Bufs {
        Bufs {
            v: Vec::new(),
            i: Vec::new(),
            out: Vec::new(),
            atlas: atlas.clone(),
        }
    }

    fn quad(
        &mut self,
        corners: [Vec3; 4],
        uvs: [(f32, f32); 4],
        sky: [f32; 4],
        torch: [f32; 4],
        flip: bool,
    ) {
        // macroquadは1ドローコールあたり頂点10000/インデックス5000で黙って
        // クランプする(超過分の面が欠落する)ため、その内側で分割する
        if self.v.len() + 4 > 3200 {
            self.flush();
        }
        let base = self.v.len() as u16;
        for k in 0..4 {
            let s = (sky[k].clamp(0.0, 1.0) * 255.0) as u8;
            let t = (torch[k].clamp(0.0, 1.0) * 255.0) as u8;
            self.v.push(Vertex {
                position: corners[k],
                uv: vec2(uvs[k].0, uvs[k].1),
                color: [s, t, 0, 255],
                normal: Vec4::ZERO,
            });
        }
        let order: [u16; 6] = if flip {
            [1, 2, 3, 1, 3, 0]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        self.i.extend(order.iter().map(|o| base + o));
    }

    fn flush(&mut self) {
        if !self.v.is_empty() {
            self.out.push(Mesh {
                vertices: std::mem::take(&mut self.v),
                indices: std::mem::take(&mut self.i),
                texture: Some(self.atlas.clone()),
            });
        }
    }
}

/// AO占有判定(不透明ブロックのみ。葉同士で暗くならないように)
#[inline]
fn occ(snap: &Snapshot, x: i32, y: i32, z: i32) -> bool {
    snap.get(x, y, z).is_opaque()
}

/// 面の4頂点ぶんのAO係数と松明光(0..1)を返す。
/// 光は頂点を囲む面上の4セル(不透明セルを除く)の平均でなめらかにする
fn face_ao_light(snap: &Snapshot, lx: i32, y: i32, lz: i32, f: usize) -> ([f32; 4], [f32; 4]) {
    let (n, corners, _) = FACES[f];
    let naxis = if n[0] != 0 { 0 } else if n[1] != 0 { 1 } else { 2 };
    let (t1, t2) = match naxis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let base = [lx + n[0], y + n[1], lz + n[2]];
    let mut ao = [1.0f32; 4];
    let mut tl = [0.0f32; 4];
    for k in 0..4 {
        let c = corners[k];
        let d1 = c[t1] * 2 - 1;
        let d2 = c[t2] * 2 - 1;
        let mut p1 = base;
        p1[t1] += d1;
        let mut p2 = base;
        p2[t2] += d2;
        let mut pc = base;
        pc[t1] += d1;
        pc[t2] += d2;
        let s1 = occ(snap, p1[0], p1[1], p1[2]) as usize;
        let s2 = occ(snap, p2[0], p2[1], p2[2]) as usize;
        let sc = occ(snap, pc[0], pc[1], pc[2]) as usize;
        let level = if s1 == 1 && s2 == 1 {
            0
        } else {
            3 - (s1 + s2 + sc)
        };
        ao[k] = AO_LUT[level];

        let mut lsum = snap.light(base[0], base[1], base[2]) as f32;
        let mut lcnt = 1.0f32;
        for (p, occluded) in [(p1, s1 == 1), (p2, s2 == 1), (pc, sc == 1)] {
            if !occluded {
                lsum += snap.light(p[0], p[1], p[2]) as f32;
                lcnt += 1.0;
            }
        }
        tl[k] = lsum / lcnt / 15.0;
    }
    (ao, tl)
}

fn face_uvs(f: usize, tile: (u32, u32)) -> [(f32, f32); 4] {
    let (u0, v0, u1, v1) = tile_uv(tile);
    let (_, corners, _) = FACES[f];
    let mut uvs = [(0.0, 0.0); 4];
    for k in 0..4 {
        let c = corners[k];
        let (uf, vf) = match f {
            0 | 1 => (c[0] as f32, c[2] as f32),
            2 | 3 => (c[2] as f32, 1.0 - c[1] as f32),
            _ => (c[0] as f32, 1.0 - c[1] as f32),
        };
        uvs[k] = (u0 + (u1 - u0) * uf, v0 + (v1 - v0) * vf);
    }
    uvs
}

/// 地表からの深さによる暗さ(掘った穴や張り出しの下を暗く)。
/// 洞窟の奥はかなり暗くなるので、明るくしたければ松明を置く
fn depth_light(snap: &Snapshot, ax: i32, ay: i32, az: i32) -> f32 {
    let depth = snap.height(ax, az) - ay;
    (1.0 - 0.12 * depth.max(0) as f32).clamp(0.12, 1.0)
}

/// 不透明メッシュと水メッシュを生成
pub fn mesh_chunk(world: &World, cx: i32, cz: i32, atlas: &Texture2D) -> (Vec<Mesh>, Vec<Mesh>) {
    let snap = snapshot(world, cx, cz);
    let mut solid = Bufs::new(atlas);
    let mut water = Bufs::new(atlas);
    let wx0 = (cx * CS) as f32;
    let wz0 = (cz * CS) as f32;

    for y in 0..CH {
        for lz in 0..CS {
            for lx in 0..CS {
                let b = snap.get(lx, y, lz);
                if b == Block::Air {
                    continue;
                }
                let px = wx0 + lx as f32;
                let py = y as f32;
                let pz = wz0 + lz as f32;

                if b == Block::Water {
                    emit_water(&snap, &mut water, lx, y, lz, vec3(px, py, pz));
                    continue;
                }

                if b.is_cross() {
                    emit_cross(&snap, &mut solid, b, lx, y, lz, vec3(px, py, pz));
                    continue;
                }

                for (f, &(n, corners, shade)) in FACES.iter().enumerate() {
                    let nb = snap.get(lx + n[0], y + n[1], lz + n[2]);
                    let hidden = nb.is_opaque() || (nb == b && b.merges());
                    if hidden {
                        continue;
                    }
                    let (ao, tl) = face_ao_light(&snap, lx, y, lz, f);
                    let dl = depth_light(&snap, lx + n[0], y + n[1], lz + n[2]);
                    let uvs = face_uvs(f, b.tile(f));
                    let mut cs4 = [Vec3::ZERO; 4];
                    let mut sky = [0.0f32; 4];
                    let mut torch = [0.0f32; 4];
                    for k in 0..4 {
                        let c = corners[k];
                        cs4[k] = vec3(px + c[0] as f32, py + c[1] as f32, pz + c[2] as f32);
                        sky[k] = shade * ao[k] * dl;
                        torch[k] = tl[k] * ao[k];
                    }
                    let flip = ao[0] + ao[2] < ao[1] + ao[3];
                    solid.quad(cs4, uvs, sky, torch, flip);
                }
            }
        }
    }

    solid.flush();
    water.flush();
    (solid.out, water.out)
}

/// 松明・草花: 対角に交差する2枚の板(両面描画はカリング無効が前提)
fn emit_cross(snap: &Snapshot, bufs: &mut Bufs, b: Block, lx: i32, y: i32, lz: i32, p: Vec3) {
    let dl = depth_light(snap, lx, y, lz);
    let tl = snap.light(lx, y, lz) as f32 / 15.0;
    let (u0, v0, u1, v1) = tile_uv(b.tile(2));
    let uvs = [(u0, v1), (u1, v1), (u1, v0), (u0, v0)];
    const A: f32 = 0.15;
    const B: f32 = 0.85;
    for (z0, z1) in [(A, B), (B, A)] {
        let c = [
            vec3(p.x + A, p.y, p.z + z0),
            vec3(p.x + B, p.y, p.z + z1),
            vec3(p.x + B, p.y + 1.0, p.z + z1),
            vec3(p.x + A, p.y + 1.0, p.z + z0),
        ];
        bufs.quad(c, uvs, [dl; 4], [tl; 4], false);
    }
}

fn emit_water(snap: &Snapshot, bufs: &mut Bufs, lx: i32, y: i32, lz: i32, p: Vec3) {
    let above = snap.get(lx, y + 1, lz);
    let top_h = if above == Block::Water { 1.0 } else { 0.85 };
    let tl = snap.light(lx, y, lz) as f32 / 15.0;

    // 上面
    if above != Block::Water && !above.is_opaque() {
        let uvs = face_uvs(0, Block::Water.tile(0));
        let c = [
            vec3(p.x, p.y + top_h, p.z),
            vec3(p.x, p.y + top_h, p.z + 1.0),
            vec3(p.x + 1.0, p.y + top_h, p.z + 1.0),
            vec3(p.x + 1.0, p.y + top_h, p.z),
        ];
        bufs.quad(c, uvs, [1.0; 4], [tl; 4], false);
    }

    // 側面(水でも不透明でもない隣に対して)
    for (f, &(n, corners, shade)) in FACES.iter().enumerate().skip(2) {
        let nb = snap.get(lx + n[0], y + n[1], lz + n[2]);
        if nb == Block::Water || nb.is_opaque() {
            continue;
        }
        let uvs = face_uvs(f, Block::Water.tile(f));
        let mut cs4 = [Vec3::ZERO; 4];
        for k in 0..4 {
            let c = corners[k];
            let cy = if c[1] == 1 { top_h } else { 0.0 };
            cs4[k] = vec3(p.x + c[0] as f32, p.y + cy, p.z + c[2] as f32);
        }
        bufs.quad(cs4, uvs, [shade; 4], [tl; 4], false);
    }
}
