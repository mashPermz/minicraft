// チャンク管理・地形生成・ブロック操作・ライト伝播・レイキャスト

use crate::blocks::Block;
use crate::noise::{fbm, hash01, hash01_3d, perlin3};
use macroquad::prelude::*;
use std::collections::{HashMap, VecDeque};

pub const CS: i32 = 16; // チャンク水平サイズ
pub const CH: i32 = 96; // ワールド高さ
pub const SEA: i32 = 36; // 海面

const DIRS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

pub struct Chunk {
    pub blocks: Vec<u8>,          // CS*CH*CS
    pub light: Vec<u8>,           // ブロック光(松明)0..15
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

    pub fn light_at(&self, lx: i32, y: i32, lz: i32) -> u8 {
        self.light[idx(lx, y, lz)]
    }

    pub fn height(&self, lx: i32, lz: i32) -> i32 {
        self.heights[(lz * CS + lx) as usize] as i32
    }
}

pub struct World {
    pub chunks: HashMap<(i32, i32), Chunk>,
    pub seed: u32,
    /// プレイヤーが変更したブロック(セーブ対象。生成後のチャンクに再適用する)
    pub edits: HashMap<(i32, i32, i32), u8>,
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

/// 3Dノイズによる洞窟判定。2本のノイズがともにゼロ付近になる場所が
/// トンネル状に連なる(いわゆるスパゲッティ洞窟)
pub fn cave_at(seed: u32, x: i32, y: i32, z: i32) -> bool {
    let fx = x as f32 * 0.045;
    let fy = y as f32 * 0.06; // 縦方向は周期を詰めて水平寄りの洞窟にする
    let fz = z as f32 * 0.045;
    const R2: f32 = 0.016;
    let n1 = perlin3(fx, fy, fz, seed ^ 0xCA7E);
    if n1 * n1 > R2 {
        return false;
    }
    let n2 = perlin3(fx + 137.2, fy + 71.3, fz - 53.9, seed ^ 0x70AD);
    n1 * n1 + n2 * n2 < R2
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
                let mut b = if y == 0 {
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
                // 洞窟: 地面の中だけを掘る。水没列は床から3ブロック分を残して
                // 水が空中に浮かないようにする(y=0の岩盤も残す)
                if b != Block::Air
                    && b != Block::Water
                    && y >= 1
                    && (h > SEA + 2 || y < h - 3)
                    && cave_at(seed, wx, y, wz)
                {
                    b = Block::Air;
                }
                blocks[idx(lx, y, lz)] = b as u8;
            }

            // 草花(草ブロックの上のみ。洞窟の入口で削れた列には生えない)
            if h + 1 < CH && blocks[idx(lx, h, lz)] == Block::Grass as u8 {
                let r = hash01(wx, wz, seed ^ 0xF10A);
                let plant = if r < 0.012 {
                    Some(Block::FlowerRed)
                } else if r < 0.022 {
                    Some(Block::FlowerYellow)
                } else if r < 0.105 {
                    Some(Block::TallGrass)
                } else {
                    None
                };
                if let Some(p) = plant {
                    blocks[idx(lx, h + 1, lz)] = p as u8;
                }
            }

            // 列高さ(陰影用)は洞窟を掘ったあとの最上段不透明ブロックから求める
            let mut hh = 0;
            for y in (0..CH).rev() {
                if Block::from_u8(blocks[idx(lx, y, lz)]).is_opaque() {
                    hh = y;
                    break;
                }
            }
            heights[(lz * CS + lx) as usize] = hh as i16;
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
            // 足元が洞窟で削れている場所には生やさない
            if cave_at(seed, wx, h, wz) {
                continue;
            }
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
        light: vec![0u8; blocks.len()],
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
            edits: HashMap::new(),
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
        let old = c.get(lx, y, lz);
        if old == b {
            return;
        }
        c.blocks[idx(lx, y, lz)] = b as u8;
        self.edits.insert((x, y, z), b as u8);

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

        self.mark_dirty_around(x, z);

        // --- ライティング更新 ---
        if old.emission() > 0 {
            self.flood_remove(x, y, z);
        }
        if b.emission() > 0 {
            let mut q = VecDeque::new();
            q.push_back((x, y, z, b.emission()));
            self.flood_add(q);
        } else if b.is_opaque() {
            // 光っていた空間を塞いだ
            if self.get_light(x, y, z) > 0 {
                self.flood_remove(x, y, z);
            }
        } else if old.is_opaque() {
            // 壊した穴に周囲の光を流し込む
            let mut q = VecDeque::new();
            for (dx, dy, dz) in DIRS {
                let l = self.get_light(x + dx, y + dy, z + dz);
                if l > 1 {
                    q.push_back((x + dx, y + dy, z + dz, l));
                }
            }
            self.flood_add(q);
        }

        // 上に乗っていた草花・松明は支えを失ったら壊す
        if !b.is_solid() && self.get_block(x, y + 1, z).needs_support() {
            self.set_block(x, y + 1, z, Block::Air);
        }
    }

    /// ブロック(x, *, z)の変更が影響するチャンクを再メッシュ対象にする
    fn mark_dirty_around(&mut self, x: i32, z: i32) {
        let (cx, cz) = (x.div_euclid(CS), z.div_euclid(CS));
        let (lx, lz) = (x.rem_euclid(CS), z.rem_euclid(CS));
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

    pub fn get_light(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..CH).contains(&y) {
            return 0;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        match self.chunks.get(&key) {
            Some(c) => c.light_at(x.rem_euclid(CS), y, z.rem_euclid(CS)),
            None => 0,
        }
    }

    fn set_light(&mut self, x: i32, y: i32, z: i32, v: u8) {
        if !(0..CH).contains(&y) {
            return;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        let Some(c) = self.chunks.get_mut(&key) else {
            return;
        };
        let i = idx(x.rem_euclid(CS), y, z.rem_euclid(CS));
        if c.light[i] == v {
            return;
        }
        c.light[i] = v;
        self.mark_dirty_around(x, z);
    }

    /// 光をBFSで広げる。種は (x, y, z, レベル)。既存の光が強い場所では止まる。
    /// 隣へ流すセルは push と同時に set_light で確定させる。dequeue まで光を
    /// 据え置くと未確定セルが `get_light+1 < lv` をすり抜けて何度も重複 enqueue され、
    /// 開けた空間で pop 回数が指数的に膨張する(松明の再点灯が極端に遅くなる原因)。
    fn flood_add(&mut self, mut q: VecDeque<(i32, i32, i32, u8)>) {
        while let Some((x, y, z, lv)) = q.pop_front() {
            // 未生成チャンクには伝播しない(生成時に relight_chunk で流入させる)
            if !self
                .chunks
                .contains_key(&(x.div_euclid(CS), z.div_euclid(CS)))
            {
                continue;
            }
            let cur = self.get_light(x, y, z);
            if cur > lv {
                continue;
            }
            // 種(松明・再伝播の境界セル)の光がまだ点いていなければ点ける。
            // cur == lv の再伝播の種はここを素通りして近傍へ展開する
            if cur < lv {
                self.set_light(x, y, z, lv);
            }
            if lv <= 1 {
                continue;
            }
            for (dx, dy, dz) in DIRS {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if !self.get_block(nx, ny, nz).is_opaque() && self.get_light(nx, ny, nz) + 1 < lv
                {
                    self.set_light(nx, ny, nz, lv - 1);
                    q.push_back((nx, ny, nz, lv - 1));
                }
            }
        }
    }

    /// セルの光を起点に減衰BFSで消し、境界に残った強い光源から再伝播する
    fn flood_remove(&mut self, x: i32, y: i32, z: i32) {
        let start = self.get_light(x, y, z);
        if start == 0 {
            return;
        }
        self.set_light(x, y, z, 0);
        let mut rq = VecDeque::new();
        let mut addq = VecDeque::new();
        rq.push_back((x, y, z, start));
        while let Some((px, py, pz, lv)) = rq.pop_front() {
            for (dx, dy, dz) in DIRS {
                let (nx, ny, nz) = (px + dx, py + dy, pz + dz);
                let nl = self.get_light(nx, ny, nz);
                if nl == 0 {
                    continue;
                }
                if nl < lv {
                    self.set_light(nx, ny, nz, 0);
                    rq.push_back((nx, ny, nz, nl));
                } else {
                    // この光は別の光源由来 → 再伝播の種にする
                    addq.push_back((nx, ny, nz, nl));
                }
            }
        }
        self.flood_add(addq);
    }

    /// 生成直後のチャンクに光を入れる: チャンク内の光源を点灯し、
    /// 隣接チャンクの境界から漏れてくる光を流し込む
    pub fn relight_chunk(&mut self, cx: i32, cz: i32) {
        let mut q = VecDeque::new();
        if let Some(c) = self.chunks.get(&(cx, cz)) {
            for y in 0..CH {
                for lz in 0..CS {
                    for lx in 0..CS {
                        let b = c.get(lx, y, lz);
                        if b.emission() > 0 {
                            q.push_back((cx * CS + lx, y, cz * CS + lz, b.emission()));
                        }
                    }
                }
            }
        }
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if !self.chunks.contains_key(&(cx + dx, cz + dz)) {
                continue;
            }
            for y in 0..CH {
                for t in 0..CS {
                    let (wx, wz) = match (dx, dz) {
                        (-1, 0) => (cx * CS, cz * CS + t),
                        (1, 0) => (cx * CS + CS - 1, cz * CS + t),
                        (0, -1) => (cx * CS + t, cz * CS),
                        _ => (cx * CS + t, cz * CS + CS - 1),
                    };
                    let l = self.get_light(wx + dx, y, wz + dz);
                    if l > 1 && !self.get_block(wx, y, wz).is_opaque() {
                        q.push_back((wx, y, wz, l - 1));
                    }
                }
            }
        }
        self.flood_add(q);
    }

    /// セーブ由来の編集差分を生成直後のチャンクへ適用する
    pub fn apply_edits_to(&self, c: &mut Chunk, cx: i32, cz: i32) {
        let mut touched = false;
        for (&(x, y, z), &b) in &self.edits {
            if x.div_euclid(CS) != cx || z.div_euclid(CS) != cz {
                continue;
            }
            // idx() は範囲チェックをしない。edits は通常 set_block / 検証済み parse
            // 由来で範囲内だが、万一の不正な y で配列範囲外 panic を起こさないよう弾く
            if !(0..CH).contains(&y) {
                continue;
            }
            c.blocks[idx(x.rem_euclid(CS), y, z.rem_euclid(CS))] = b;
            touched = true;
        }
        if touched {
            for lz in 0..CS {
                for lx in 0..CS {
                    let mut hh = 0;
                    for y in (0..CH).rev() {
                        if c.get(lx, y, lz).is_opaque() {
                            hh = y;
                            break;
                        }
                    }
                    c.heights[(lz * CS + lx) as usize] = hh as i16;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// y=0..=10 が石、それより上が空気の平らなチャンク
    fn flat_chunk() -> Chunk {
        let mut blocks = vec![0u8; (CS * CH * CS) as usize];
        for y in 0..=10 {
            for lz in 0..CS {
                for lx in 0..CS {
                    blocks[idx(lx, y, lz)] = Block::Stone as u8;
                }
            }
        }
        Chunk {
            light: vec![0u8; blocks.len()],
            blocks,
            heights: [10; 256],
            dirty: true,
            meshes: Vec::new(),
            water_meshes: Vec::new(),
        }
    }

    fn flat_world() -> World {
        let mut w = World::new(1);
        for cz in -1..=1 {
            for cx in -1..=1 {
                w.chunks.insert((cx, cz), flat_chunk());
            }
        }
        w
    }

    #[test]
    fn torch_light_propagates_and_removes() {
        let mut w = flat_world();
        w.set_block(8, 12, 8, Block::Torch);
        assert_eq!(w.get_light(8, 12, 8), 14);
        assert_eq!(w.get_light(12, 12, 8), 10); // マンハッタン距離4
        assert_eq!(w.get_light(10, 12, 10), 10);
        assert_eq!(w.get_light(8, 12, 21), 1); // 距離13でぎりぎり届く
        assert_eq!(w.get_light(8, 12, 22), 0);
        assert_eq!(w.get_light(8, 10, 8), 0); // 不透明な床には入らない

        // 不透明ブロックで塞ぐとそのセルは消え、周囲は回り込みで残る
        w.set_block(10, 12, 8, Block::Stone);
        assert_eq!(w.get_light(10, 12, 8), 0);
        assert_eq!(w.get_light(11, 12, 8), 9); // (11,12,7)経由

        // 松明を壊すと全て消える
        w.set_block(8, 12, 8, Block::Air);
        for d in 0..14 {
            assert_eq!(w.get_light(8, 12, 8 + d), 0);
        }
    }

    #[test]
    fn cross_chunk_relight_on_generation() {
        let mut w = flat_world();
        w.set_block(15, 12, 8, Block::Torch); // チャンク(0,0)の東端
        assert_eq!(w.get_light(18, 12, 8), 11); // 隣のチャンク(1,0)へ流入

        // チャンク(1,0)を作り直して「あとから生成された」状況を再現
        w.chunks.insert((1, 0), flat_chunk());
        assert_eq!(w.get_light(18, 12, 8), 0);
        w.relight_chunk(1, 0);
        assert_eq!(w.get_light(18, 12, 8), 11);
    }

    #[test]
    fn plant_breaks_without_support() {
        let mut w = flat_world();
        w.set_block(4, 11, 4, Block::FlowerRed);
        assert_eq!(w.get_block(4, 11, 4), Block::FlowerRed);
        w.set_block(4, 10, 4, Block::Air); // 支えを壊す
        assert_eq!(w.get_block(4, 11, 4), Block::Air);
        // 連鎖破壊も編集差分として記録される
        assert_eq!(w.edits.get(&(4, 11, 4)), Some(&(Block::Air as u8)));
    }

    #[test]
    fn edits_apply_on_generation() {
        // セーブからのロードを再現: 編集差分が生成後のチャンクに反映され、
        // 松明が再点灯する
        let mut w = World::new(7);
        w.edits.insert((5, 50, 5), Block::Cobble as u8);
        w.edits.insert((5, 51, 5), Block::Torch as u8);
        let mut c = generate_chunk(7, 0, 0);
        w.apply_edits_to(&mut c, 0, 0);
        w.chunks.insert((0, 0), c);
        w.relight_chunk(0, 0);
        assert_eq!(w.get_block(5, 50, 5), Block::Cobble);
        assert_eq!(w.get_block(5, 51, 5), Block::Torch);
        assert_eq!(w.get_light(5, 51, 5), 14);
        assert!(w.height_at(5, 5) >= 50); // 深さ陰影用の高さも更新済み
    }

    #[test]
    fn cave_density_is_sane() {
        let seed = 12345u32;
        let (mut carved, mut total) = (0u32, 0u32);
        for x in 0..96 {
            for z in 0..96 {
                for y in 5..40 {
                    total += 1;
                    if cave_at(seed, x, y, z) {
                        carved += 1;
                    }
                }
            }
        }
        let frac = carved as f32 / total as f32;
        println!("cave fraction: {frac:.4}");
        // 断面図(目視確認用): cargo test cave -- --nocapture
        for y in (8..40).rev() {
            let row: String = (0..96)
                .map(|x| if cave_at(seed, x, y, 48) { '#' } else { '.' })
                .collect();
            println!("{row}");
        }
        assert!((0.005..0.12).contains(&frac), "cave fraction {frac} out of range");
    }
}
