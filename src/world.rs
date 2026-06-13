// チャンク管理・地形生成・ブロック操作・レイキャスト

use crate::blocks::Block;
use crate::noise::{fbm, hash01, hash01_3d};
use macroquad::prelude::*;
use std::collections::HashMap;

pub const CS: i32 = 16; // チャンク水平サイズ
pub const CH: i32 = 96; // ワールド高さ
pub const SEA: i32 = 36; // 海面

pub struct Chunk {
    pub blocks: Vec<u8>,          // CS*CH*CS
    pub heights: [i16; 256],      // 列ごとの最上段ソリッド(陰影用)
    pub dirty: bool,
    pub meshes: Vec<Mesh>,        // 不透明
    pub water_meshes: Vec<Mesh>,  // 半透明
}

#[inline]
fn idx(lx: i32, y: i32, lz: i32) -> usize {
    ((y * CS + lz) * CS + lx) as usize
}

impl Chunk {
    pub fn get(&self, lx: i32, y: i32, lz: i32) -> Block {
        Block::from_u8(self.blocks[idx(lx, y, lz)])
    }

    pub fn height(&self, lx: i32, lz: i32) -> i32 {
        self.heights[(lz * CS + lx) as usize] as i32
    }
}

pub struct World {
    pub chunks: HashMap<(i32, i32), Chunk>,
    pub seed: u32,
}

/// 列の地表情報: (地表の高さ, 気温 0..1)
pub fn surface(seed: u32, x: i32, z: i32) -> (i32, f32) {
    let xf = x as f32;
    let zf = z as f32;
    let cont = fbm(xf * 0.0022, zf * 0.0022, 4, seed);
    let hills = fbm(xf * 0.011 + 31.7, zf * 0.011, 4, seed ^ 0x1234);
    let mfac = (fbm(xf * 0.006 + 100.3, zf * 0.006 - 47.9, 4, seed ^ 0xBEEF) * 0.5 + 0.5)
        .clamp(0.0, 1.0);
    let h = 34.0 + cont * 20.0 + hills * 9.0 + mfac.powi(3) * 38.0;
    let temp = (fbm(xf * 0.0035 - 200.0, zf * 0.0035 + 88.0, 3, seed ^ 0x77AA) * 0.5 + 0.5)
        .clamp(0.0, 1.0);
    (h.clamp(4.0, (CH - 8) as f32) as i32, temp)
}

/// 木が生えるか。生えるなら幹の高さを返す
fn tree_at(seed: u32, x: i32, z: i32, h: i32, temp: f32) -> Option<i32> {
    if h <= SEA + 1 || h >= 62 || !(0.28..=0.72).contains(&temp) {
        return None;
    }
    let forest = fbm(x as f32 * 0.013 + 55.3, z as f32 * 0.013 - 9.1, 2, seed ^ 0x51F0) > 0.18;
    let p = if forest { 0.034 } else { 0.004 };
    if hash01(x, z, seed ^ 0x7E57) < p {
        Some(4 + (hash01(x, z, seed ^ 0x111) * 3.0) as i32)
    } else {
        None
    }
}

fn top_block(h: i32, temp: f32) -> Block {
    // 水際または高温(砂漠)は砂
    if h <= SEA + 1 || temp > 0.72 {
        Block::Sand
    } else if temp < 0.26 || h >= 68 {
        Block::SnowGrass
    } else {
        Block::Grass
    }
}

pub fn generate_chunk(seed: u32, cx: i32, cz: i32) -> Chunk {
    let mut blocks = vec![0u8; (CS * CH * CS) as usize];
    let mut heights = [0i16; 256];

    for lz in 0..CS {
        for lx in 0..CS {
            let wx = cx * CS + lx;
            let wz = cz * CS + lz;
            let (h, temp) = surface(seed, wx, wz);
            let top = top_block(h, temp);
            let sub = if top == Block::Sand { Block::Sand } else { Block::Dirt };
            for y in 0..CH {
                let b = if y == 0 {
                    Block::Stone
                } else if y < h - 3 {
                    if hash01_3d(wx, y, wz, seed ^ 0xC0A1) < 0.02 {
                        Block::Coal
                    } else {
                        Block::Stone
                    }
                } else if y < h {
                    sub
                } else if y == h {
                    top
                } else if y <= SEA {
                    Block::Water
                } else {
                    Block::Air
                };
                blocks[idx(lx, y, lz)] = b as u8;
            }
            heights[(lz * CS + lx) as usize] = h as i16;
        }
    }

    // 木: 樹冠がチャンク境界をまたぐため、外周2ブロックの列も走査する
    for tz in -2..CS + 2 {
        for tx in -2..CS + 2 {
            let wx = cx * CS + tx;
            let wz = cz * CS + tz;
            let (h, temp) = surface(seed, wx, wz);
            let Some(th) = tree_at(seed, wx, wz, h, temp) else {
                continue;
            };
            let mut put = |dx: i32, y: i32, dz: i32, b: Block, only_air: bool| {
                let lx = tx + dx;
                let lz = tz + dz;
                if !(0..CS).contains(&lx) || !(0..CS).contains(&lz) || !(0..CH).contains(&y) {
                    return;
                }
                let i = idx(lx, y, lz);
                if only_air && blocks[i] != Block::Air as u8 {
                    return;
                }
                blocks[i] = b as u8;
                // 陰影用の高さは不透明ブロックのみ数える(葉の下を暗くしない)
                if b.is_opaque() {
                    let hi = (lz * CS + lx) as usize;
                    if y as i16 > heights[hi] {
                        heights[hi] = y as i16;
                    }
                }
            };
            // 幹と根元
            put(0, h, 0, Block::Dirt, false);
            for y in h + 1..=h + th {
                put(0, y, 0, Block::Log, false);
            }
            // 樹冠
            for (ly, r) in [(th - 1, 2i32), (th, 2), (th + 1, 1), (th + 2, 1)] {
                for dz in -r..=r {
                    for dx in -r..=r {
                        if dx == 0 && dz == 0 && ly <= th {
                            continue; // 幹の位置
                        }
                        // 角を間引いて丸く見せる
                        if dx.abs() == r && dz.abs() == r {
                            if r == 2 && hash01(wx + dx, wz + dz * 7 + ly, seed ^ 0x3A3) < 0.6 {
                                continue;
                            }
                            if r == 1 && ly == th + 2 {
                                continue;
                            }
                        }
                        put(dx, h + ly + 1, dz, Block::Leaves, true);
                    }
                }
            }
        }
    }

    Chunk {
        blocks,
        heights,
        dirty: true,
        meshes: Vec::new(),
        water_meshes: Vec::new(),
    }
}

impl World {
    pub fn new(seed: u32) -> World {
        World {
            chunks: HashMap::new(),
            seed,
        }
    }

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> Block {
        if y < 0 {
            return Block::Stone;
        }
        if y >= CH {
            return Block::Air;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        match self.chunks.get(&key) {
            Some(c) => c.get(x.rem_euclid(CS), y, z.rem_euclid(CS)),
            None => Block::Air,
        }
    }

    pub fn set_block(&mut self, x: i32, y: i32, z: i32, b: Block) {
        if !(0..CH).contains(&y) {
            return;
        }
        let (cx, cz) = (x.div_euclid(CS), z.div_euclid(CS));
        let (lx, lz) = (x.rem_euclid(CS), z.rem_euclid(CS));
        let Some(c) = self.chunks.get_mut(&(cx, cz)) else {
            return;
        };
        c.blocks[idx(lx, y, lz)] = b as u8;

        // 陰影用の列高さを更新(不透明ブロックのみ対象)
        let hi = (lz * CS + lx) as usize;
        let h = c.heights[hi] as i32;
        if b.is_opaque() && y > h {
            c.heights[hi] = y as i16;
        } else if !b.is_opaque() && y == h {
            let mut ny = y - 1;
            while ny > 0 && !c.get(lx, ny, lz).is_opaque() {
                ny -= 1;
            }
            c.heights[hi] = ny as i16;
        }

        // 自チャンクと、境界に接する場合は隣接チャンクも再メッシュ対象に
        for dz in -1..=1 {
            for dx in -1..=1 {
                let touch_x = dx == 0 || (dx == -1 && lx == 0) || (dx == 1 && lx == CS - 1);
                let touch_z = dz == 0 || (dz == -1 && lz == 0) || (dz == 1 && lz == CS - 1);
                if touch_x && touch_z {
                    if let Some(n) = self.chunks.get_mut(&(cx + dx, cz + dz)) {
                        n.dirty = true;
                    }
                }
            }
        }
    }

    /// 列の最上段ソリイドの高さ(チャンク未生成なら生成ノイズから推定)
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        match self.chunks.get(&key) {
            Some(c) => c.height(x.rem_euclid(CS), z.rem_euclid(CS)),
            None => surface(self.seed, x, z).0,
        }
    }

    /// ボクセルDDA。ヒットしたブロック座標と面法線を返す
    pub fn raycast(&self, o: Vec3, dir: Vec3, max_t: f32) -> Option<(IVec3, IVec3)> {
        let mut cell = [
            o.x.floor() as i32,
            o.y.floor() as i32,
            o.z.floor() as i32,
        ];
        let d = [dir.x, dir.y, dir.z];
        let mut step = [0i32; 3];
        let mut t_delta = [f32::INFINITY; 3];
        let mut t_max = [f32::INFINITY; 3];
        let ov = [o.x, o.y, o.z];
        for a in 0..3 {
            if d[a] > 1e-8 {
                step[a] = 1;
                t_delta[a] = 1.0 / d[a];
                t_max[a] = ((cell[a] + 1) as f32 - ov[a]) / d[a];
            } else if d[a] < -1e-8 {
                step[a] = -1;
                t_delta[a] = -1.0 / d[a];
                t_max[a] = (cell[a] as f32 - ov[a]) / d[a];
            }
        }
        let mut normal = IVec3::ZERO;
        for _ in 0..256 {
            let b = self.get_block(cell[0], cell[1], cell[2]);
            if b != Block::Air && b != Block::Water {
                return Some((IVec3::new(cell[0], cell[1], cell[2]), normal));
            }
            let a = if t_max[0] < t_max[1] && t_max[0] < t_max[2] {
                0
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            if t_max[a] > max_t {
                return None;
            }
            cell[a] += step[a];
            t_max[a] += t_delta[a];
            normal = IVec3::ZERO;
            normal[a] = -step[a];
        }
        None
    }
}
