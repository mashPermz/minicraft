// ブロック定義とテクスチャアトラスのタイル対応

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Block {
    Air = 0,
    Grass,
    Dirt,
    Stone,
    Sand,
    Log,
    Leaves,
    Water,
    SnowGrass,
    Plank,
    Cobble,
    Glass,
    Coal,
    Torch,
    TallGrass,
    FlowerRed,
    FlowerYellow,
}

/// 松明の光源レベル(1ブロックごとに1減衰)
pub const TORCH_LIGHT: u8 = 14;

pub const HOTBAR: [Block; 9] = [
    Block::Grass,
    Block::Stone,
    Block::Sand,
    Block::Log,
    Block::Leaves,
    Block::Plank,
    Block::Cobble,
    Block::Glass,
    Block::Torch,
];

/// 面の並び: 0=+Y(上) 1=-Y(下) 2=+X 3=-X 4=+Z 5=-Z
pub const FACE_UP: usize = 0;
pub const FACE_DOWN: usize = 1;

// アトラス(8x8タイル)上の位置
pub const TILE_GRASS_TOP: (u32, u32) = (0, 0);
pub const TILE_GRASS_SIDE: (u32, u32) = (1, 0);
pub const TILE_DIRT: (u32, u32) = (2, 0);
pub const TILE_STONE: (u32, u32) = (3, 0);
pub const TILE_SAND: (u32, u32) = (0, 1);
pub const TILE_LOG_SIDE: (u32, u32) = (1, 1);
pub const TILE_LOG_TOP: (u32, u32) = (2, 1);
pub const TILE_LEAVES: (u32, u32) = (3, 1);
pub const TILE_WATER: (u32, u32) = (0, 2);
pub const TILE_SNOW_TOP: (u32, u32) = (1, 2);
pub const TILE_SNOW_SIDE: (u32, u32) = (2, 2);
pub const TILE_PLANK: (u32, u32) = (3, 2);
pub const TILE_COBBLE: (u32, u32) = (0, 3);
pub const TILE_GLASS: (u32, u32) = (1, 3);
pub const TILE_COAL: (u32, u32) = (2, 3);
pub const TILE_TORCH: (u32, u32) = (3, 3);
pub const TILE_TALL_GRASS: (u32, u32) = (4, 0);
pub const TILE_FLOWER_RED: (u32, u32) = (5, 0);
pub const TILE_FLOWER_YELLOW: (u32, u32) = (6, 0);

impl Block {
    pub fn from_u8(v: u8) -> Block {
        if v <= Block::FlowerYellow as u8 {
            unsafe { std::mem::transmute::<u8, Block>(v) }
        } else {
            Block::Air
        }
    }

    /// 当たり判定があるか
    pub fn is_solid(self) -> bool {
        !matches!(self, Block::Air | Block::Water) && !self.is_cross()
    }

    /// 隣接面を完全に隠すか
    pub fn is_opaque(self) -> bool {
        !matches!(self, Block::Air | Block::Water | Block::Leaves | Block::Glass)
            && !self.is_cross()
    }

    /// 交差した2枚の板で描画するか(松明・草花)
    pub fn is_cross(self) -> bool {
        matches!(
            self,
            Block::Torch | Block::TallGrass | Block::FlowerRed | Block::FlowerYellow
        )
    }

    /// 下に固体ブロックが必要か(下が壊れたら一緒に壊れる)
    pub fn needs_support(self) -> bool {
        self.is_cross()
    }

    /// 発光レベル(0 = 光らない)
    pub fn emission(self) -> u8 {
        if self == Block::Torch {
            TORCH_LIGHT
        } else {
            0
        }
    }

    /// 同種ブロックが隣接したとき面を省略するか(ガラス・水)
    pub fn merges(self) -> bool {
        matches!(self, Block::Glass | Block::Water)
    }

    /// 設置時に上書きしてよいか
    pub fn replaceable(self) -> bool {
        matches!(
            self,
            Block::Air | Block::Water | Block::TallGrass | Block::FlowerRed | Block::FlowerYellow
        )
    }

    pub fn tile(self, face: usize) -> (u32, u32) {
        match self {
            Block::Grass => match face {
                FACE_UP => TILE_GRASS_TOP,
                FACE_DOWN => TILE_DIRT,
                _ => TILE_GRASS_SIDE,
            },
            Block::Dirt => TILE_DIRT,
            Block::Stone => TILE_STONE,
            Block::Sand => TILE_SAND,
            Block::Log => match face {
                FACE_UP | FACE_DOWN => TILE_LOG_TOP,
                _ => TILE_LOG_SIDE,
            },
            Block::Leaves => TILE_LEAVES,
            Block::Water => TILE_WATER,
            Block::SnowGrass => match face {
                FACE_UP => TILE_SNOW_TOP,
                FACE_DOWN => TILE_DIRT,
                _ => TILE_SNOW_SIDE,
            },
            Block::Plank => TILE_PLANK,
            Block::Cobble => TILE_COBBLE,
            Block::Glass => TILE_GLASS,
            Block::Coal => TILE_COAL,
            Block::Torch => TILE_TORCH,
            Block::TallGrass => TILE_TALL_GRASS,
            Block::FlowerRed => TILE_FLOWER_RED,
            Block::FlowerYellow => TILE_FLOWER_YELLOW,
            Block::Air => TILE_STONE,
        }
    }

    /// ホットバーUIに表示するタイル
    pub fn icon_tile(self) -> (u32, u32) {
        match self {
            Block::Grass => TILE_GRASS_SIDE,
            Block::SnowGrass => TILE_SNOW_SIDE,
            Block::Log => TILE_LOG_SIDE,
            _ => self.tile(2),
        }
    }
}
