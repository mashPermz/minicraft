// 受動的モブ(ブタ風の四足獣・ニワトリ風の小型鳥): スポーン・徘徊AI・被弾/逃走・簡易物理
//
// world.rs は読み取り専用の制約があるため、ブロック照会・heights は
// World の既存 public API (get_block / height_at / chunks) のみを使う。

use crate::world::{World, CS};
use macroquad::prelude::*;

/// 総数上限(仕様: 10〜15体程度)
pub const MAX_MOBS: usize = 12;
/// この距離を超えたらデスポーン
const DESPAWN_DIST: f32 = 48.0;
/// この範囲内のロード済み草ブロック上にのみスポーンする
const SPAWN_MIN_DIST: f32 = 12.0;
const SPAWN_MAX_DIST: f32 = 30.0;

const GRAVITY: f32 = 26.0;
const WANDER_SPEED: f32 = 1.6;
const FLEE_SPEED: f32 = 4.2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobKind {
    Pig,
    Chicken,
}

impl MobKind {
    fn half_width(self) -> f32 {
        match self {
            MobKind::Pig => 0.45,
            MobKind::Chicken => 0.25,
        }
    }
    fn height(self) -> f32 {
        match self {
            MobKind::Pig => 0.8,
            MobKind::Chicken => 0.6,
        }
    }
    fn max_hp(self) -> i32 {
        match self {
            MobKind::Pig => 3,
            MobKind::Chicken => 2,
        }
    }
}

/// AI状態機械: 待機・徘徊・被弾後の逃走
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobState {
    Idle,
    Wander,
    Flee,
}

pub struct Mob {
    pub kind: MobKind,
    pub pos: Vec3, // 足元中心
    pub vel: Vec3,
    pub yaw: f32,
    pub hp: i32,
    pub state: MobState,
    pub state_timer: f32,
    pub on_ground: bool,
    walk_phase: f32,
    flee_dir: Vec3,
}

impl Mob {
    fn new(kind: MobKind, pos: Vec3) -> Mob {
        Mob {
            kind,
            pos,
            vel: Vec3::ZERO,
            yaw: rand::gen_range(0.0, std::f32::consts::TAU),
            hp: kind.max_hp(),
            state: MobState::Idle,
            state_timer: rand::gen_range(0.5, 2.0),
            on_ground: false,
            walk_phase: 0.0,
            flee_dir: Vec3::ZERO,
        }
    }

    pub fn half_width(&self) -> f32 {
        self.kind.half_width()
    }
    pub fn height(&self) -> f32 {
        self.kind.height()
    }

    fn aabb(&self) -> (Vec3, Vec3) {
        let hw = self.half_width();
        (
            self.pos - vec3(hw, 0.0, hw),
            self.pos + vec3(hw, self.height(), hw),
        )
    }

    fn in_water(&self, world: &World) -> bool {
        let p = self.pos + vec3(0.0, 0.3, 0.0);
        world.get_block(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
            == crate::blocks::Block::Water
    }

    /// AI状態遷移(描画・物理に依存しない部分。ユニットテスト対象)
    fn update_ai(&mut self, dt: f32, in_water: bool) {
        self.state_timer -= dt;
        match self.state {
            MobState::Flee => {
                if self.state_timer <= 0.0 {
                    self.state = MobState::Idle;
                    self.state_timer = rand::gen_range(0.5, 2.0);
                }
            }
            MobState::Idle | MobState::Wander => {
                if in_water {
                    // 水に入ったら引き返す: 向きを反転して少し徘徊状態を維持
                    self.yaw += std::f32::consts::PI;
                    self.state = MobState::Wander;
                    self.state_timer = rand::gen_range(0.6, 1.4);
                } else if self.state_timer <= 0.0 {
                    if self.state == MobState::Idle {
                        self.state = MobState::Wander;
                        self.state_timer = rand::gen_range(1.5, 4.0);
                        self.yaw += rand::gen_range(-1.2, 1.2);
                    } else {
                        self.state = MobState::Idle;
                        self.state_timer = rand::gen_range(1.0, 3.0);
                    }
                } else if self.state == MobState::Wander && rand::gen_range(0.0, 1.0) < 0.01 {
                    // たまに向き変更
                    self.yaw += rand::gen_range(-0.8, 0.8);
                }
            }
        }
    }

    /// ダメージを受ける。攻撃者から離れる方向へノックバック+逃走状態へ
    pub fn hit(&mut self, damage: i32, from: Vec3) {
        self.hp -= damage;
        let mut away = self.pos - from;
        away.y = 0.0;
        if away.length_squared() < 1e-6 {
            away = vec3(self.yaw.cos(), 0.0, self.yaw.sin());
        } else {
            away = away.normalize();
        }
        self.flee_dir = away;
        self.yaw = away.z.atan2(away.x);
        self.vel = away * 5.5 + vec3(0.0, 6.5, 0.0);
        self.state = MobState::Flee;
        self.state_timer = rand::gen_range(1.5, 2.5);
    }

    pub fn is_dead(&self) -> bool {
        self.hp <= 0
    }

    fn move_axis(&mut self, world: &World, axis: usize, amount: f32) {
        if amount == 0.0 {
            return;
        }
        let steps = (amount.abs() / 0.45).ceil().max(1.0) as i32;
        let step = amount / steps as f32;
        const EPS: f32 = 1e-4;
        let hw = self.half_width();
        let h = self.height();
        'outer: for _ in 0..steps {
            self.pos[axis] += step;
            let min = self.pos - vec3(hw, 0.0, hw);
            let max = self.pos + vec3(hw, h, hw);
            let lo = [
                min.x.floor() as i32,
                min.y.floor() as i32,
                min.z.floor() as i32,
            ];
            let hi = [
                (max.x - EPS).floor() as i32,
                (max.y - EPS).floor() as i32,
                (max.z - EPS).floor() as i32,
            ];
            for by in lo[1]..=hi[1] {
                for bz in lo[2]..=hi[2] {
                    for bx in lo[0]..=hi[0] {
                        if !world.get_block(bx, by, bz).is_solid() {
                            continue;
                        }
                        let cell = [bx, by, bz];
                        if step > 0.0 {
                            let off = match axis {
                                0 | 2 => hw,
                                _ => h,
                            };
                            self.pos[axis] = cell[axis] as f32 - off - EPS;
                        } else {
                            let off = match axis {
                                0 | 2 => hw,
                                _ => 0.0,
                            };
                            self.pos[axis] = (cell[axis] + 1) as f32 + off + EPS;
                            if axis == 1 {
                                self.on_ground = true;
                            }
                        }
                        self.vel[axis] = 0.0;
                        break 'outer;
                    }
                }
            }
        }
    }

    /// 1ブロック段差を登れるようにする: 進行方向前方が塞がっていて、
    /// その上が空いていれば少し持ち上げる
    fn try_step_up(&mut self, world: &World, wish: Vec3) {
        if wish.length_squared() < 1e-6 || !self.on_ground {
            return;
        }
        let hw = self.half_width();
        let ahead = self.pos + wish.normalize() * (hw + 0.15);
        let (fx, fz) = (ahead.x.floor() as i32, ahead.z.floor() as i32);
        let fy = self.pos.y.floor() as i32;
        let blocked = world.get_block(fx, fy, fz).is_solid();
        let clear_above = !world.get_block(fx, fy + 1, fz).is_solid()
            && !world.get_block(fx, fy + 2, fz).is_solid();
        if blocked && clear_above {
            self.pos.y = (fy + 1) as f32 + EPS_STEP;
            self.vel.y = self.vel.y.max(0.0);
        }
    }

    fn physics_update(&mut self, world: &World, dt: f32) {
        let in_water = self.in_water(world);
        let speed = if self.state == MobState::Flee {
            FLEE_SPEED
        } else if self.state == MobState::Wander {
            WANDER_SPEED
        } else {
            0.0
        };
        let mut wish = vec3(self.yaw.cos(), 0.0, self.yaw.sin());
        if wish.length_squared() > 0.0 {
            wish = wish.normalize();
        }
        let target = if self.state == MobState::Flee {
            self.flee_dir * speed
        } else {
            wish * speed
        };
        let accel = if self.on_ground { 10.0 } else { 3.0 };
        let t = (accel * dt).min(1.0);
        self.vel.x += (target.x - self.vel.x) * t;
        self.vel.z += (target.z - self.vel.z) * t;

        if in_water {
            // 浮く: 沈まない程度にゆっくり浮上
            self.vel.y += 6.0 * dt;
            self.vel.y = self.vel.y.clamp(-1.0, 2.0);
        } else {
            self.vel.y -= GRAVITY * dt;
            self.vel.y = self.vel.y.max(-50.0);
        }

        self.try_step_up(world, target);

        self.on_ground = false;
        let delta = self.vel * dt;
        self.move_axis(world, 0, delta.x);
        self.move_axis(world, 1, delta.y);
        self.move_axis(world, 2, delta.z);

        // 崖からの引き返し: 徘徊中に進行方向の足元が空いていて落差が大きいなら
        // 向きを反転する(連続落下防止の常識的挙動)
        if self.state == MobState::Wander && self.on_ground {
            let ahead = self.pos + wish * (self.half_width() + 0.4);
            let (ax, az) = (ahead.x.floor() as i32, ahead.z.floor() as i32);
            let ground_y = self.pos.y.floor() as i32 - 1;
            let mut found = false;
            for dy in 0..3 {
                if world.get_block(ax, ground_y - dy, az).is_solid() {
                    found = true;
                    break;
                }
            }
            if !found {
                self.yaw += std::f32::consts::PI;
            }
        }

        let hs = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
        self.walk_phase += hs * dt * 3.0;
    }

    /// 三人称の簡易ブロック造形描画(player.rs draw_model と同様の手法)
    pub fn draw(&self, light: Vec3) {
        let hs = (self.vel.x * self.vel.x + self.vel.z * self.vel.z).sqrt();
        let swing = self.walk_phase.sin() * (hs / WANDER_SPEED.max(0.1)).min(1.3) * 0.5;
        let cl = |r: f32, g: f32, b: f32| Color::new(r * light.x, g * light.y, b * light.z, 1.0);
        unsafe {
            get_internal_gl()
                .quad_gl
                .push_model_matrix(Mat4::from_translation(self.pos) * Mat4::from_rotation_y(-self.yaw));
        }
        match self.kind {
            MobKind::Pig => {
                let skin = cl(0.93, 0.62, 0.66);
                draw_cube(vec3(0.0, 0.5, 0.0), vec3(0.5, 0.42, 0.78), None, skin);
                draw_cube(vec3(0.42, 0.48, 0.0), vec3(0.32, 0.32, 0.32), None, skin);
                for (sx, sz) in [(0.22, 0.16), (0.22, -0.16), (-0.22, 0.16), (-0.22, -0.16)] {
                    let a = swing * if sz > 0.0 { 1.0 } else { -1.0 };
                    unsafe {
                        get_internal_gl().quad_gl.push_model_matrix(
                            Mat4::from_translation(vec3(sx, 0.29, sz)) * Mat4::from_rotation_x(a),
                        );
                    }
                    draw_cube(vec3(0.0, -0.145, 0.0), vec3(0.14, 0.29, 0.14), None, skin);
                    unsafe {
                        get_internal_gl().quad_gl.pop_model_matrix();
                    }
                }
            }
            MobKind::Chicken => {
                let feather = cl(0.95, 0.95, 0.92);
                let comb = cl(0.85, 0.2, 0.2);
                draw_cube(vec3(0.0, 0.42, 0.0), vec3(0.3, 0.34, 0.4), None, feather);
                draw_cube(vec3(0.26, 0.5, 0.0), vec3(0.18, 0.18, 0.18), None, feather);
                draw_cube(vec3(0.34, 0.55, 0.0), vec3(0.08, 0.08, 0.08), None, comb);
                for (sx, sz) in [(0.06, 0.09), (0.06, -0.09), (-0.06, 0.09), (-0.06, -0.09)] {
                    let a = swing * if sz > 0.0 { 1.0 } else { -1.0 };
                    unsafe {
                        get_internal_gl().quad_gl.push_model_matrix(
                            Mat4::from_translation(vec3(sx, 0.24, sz)) * Mat4::from_rotation_x(a),
                        );
                    }
                    draw_cube(vec3(0.0, -0.12, 0.0), vec3(0.06, 0.24, 0.06), None, comb);
                    unsafe {
                        get_internal_gl().quad_gl.pop_model_matrix();
                    }
                }
            }
        }
        unsafe {
            get_internal_gl().quad_gl.pop_model_matrix();
        }
    }
}

const EPS_STEP: f32 = 1e-3;

pub struct MobManager {
    pub mobs: Vec<Mob>,
}

impl MobManager {
    pub fn new() -> MobManager {
        MobManager { mobs: Vec::new() }
    }

    /// スポーン条件(描画非依存、テスト対象): 草ブロック上・洞窟でない・水中でない
    pub fn spawn_ok(world: &World, seed: u32, x: i32, z: i32) -> Option<i32> {
        let h = world.height_at(x, z);
        if h <= 0 || h + 1 >= crate::world::CH {
            return None;
        }
        let top = world.get_block(x, h, z);
        if top != crate::blocks::Block::Grass && top != crate::blocks::Block::SnowGrass {
            return None;
        }
        if world.get_block(x, h + 1, z) != crate::blocks::Block::Air {
            return None;
        }
        if crate::world::cave_at(seed, x, h, z) {
            return None;
        }
        Some(h)
    }

    /// プレイヤー周辺のロード済みチャンクにモブを補充スポーンする
    pub fn try_spawn(&mut self, world: &World, seed: u32, player_pos: Vec3) {
        if self.mobs.len() >= MAX_MOBS {
            return;
        }
        // ロード済みチャンクからランダムに1つ選び、その中のランダムな列を試す
        let keys: Vec<(i32, i32)> = world.chunks.keys().copied().collect();
        if keys.is_empty() {
            return;
        }
        for _ in 0..4 {
            let (cx, cz) = keys[rand::gen_range(0, keys.len())];
            let lx = rand::gen_range(0, CS);
            let lz = rand::gen_range(0, CS);
            let x = cx * CS + lx;
            let z = cz * CS + lz;
            let d = ((x as f32 - player_pos.x).powi(2) + (z as f32 - player_pos.z).powi(2)).sqrt();
            if !(SPAWN_MIN_DIST..=SPAWN_MAX_DIST).contains(&d) {
                continue;
            }
            if let Some(h) = Self::spawn_ok(world, seed, x, z) {
                let kind = if rand::gen_range(0, 2) == 0 {
                    MobKind::Pig
                } else {
                    MobKind::Chicken
                };
                self.mobs.push(Mob::new(
                    kind,
                    vec3(x as f32 + 0.5, h as f32 + 1.0, z as f32 + 0.5),
                ));
                return;
            }
        }
    }

    /// デスポーン(プレイヤーから離れすぎ or HP0)。描画非依存でテスト対象
    pub fn despawn(&mut self, player_pos: Vec3) {
        self.mobs.retain(|m| {
            if m.is_dead() {
                return false;
            }
            let d2 = (m.pos.x - player_pos.x).powi(2) + (m.pos.z - player_pos.z).powi(2);
            d2 <= DESPAWN_DIST * DESPAWN_DIST
        });
    }

    /// 毎フレーム更新。遠いモブは間引いてAI/物理コストを抑える
    pub fn update(&mut self, world: &World, seed: u32, player_pos: Vec3, dt: f32, frame_no: u64) {
        self.try_spawn(world, seed, player_pos);
        for (i, m) in self.mobs.iter_mut().enumerate() {
            let d2 = (m.pos.x - player_pos.x).powi(2) + (m.pos.z - player_pos.z).powi(2);
            // 20ブロック以遠は3フレームに1回だけ更新(遠距離間引き)
            if d2 > 20.0 * 20.0 && !(frame_no as usize + i).is_multiple_of(3) {
                continue;
            }
            let in_water = m.in_water(world);
            m.update_ai(dt, in_water);
            m.physics_update(world, dt);
        }
        self.despawn(player_pos);
    }

    pub fn draw_all(&self, light: Vec3) {
        for m in &self.mobs {
            m.draw(light);
        }
    }

    /// レイキャストでモブに命中するか判定(ブロックより手前限定)。
    /// 命中したモブのインデックスと衝突距離tを返す
    pub fn raycast_hit(&self, o: Vec3, dir: Vec3, max_t: f32) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, m) in self.mobs.iter().enumerate() {
            let (bmin, bmax) = m.aabb();
            if let Some(t) = ray_aabb(o, dir, bmin, bmax, max_t) {
                if best.map(|(_, bt)| t < bt).unwrap_or(true) {
                    best = Some((i, t));
                }
            }
        }
        best
    }

    /// 攻撃: 命中したモブにダメージを与え、HP0なら除去する
    pub fn attack(&mut self, idx: usize, damage: i32, from: Vec3) {
        if let Some(m) = self.mobs.get_mut(idx) {
            m.hit(damage, from);
            if m.is_dead() {
                self.mobs.remove(idx);
            }
        }
    }
}

impl Default for MobManager {
    fn default() -> Self {
        Self::new()
    }
}

/// スラブ法によるレイ-AABB交差判定。ヒットする最小のtを返す
fn ray_aabb(o: Vec3, dir: Vec3, bmin: Vec3, bmax: Vec3, max_t: f32) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = max_t;
    for a in 0..3 {
        let (o_a, d_a, min_a, max_a) = (o[a], dir[a], bmin[a], bmax[a]);
        if d_a.abs() < 1e-8 {
            if o_a < min_a || o_a > max_a {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d_a;
        let mut ta = (min_a - o_a) * inv;
        let mut tb = (max_a - o_a) * inv;
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::Block;
    use crate::world::CH;

    fn flat_world_with_grass() -> World {
        let mut w = World::new(1);
        for cz in -2..=2 {
            for cx in -2..=2 {
                let mut blocks = vec![0u8; (CS * CH * CS) as usize];
                for lz in 0..CS {
                    for lx in 0..CS {
                        for y in 0..10 {
                            let idx = ((y * CS + lz) * CS + lx) as usize;
                            blocks[idx] = Block::Stone as u8;
                        }
                        let idx = ((10 * CS + lz) * CS + lx) as usize;
                        blocks[idx] = Block::Grass as u8;
                    }
                }
                let c = crate::world::Chunk {
                    light: vec![0u8; blocks.len()],
                    blocks,
                    heights: [10; 256],
                    dirty: true,
                    meshes: Vec::new(),
                    water_meshes: Vec::new(),
                };
                w.chunks.insert((cx, cz), c);
            }
        }
        w
    }

    #[test]
    fn spawn_ok_on_grass_top() {
        let w = flat_world_with_grass();
        // cave_at はシード依存だが、開けた平地なので大半の座標で洞窟判定はfalseのはず。
        // 複数座標を試し、少なくとも1つはスポーン可能であることを確認する
        let mut ok = false;
        for x in 0..16 {
            if MobManager::spawn_ok(&w, 1, x, 3).is_some() {
                ok = true;
                break;
            }
        }
        assert!(ok, "grass top should allow spawn at some column");
    }

    #[test]
    fn spawn_rejected_underwater_or_in_stone() {
        let mut w = flat_world_with_grass();
        // 列(0,0)の上をブロックで塞ぐ(水中相当)
        w.set_block(0, 11, 0, Block::Water);
        assert!(MobManager::spawn_ok(&w, 1, 0, 0).is_none());
    }

    #[test]
    fn spawn_rejected_in_unloaded_chunk() {
        let w = World::new(1); // チャンク未生成
        assert!(MobManager::spawn_ok(&w, 1, 0, 0).is_none());
    }

    #[test]
    fn despawn_removes_far_and_dead_mobs() {
        let mut mgr = MobManager::new();
        mgr.mobs.push(Mob::new(MobKind::Pig, vec3(0.0, 11.0, 0.0)));
        mgr.mobs.push(Mob::new(MobKind::Chicken, vec3(100.0, 11.0, 0.0)));
        let mut dead = Mob::new(MobKind::Pig, vec3(1.0, 11.0, 1.0));
        dead.hp = 0;
        mgr.mobs.push(dead);
        assert_eq!(mgr.mobs.len(), 3);
        mgr.despawn(vec3(0.0, 11.0, 0.0));
        assert_eq!(mgr.mobs.len(), 1);
        assert_eq!(mgr.mobs[0].kind, MobKind::Pig);
    }

    #[test]
    fn hit_reduces_hp_and_sets_flee_state() {
        let mut m = Mob::new(MobKind::Chicken, vec3(5.0, 11.0, 5.0));
        assert_eq!(m.hp, 2);
        m.hit(1, vec3(6.0, 11.0, 5.0));
        assert_eq!(m.hp, 1);
        assert_eq!(m.state, MobState::Flee);
        // ノックバックは攻撃者から離れる方向(-x)
        assert!(m.vel.x < 0.0);
        assert!(m.vel.y > 0.0);
        m.hit(1, vec3(6.0, 11.0, 5.0));
        assert!(m.is_dead());
    }

    #[test]
    fn attack_removes_dead_mob_from_manager() {
        let mut mgr = MobManager::new();
        mgr.mobs.push(Mob::new(MobKind::Chicken, vec3(0.0, 11.0, 0.0)));
        assert_eq!(mgr.mobs.len(), 1);
        mgr.attack(0, 5, vec3(1.0, 11.0, 0.0)); // ダメージ5 > HP2 で即死
        assert_eq!(mgr.mobs.len(), 0);
    }

    #[test]
    fn ai_idle_transitions_to_wander_after_timer() {
        let mut m = Mob::new(MobKind::Pig, vec3(0.0, 11.0, 0.0));
        m.state = MobState::Idle;
        m.state_timer = 0.05;
        m.update_ai(0.1, false);
        assert_eq!(m.state, MobState::Wander);
    }

    #[test]
    fn ai_enters_wander_when_in_water() {
        let mut m = Mob::new(MobKind::Pig, vec3(0.0, 11.0, 0.0));
        m.state = MobState::Idle;
        m.state_timer = 5.0; // タイマーはまだ残っている
        m.update_ai(0.1, true);
        assert_eq!(m.state, MobState::Wander);
    }

    #[test]
    fn flee_state_expires_back_to_idle() {
        let mut m = Mob::new(MobKind::Pig, vec3(0.0, 11.0, 0.0));
        m.state = MobState::Flee;
        m.state_timer = 0.01;
        m.update_ai(0.1, false);
        assert_eq!(m.state, MobState::Idle);
    }

    #[test]
    fn ray_aabb_hits_mob_box() {
        let bmin = vec3(-0.5, 0.0, -0.5);
        let bmax = vec3(0.5, 1.0, 0.5);
        let hit = ray_aabb(vec3(0.0, 0.5, -5.0), vec3(0.0, 0.0, 1.0), bmin, bmax, 10.0);
        assert!(hit.is_some());
        let miss = ray_aabb(vec3(5.0, 0.5, -5.0), vec3(0.0, 0.0, 1.0), bmin, bmax, 10.0);
        assert!(miss.is_none());
    }

    #[test]
    fn manager_raycast_hit_picks_nearest_mob() {
        let mut mgr = MobManager::new();
        mgr.mobs.push(Mob::new(MobKind::Pig, vec3(0.0, 0.0, 5.0)));
        mgr.mobs.push(Mob::new(MobKind::Pig, vec3(0.0, 0.0, 2.0)));
        let hit = mgr.raycast_hit(vec3(0.0, 0.4, -5.0), vec3(0.0, 0.0, 1.0), 20.0);
        assert!(hit.is_some());
        let (idx, _) = hit.unwrap();
        assert_eq!(idx, 1); // z=2のほうが近い
    }

    #[test]
    fn max_mobs_cap_respected() {
        let w = flat_world_with_grass();
        let mut mgr = MobManager::new();
        for _ in 0..500 {
            mgr.try_spawn(&w, 1, vec3(8.0, 11.0, 8.0));
        }
        assert!(mgr.mobs.len() <= MAX_MOBS);
    }
}
