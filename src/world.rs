// チャンク管理・地形生成・ブロック操作・ライト伝播・水流・レイキャスト

use crate::blocks::{Block, SKY_LIGHT_MAX, WATER_MAX};
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

/// 光チャンネル(松明光 / スカイライト)。BFS伝播ロジックを共通化するための切替
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chan {
    Torch,
    Sky,
}

pub struct Chunk {
    pub blocks: Vec<u8>,          // CS*CH*CS
    pub light: Vec<u8>,           // ブロック光(松明)0..15
    pub skylight: Vec<u8>,        // スカイライト 0..15
    pub water_level: Vec<u8>,     // 水レベル 0..WATER_MAX(水ブロック以外は0)
    pub heights: [i16; 256],      // 列ごとの最上段ソリッド(木生成・スポーン用)
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

    pub fn skylight_at(&self, lx: i32, y: i32, lz: i32) -> u8 {
        self.skylight[idx(lx, y, lz)]
    }

    pub fn water_level_at(&self, lx: i32, y: i32, lz: i32) -> u8 {
        self.water_level[idx(lx, y, lz)]
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
    /// 水流の更新待ちキュー(フレーム予算制で処理する)
    water_queue: VecDeque<(i32, i32, i32)>,
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

            // 列高さ(木生成・スポーン用)は洞窟を掘ったあとの最上段不透明ブロックから求める
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

    // 水源(海面下の生成時Water)はレベル最大。地形生成時点では水は隣接チャンクを
    // またいで広がっていないため、ここでは自チャンク内の生成済みWaterのみ初期化する
    let mut water_level = vec![0u8; blocks.len()];
    for (i, &b) in blocks.iter().enumerate() {
        if b == Block::Water as u8 {
            water_level[i] = WATER_MAX;
        }
    }

    Chunk {
        light: vec![0u8; blocks.len()],
        skylight: vec![0u8; blocks.len()],
        water_level,
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
            water_queue: VecDeque::new(),
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
        let i0 = idx(lx, y, lz);
        c.blocks[i0] = b as u8;
        self.edits.insert((x, y, z), b as u8);

        // 水レベル: Water以外になったら0にする(周囲からの再評価は水流キューに委ねる)
        if b != Block::Water {
            c.water_level[i0] = 0;
        }

        // 木生成・スポーン用の列高さを更新(不透明ブロックのみ対象)
        let hi = (lz * CS + lx) as usize;
        let h = c.heights[hi] as i32;
        let opacity_changed = old.is_opaque() != b.is_opaque();
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

        // --- 松明光の更新 ---
        if old.emission() > 0 {
            self.flood_remove(Chan::Torch, x, y, z);
        }
        if b.emission() > 0 {
            let mut q = VecDeque::new();
            q.push_back((x, y, z, b.emission()));
            self.flood_add(Chan::Torch, q);
        } else if b.is_opaque() {
            // 光っていた空間を塞いだ
            if self.get_light(x, y, z) > 0 {
                self.flood_remove(Chan::Torch, x, y, z);
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
            self.flood_add(Chan::Torch, q);
        }

        // --- スカイライトの更新: 遮蔽が増減した時だけ(松明と同じパターン) ---
        if opacity_changed {
            if b.is_opaque() {
                // 塞いだ: このセルの光を除去。直下の直射日光の柱は
                // flood_remove 内の「真下の同値カスケード」で一緒に消える
                if self.get_skylight(x, y, z) > 0 {
                    self.flood_remove(Chan::Sky, x, y, z);
                }
            } else {
                // 壊した: 周囲のスカイライトを流し込む。真上が直射日光(15)なら
                // flood_add の無減衰下方向伝播で柱が下まで再点灯する
                let mut q = VecDeque::new();
                if y == CH - 1 {
                    // 世界の天辺は常に空に露出している
                    q.push_back((x, y, z, SKY_LIGHT_MAX));
                }
                for (dx, dy, dz) in DIRS {
                    let l = self.get_skylight(x + dx, y + dy, z + dz);
                    if l > 1 {
                        q.push_back((x + dx, y + dy, z + dz, l));
                    }
                }
                self.flood_add(Chan::Sky, q);
            }
        }

        // --- 水流: 周囲のセルを再評価キューに積む(破壊で流入、設置で埋まる) ---
        self.enqueue_water_neighbors(x, y, z);

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
        self.get_chan(Chan::Torch, x, y, z)
    }

    pub fn get_skylight(&self, x: i32, y: i32, z: i32) -> u8 {
        self.get_chan(Chan::Sky, x, y, z)
    }

    fn get_chan(&self, chan: Chan, x: i32, y: i32, z: i32) -> u8 {
        if !(0..CH).contains(&y) {
            return 0;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        match self.chunks.get(&key) {
            Some(c) => {
                let (lx, lz) = (x.rem_euclid(CS), z.rem_euclid(CS));
                match chan {
                    Chan::Torch => c.light_at(lx, y, lz),
                    Chan::Sky => c.skylight_at(lx, y, lz),
                }
            }
            None => 0,
        }
    }

    fn set_chan(&mut self, chan: Chan, x: i32, y: i32, z: i32, v: u8) {
        if !(0..CH).contains(&y) {
            return;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        let Some(c) = self.chunks.get_mut(&key) else {
            return;
        };
        let i = idx(x.rem_euclid(CS), y, z.rem_euclid(CS));
        let slot = match chan {
            Chan::Torch => &mut c.light[i],
            Chan::Sky => &mut c.skylight[i],
        };
        if *slot == v {
            return;
        }
        *slot = v;
        self.mark_dirty_around(x, z);
    }

    /// 光をBFSで広げる。種は (x, y, z, レベル)。既存の光が強い場所では止まる。
    /// 隣へ流すセルは push と同時に set_chan で確定させる。dequeue まで光を
    /// 据え置くと未確定セルが `get+1 < lv` をすり抜けて何度も重複 enqueue され、
    /// 開けた空間で pop 回数が指数的に膨張する(松明の再点灯が極端に遅くなる原因)。
    /// スカイライトは最大値(直射日光)のときだけ真下へ減衰なしで伝わる。
    /// 注意: y範囲外のセルは set_chan が書き込めず「未確定のまま」何度でも push
    /// されて木状に爆発するため、範囲内に限定して展開する。
    fn flood_add(&mut self, chan: Chan, mut q: VecDeque<(i32, i32, i32, u8)>) {
        while let Some((x, y, z, lv)) = q.pop_front() {
            if !(0..CH).contains(&y) {
                continue;
            }
            // 未生成チャンクには伝播しない(生成時に relight_chunk で流入させる)
            if !self
                .chunks
                .contains_key(&(x.div_euclid(CS), z.div_euclid(CS)))
            {
                continue;
            }
            let cur = self.get_chan(chan, x, y, z);
            if cur > lv {
                continue;
            }
            // 種(光源・再伝播の境界セル)の光がまだ点いていなければ点ける。
            // cur == lv の再伝播の種はここを素通りして近傍へ展開する
            if cur < lv {
                self.set_chan(chan, x, y, z, lv);
            }
            if lv <= 1 {
                continue;
            }
            for (dx, dy, dz) in DIRS {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if !(0..CH).contains(&ny) {
                    continue;
                }
                if self.get_block(nx, ny, nz).is_opaque() {
                    continue;
                }
                // 直射日光(最大値)だけは真下へ減衰なしで伝わる(光の柱)
                let no_decay = chan == Chan::Sky && dy == -1 && lv == SKY_LIGHT_MAX;
                let nlv = if no_decay { lv } else { lv - 1 };
                // 「厳密に明るくなる時だけ」書き込んで push(set-at-enqueue で重複防止)
                if self.get_chan(chan, nx, ny, nz) < nlv {
                    self.set_chan(chan, nx, ny, nz, nlv);
                    q.push_back((nx, ny, nz, nlv));
                }
            }
        }
    }

    /// セルの光を起点に減衰BFSで消し、境界に残った強い光源から再伝播する。
    /// 松明: 隣の光が「厳密に弱い」ときだけこのセル由来とみなして消す
    /// (同値の隣は別光源)。スカイライトはそれに加えて「真下が同値 かつ 最大値」も
    /// このセル由来(無減衰の直射日光の柱)なのでカスケードして消す。
    fn flood_remove(&mut self, chan: Chan, x: i32, y: i32, z: i32) {
        let start = self.get_chan(chan, x, y, z);
        if start == 0 {
            return;
        }
        self.set_chan(chan, x, y, z, 0);
        let mut rq = VecDeque::new();
        let mut addq = VecDeque::new();
        rq.push_back((x, y, z, start));
        while let Some((px, py, pz, lv)) = rq.pop_front() {
            for (dx, dy, dz) in DIRS {
                let (nx, ny, nz) = (px + dx, py + dy, pz + dz);
                if !(0..CH).contains(&ny) {
                    continue;
                }
                let nl = self.get_chan(chan, nx, ny, nz);
                if nl == 0 {
                    continue;
                }
                let depends = nl < lv
                    || (chan == Chan::Sky && dy == -1 && nl == lv && lv == SKY_LIGHT_MAX);
                if depends {
                    self.set_chan(chan, nx, ny, nz, 0);
                    rq.push_back((nx, ny, nz, nl));
                } else {
                    // この光は別の光源由来 → 再伝播の種にする
                    addq.push_back((nx, ny, nz, nl));
                }
            }
        }
        // 発見時に「別光源」に見えた種も、その後に別経路から届いたより強い除去波で
        // 消えていることがある。発見時の値のまま再点灯すると、実際にはどの光源にも
        // つながっていない「幽霊光」のプラトーが残る(以後の除去波は同値以上の光を
        // 消せないため自己保持してしまう)。現値が一致する種だけを流す
        addq.retain(|&(nx, ny, nz, nl)| self.get_chan(chan, nx, ny, nz) == nl);
        self.flood_add(chan, addq);
    }

    /// 列 (x, z) の直射日光の床(最上段不透明ブロックの1つ上)。
    /// この高さから上のセルはすべて遮蔽なしの直射日光(最大値)になる
    fn sky_floor(&self, x: i32, z: i32) -> i32 {
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        match self.chunks.get(&key) {
            Some(c) => c.height(x.rem_euclid(CS), z.rem_euclid(CS)) + 1,
            None => CH, // 未生成: 種を作らない側に倒す(生成時に relight で処理)
        }
    }

    /// 生成直後のチャンクに光を入れる: チャンク内の光源(松明)を点灯し、
    /// 各列の直射日光を垂直に流し込み、隣接チャンクの境界から漏れてくる
    /// 松明光・スカイライトを流し込む
    pub fn relight_chunk(&mut self, cx: i32, cz: i32) {
        let mut tq = VecDeque::new();
        let mut sq = VecDeque::new();
        if let Some(c) = self.chunks.get(&(cx, cz)) {
            for y in 0..CH {
                for lz in 0..CS {
                    for lx in 0..CS {
                        let b = c.get(lx, y, lz);
                        if b.emission() > 0 {
                            tq.push_back((cx * CS + lx, y, cz * CS + lz, b.emission()));
                        }
                    }
                }
            }
        }
        // スカイライト: 各列の床から上へ15を直接書き込み(set-at-enqueue)、
        // BFSの種は「隣接列の床より低い部分」だけに絞る。それより上は周囲も
        // 全て15なので展開しても何も起きず、全セルを種にすると生成が桁で重くなる
        if self.chunks.contains_key(&(cx, cz)) {
            for lz in 0..CS {
                for lx in 0..CS {
                    let (x, z) = (cx * CS + lx, cz * CS + lz);
                    let f = self.sky_floor(x, z).min(CH);
                    let mut fmax = f;
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (x + dx, z + dz);
                        if self
                            .chunks
                            .contains_key(&(nx.div_euclid(CS), nz.div_euclid(CS)))
                        {
                            fmax = fmax.max(self.sky_floor(nx, nz).min(CH));
                        }
                    }
                    let c = self.chunks.get_mut(&(cx, cz)).unwrap();
                    for y in f..CH {
                        c.skylight[idx(lx, y, lz)] = SKY_LIGHT_MAX;
                    }
                    for y in f..fmax {
                        sq.push_back((x, y, z, SKY_LIGHT_MAX));
                    }
                }
            }
            self.chunks.get_mut(&(cx, cz)).unwrap().dirty = true;
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
                    if self.get_block(wx, y, wz).is_opaque() {
                        continue;
                    }
                    let l = self.get_light(wx + dx, y, wz + dz);
                    if l > 1 {
                        tq.push_back((wx, y, wz, l - 1));
                    }
                    let sl = self.get_skylight(wx + dx, y, wz + dz);
                    if sl > 1 {
                        sq.push_back((wx, y, wz, sl - 1));
                    }
                }
            }
        }
        self.flood_add(Chan::Torch, tq);
        self.flood_add(Chan::Sky, sq);
    }

    /// セーブ由来の編集差分を生成直後のチャンクへ適用する
    pub fn apply_edits_to(&mut self, c: &mut Chunk, cx: i32, cz: i32) {
        let mut touched = false;
        let mut requeue: Vec<(i32, i32, i32)> = Vec::new();
        for (&(x, y, z), &b) in &self.edits {
            if x.div_euclid(CS) != cx || z.div_euclid(CS) != cz {
                continue;
            }
            // idx() は範囲チェックをしない。edits は通常 set_block / 検証済み parse
            // 由来で範囲内だが、万一の不正な y で配列範囲外 panic を起こさないよう弾く
            if !(0..CH).contains(&y) {
                continue;
            }
            let i = idx(x.rem_euclid(CS), y, z.rem_euclid(CS));
            c.blocks[i] = b;
            // 水は編集差分では常に満水として復元し、実レベルは水流の再評価に委ねる
            // (セーブは水レベルを保存しないため、決定的に再計算する設計判断)
            c.water_level[i] = if Block::from_u8(b) == Block::Water {
                WATER_MAX
            } else {
                0
            };
            // 掘った穴(Air)が海に隣接していれば流入が再開するよう再評価対象にする
            requeue.push((x, y, z));
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
        // 編集セルとその周囲を水流キューで再評価させる(海への穴の再流入・水の復元)
        for (x, y, z) in requeue {
            self.enqueue_water_neighbors(x, y, z);
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

    /// 水レベル(そのセルがWaterでなければ0)
    pub fn water_level_at(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..CH).contains(&y) {
            return 0;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        match self.chunks.get(&key) {
            Some(c) => c.water_level_at(x.rem_euclid(CS), y, z.rem_euclid(CS)),
            None => 0,
        }
    }

    fn set_water_level(&mut self, x: i32, y: i32, z: i32, v: u8) {
        if !(0..CH).contains(&y) {
            return;
        }
        let key = (x.div_euclid(CS), z.div_euclid(CS));
        let Some(c) = self.chunks.get_mut(&key) else {
            return;
        };
        let i = idx(x.rem_euclid(CS), y, z.rem_euclid(CS));
        c.water_level[i] = v;
        c.blocks[i] = if v > 0 { Block::Water as u8 } else { Block::Air as u8 };
        self.mark_dirty_around(x, z);
    }

    /// セル (x,y,z) とその6近傍を水流の再評価キューに積む(破壊/設置の直後に呼ぶ)
    fn enqueue_water_neighbors(&mut self, x: i32, y: i32, z: i32) {
        self.water_queue.push_back((x, y, z));
        for (dx, dy, dz) in DIRS {
            self.water_queue.push_back((x + dx, y + dy, z + dz));
        }
    }

    /// このセルがあるべき水レベルを、隣接セルから計算する(源には触れない)。
    /// 上が水なら減衰なしで最大値、それ以外は横4方向の最大値-1(0以下なら水なし)
    fn compute_flow_level(&self, x: i32, y: i32, z: i32) -> u8 {
        if self.get_block(x, y + 1, z) == Block::Water {
            return WATER_MAX;
        }
        let mut best = 0u8;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, nz) = (x + dx, z + dz);
            if self.get_block(nx, y, nz) == Block::Water {
                let nl = self.water_level_at(nx, y, nz);
                if nl > 0 {
                    best = best.max(nl - 1);
                }
            }
        }
        best
    }

    /// 水流を予算内で処理する。1件あたり最大 `budget` セルまでキューを消費し、
    /// 無限拡散・フレーム落ちを防ぐ(拡散が続く限りキューには残りが積まれる)
    pub fn tick_water(&mut self, budget: i32) {
        let mut n = 0;
        while n < budget {
            let Some((x, y, z)) = self.water_queue.pop_front() else {
                break;
            };
            n += 1;
            if !(0..CH).contains(&y) {
                continue;
            }
            if !self
                .chunks
                .contains_key(&(x.div_euclid(CS), z.div_euclid(CS)))
            {
                continue;
            }
            let b = self.get_block(x, y, z);
            // 海(y<=SEAのWater)は source_level が常にWATER_MAXを返す無限源。
            // それ以外のWaterは毎フレーム周囲から必要レベルを再計算し、支えを
            // 失っていれば蒸発させる(除去BFSの代わりに毎回再評価する軽量方式)
            if b == Block::Water {
                let want = self.compute_flow_level(x, y, z).max(self.source_level(x, y, z));
                let cur = self.water_level_at(x, y, z);
                if want == 0 {
                    // 供給を失った: 蒸発させ、隣接セルを再評価
                    self.set_water_level(x, y, z, 0);
                    self.enqueue_water_neighbors(x, y, z);
                } else if want != cur {
                    self.set_water_level(x, y, z, want);
                    if want > 1 {
                        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            self.water_queue.push_back((x + dx, y, z + dz));
                        }
                    }
                    self.water_queue.push_back((x, y - 1, z));
                }
            } else if b.replaceable() {
                let want = self.compute_flow_level(x, y, z);
                if want > 0 {
                    self.set_water_level(x, y, z, want);
                    self.mark_dirty_around(x, z);
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        self.water_queue.push_back((x + dx, y, z + dz));
                    }
                    self.water_queue.push_back((x, y - 1, z));
                }
            }
        }
    }

    /// 海水源かどうか(y<=SEAで、上が空か海面ぎりぎりの海水柱の一部)。
    /// 生成時に置かれた海はプレイヤーが埋めない限り無限に湧き続ける源として扱う
    fn source_level(&self, x: i32, y: i32, z: i32) -> u8 {
        if y <= SEA && self.get_block(x, y, z) == Block::Water {
            WATER_MAX
        } else {
            0
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
            skylight: vec![0u8; blocks.len()],
            water_level: vec![0u8; blocks.len()],
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
        for cz in -1..=1 {
            for cx in -1..=1 {
                w.relight_chunk(cx, cz);
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
        assert!(w.height_at(5, 5) >= 50); // 木生成・スポーン用の高さも更新済み
    }

    #[test]
    fn water_edit_survives_save_roundtrip_via_apply_edits() {
        // セーブ→ロードを模す: 編集差分に記録された水が、生成後のチャンクに
        // 復元されて水流キューで整合性のあるレベルへ再評価されることを確認する。
        // y=30 は海面下なので無限源として安定して残る
        let mut w = World::new(9);
        w.edits.insert((5, 30, 5), Block::Water as u8);
        let mut c = generate_chunk(9, 0, 0);
        w.apply_edits_to(&mut c, 0, 0);
        w.chunks.insert((0, 0), c);
        w.relight_chunk(0, 0);
        assert_eq!(w.get_block(5, 30, 5), Block::Water);
        assert_eq!(w.water_level_at(5, 30, 5), WATER_MAX);
        for _ in 0..50 {
            w.tick_water(64);
        }
        // 再評価後も水のまま安定している
        assert_eq!(w.get_block(5, 30, 5), Block::Water);
        assert_eq!(w.water_level_at(5, 30, 5), WATER_MAX);
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

    #[test]
    fn skylight_propagates_vertically_without_decay() {
        let w = flat_world();
        // 地表(y=11)から世界の天辺まで、直射日光は減衰せず最大値のまま
        for y in 11..CH {
            assert_eq!(w.get_skylight(8, y, 8), 15, "y={y}");
        }
        // 地表の石(y<=10)には入らない
        assert_eq!(w.get_skylight(8, 10, 8), 0);
    }

    #[test]
    fn skylight_hole_makes_light_shaft_and_decays_horizontally() {
        let mut w = flat_world();
        // y=13 に大きな屋根を張る(減衰距離15より広く、縁からの漏れを遮る)。
        // 下の空間は y=11..12 の2段
        for dz in -16..=16 {
            for dx in -16..=16 {
                w.set_block(8 + dx, 13, 8 + dz, Block::Stone);
            }
        }
        assert_eq!(w.get_skylight(8, 12, 8), 0); // 屋根下の中心は真っ暗

        // 屋根に1マス穴を開けると、直下は無減衰の光の柱、横は1ずつ減衰
        w.set_block(8, 13, 8, Block::Air);
        assert_eq!(w.get_skylight(8, 12, 8), 15);
        assert_eq!(w.get_skylight(8, 11, 8), 15); // 柱は下まで15
        assert_eq!(w.get_skylight(9, 12, 8), 14);
        assert_eq!(w.get_skylight(12, 12, 8), 11); // 4ホップで-4

        // 再び塞ぐと柱ごと消えて真っ暗に戻る
        w.set_block(8, 13, 8, Block::Stone);
        assert_eq!(w.get_skylight(8, 12, 8), 0);
        assert_eq!(w.get_skylight(9, 12, 8), 0);
    }

    #[test]
    fn skylight_flows_across_chunk_boundary_on_generation() {
        let mut w = flat_world();
        // チャンク(1,0)を「あとから生成された」状態にして再点灯を確認
        w.chunks.insert((1, 0), flat_chunk());
        assert_eq!(w.get_skylight(18, 12, 8), 0);
        w.relight_chunk(1, 0);
        assert_eq!(w.get_skylight(18, 12, 8), 15); // 露出した列は独立して満点
    }

    #[test]
    fn sea_water_floods_dug_hole() {
        let mut w = flat_world();
        // 疑似的な海: y=10 (<= SEA) の石を水に置き換えると無限源になる
        w.set_block(8, 10, 8, Block::Water);
        for _ in 0..5 {
            w.tick_water(64);
        }
        assert_eq!(w.water_level_at(8, 10, 8), WATER_MAX);

        // 隣を掘ると水が流れ込む
        w.set_block(9, 10, 8, Block::Air);
        for _ in 0..10 {
            w.tick_water(64);
        }
        assert_eq!(w.get_block(9, 10, 8), Block::Water);
        assert!(w.water_level_at(9, 10, 8) > 0);
    }

    #[test]
    fn placed_block_displaces_water() {
        let mut w = flat_world();
        w.set_block(8, 10, 8, Block::Water);
        for _ in 0..5 {
            w.tick_water(64);
        }
        // 土嚢: 水のセルに固体を置くと水が消え、供給源を失うので戻らない
        w.set_block(8, 10, 8, Block::Cobble);
        assert_eq!(w.get_block(8, 10, 8), Block::Cobble);
        assert_eq!(w.water_level_at(8, 10, 8), 0);
        for _ in 0..10 {
            w.tick_water(64);
        }
        assert_eq!(w.get_block(8, 10, 8), Block::Cobble);
    }

    #[test]
    fn water_above_sea_decays_and_simulation_terminates() {
        let mut w = flat_world();
        // 海面より上(y=41 > SEA)に石の台と水を置く。源がないため
        // 有限に広がったのち引いて、キューは必ず空になる(停止性)
        for dz in -9..=9 {
            for dx in -9..=9 {
                w.set_block(8 + dx, 40, 8 + dz, Block::Stone);
            }
        }
        w.set_water_level(8, 41, 8, WATER_MAX);
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            w.water_queue.push_back((8 + dx, 41, 8 + dz));
        }
        let mut guard = 0;
        while !w.water_queue.is_empty() {
            w.tick_water(64);
            guard += 1;
            assert!(guard < 100_000, "water simulation did not settle");
        }
        // 源がないので最終的にすべて蒸発する(有限水)
        assert_eq!(w.water_level_at(8, 41, 8), 0);
        assert_eq!(w.get_block(12, 41, 8), Block::Air);
    }
}
