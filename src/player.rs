// プレイヤー: FPS視点、重力・ジャンプ・泳ぎ・飛行、AABB衝突

use crate::blocks::Block;
use crate::world::World;
use macroquad::prelude::*;

pub const HALF_W: f32 = 0.3;
pub const HEIGHT: f32 = 1.8;
pub const EYE: f32 = 1.62;

const GRAVITY: f32 = 26.0;
const JUMP_V: f32 = 8.4;
const WALK: f32 = 4.4;
const SPRINT: f32 = 6.9;
const FLY: f32 = 12.0;

pub struct Player {
    pub pos: Vec3, // 足元中心
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fly: bool,
    pub on_ground: bool,
}

impl Player {
    pub fn new(pos: Vec3) -> Player {
        Player {
            pos,
            vel: Vec3::ZERO,
            yaw: 0.8,
            pitch: -0.15,
            fly: false,
            on_ground: false,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + vec3(0.0, EYE, 0.0)
    }

    pub fn dir(&self) -> Vec3 {
        vec3(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.sin(),
        )
    }

    pub fn in_water(&self, world: &World) -> bool {
        let p = self.pos + vec3(0.0, 0.4, 0.0);
        world.get_block(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) == Block::Water
    }

    /// ブロック設置位置がプレイヤーと重なるか
    pub fn intersects_block(&self, bp: IVec3) -> bool {
        let min = self.pos - vec3(HALF_W, 0.0, HALF_W);
        let max = self.pos + vec3(HALF_W, HEIGHT, HALF_W);
        let b0 = bp.as_vec3();
        let b1 = b0 + Vec3::ONE;
        min.x < b1.x && max.x > b0.x && min.y < b1.y && max.y > b0.y && min.z < b1.z && max.z > b0.z
    }

    /// active = マウスが捕捉されているときだけ視点・移動入力を受け付ける
    pub fn update(&mut self, world: &World, dt: f32, active: bool, look_locked: bool) {
        // 現在チャンク未生成なら物理を止める(ロード中の落下防止)
        let key = (
            (self.pos.x.floor() as i32).div_euclid(crate::world::CS),
            (self.pos.z.floor() as i32).div_euclid(crate::world::CS),
        );
        if !world.chunks.contains_key(&key) {
            return;
        }

        if active && !look_locked {
            let d = mouse_delta_position();
            let aspect = screen_width() / screen_height();
            self.yaw -= d.x * 1.4 * aspect;
            self.pitch += d.y * 1.4;
            self.pitch = self.pitch.clamp(-1.54, 1.54);
        }

        if active && is_key_pressed(KeyCode::F) {
            self.fly = !self.fly;
            self.vel.y = 0.0;
        }

        let fwd = vec3(self.yaw.cos(), 0.0, self.yaw.sin());
        let right = vec3(-self.yaw.sin(), 0.0, self.yaw.cos());
        let mut wish = Vec3::ZERO;
        let mut forward_input = false;
        if active {
            if is_key_down(KeyCode::W) {
                wish += fwd;
                forward_input = true;
            }
            if is_key_down(KeyCode::S) {
                wish -= fwd;
            }
            if is_key_down(KeyCode::D) {
                wish += right;
            }
            if is_key_down(KeyCode::A) {
                wish -= right;
            }
        }
        if wish.length_squared() > 0.0 {
            wish = wish.normalize();
        }
        let sprint = active && is_key_down(KeyCode::LeftShift);
        let in_water = self.in_water(world);

        if self.fly {
            let boost = if active && is_key_down(KeyCode::LeftControl) {
                2.2
            } else {
                1.0
            };
            let mut target = wish * FLY * boost;
            if active && is_key_down(KeyCode::Space) {
                target.y = FLY * 0.8 * boost;
            }
            if active && is_key_down(KeyCode::LeftShift) {
                target.y = -FLY * 0.8 * boost;
            }
            self.vel += (target - self.vel) * (10.0 * dt).min(1.0);
        } else {
            let mut speed = if sprint && forward_input { SPRINT } else { WALK };
            if in_water {
                speed *= 0.55;
            }
            let target = wish * speed;
            let accel = if self.on_ground { 14.0 } else { 4.0 };
            let t = (accel * dt).min(1.0);
            self.vel.x += (target.x - self.vel.x) * t;
            self.vel.z += (target.z - self.vel.z) * t;

            if in_water {
                self.vel.y -= 8.0 * dt;
                if active && is_key_down(KeyCode::Space) {
                    self.vel.y += 30.0 * dt;
                }
                self.vel.y = self.vel.y.clamp(-3.5, 3.5);
            } else {
                self.vel.y -= GRAVITY * dt;
                self.vel.y = self.vel.y.max(-50.0);
                if active && is_key_down(KeyCode::Space) && self.on_ground {
                    self.vel.y = JUMP_V;
                }
            }
        }

        self.on_ground = false;
        let delta = self.vel * dt;
        self.move_axis(world, 0, delta.x);
        self.move_axis(world, 1, delta.y);
        self.move_axis(world, 2, delta.z);
    }

    fn move_axis(&mut self, world: &World, axis: usize, amount: f32) {
        if amount == 0.0 {
            return;
        }
        let steps = (amount.abs() / 0.45).ceil().max(1.0) as i32;
        let step = amount / steps as f32;
        const EPS: f32 = 1e-4;
        'outer: for _ in 0..steps {
            self.pos[axis] += step;
            let min = self.pos - vec3(HALF_W, 0.0, HALF_W);
            let max = self.pos + vec3(HALF_W, HEIGHT, HALF_W);
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
                            // 正方向: ブロックの手前面に張り付く
                            let off = match axis {
                                0 | 2 => HALF_W,
                                _ => HEIGHT,
                            };
                            self.pos[axis] = cell[axis] as f32 - off - EPS;
                        } else {
                            let off = match axis {
                                0 | 2 => HALF_W,
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
}
