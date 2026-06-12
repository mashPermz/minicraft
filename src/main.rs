// minicraft: ブラウザで動く軽量マインクラフト風ゲーム

mod blocks;
mod mesher;
mod noise;
mod player;
mod sky;
mod textures;
mod world;

use blocks::{Block, HOTBAR};
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;
use player::Player;
use world::{World, CS, SEA};

const FOVY: f32 = 1.22; // 約70度

const TERRAIN_VS: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec4 vcolor;
varying mediump vec2 vuv;
varying lowp float vfog;
uniform mat4 Model;
uniform mat4 Projection;
uniform vec3 CamPos;
uniform float FogStart;
uniform float FogEnd;
void main() {
    vec4 wpos = Model * vec4(position, 1.0);
    gl_Position = Projection * wpos;
    vcolor = color0 / 255.0;
    vuv = texcoord;
    float d = length(wpos.xz - CamPos.xz);
    vfog = clamp((d - FogStart) / (FogEnd - FogStart), 0.0, 1.0);
}
"#;

const TERRAIN_FS: &str = r#"#version 100
precision mediump float;
varying lowp vec4 vcolor;
varying mediump vec2 vuv;
varying lowp float vfog;
uniform sampler2D Texture;
uniform vec3 FogColor;
uniform vec3 LightColor;
void main() {
    vec4 t = texture2D(Texture, vuv);
    if (t.a < 0.5) discard;
    vec3 c = t.rgb * vcolor.rgb * LightColor;
    gl_FragColor = vec4(mix(c, FogColor, vfog), 1.0);
}
"#;

const WATER_VS: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec4 vcolor;
varying mediump vec2 vuv;
varying lowp float vfog;
uniform mat4 Model;
uniform mat4 Projection;
uniform vec3 CamPos;
uniform float FogStart;
uniform float FogEnd;
uniform float Time;
void main() {
    vec4 wpos = Model * vec4(position, 1.0);
    gl_Position = Projection * wpos;
    vcolor = color0 / 255.0;
    // テクスチャの揺らぎ(アトラスのにじみ防止に振幅は半テクセル以内)
    vuv = texcoord + vec2(
        sin(Time * 0.8 + wpos.x * 0.7 + wpos.z * 0.3) * 0.003,
        cos(Time * 0.7 + wpos.z * 0.8) * 0.003
    );
    float d = length(wpos.xz - CamPos.xz);
    vfog = clamp((d - FogStart) / (FogEnd - FogStart), 0.0, 1.0);
}
"#;

const WATER_FS: &str = r#"#version 100
precision mediump float;
varying lowp vec4 vcolor;
varying mediump vec2 vuv;
varying lowp float vfog;
uniform sampler2D Texture;
uniform vec3 FogColor;
uniform vec3 LightColor;
void main() {
    vec4 t = texture2D(Texture, vuv);
    vec3 c = t.rgb * vcolor.rgb * LightColor;
    gl_FragColor = vec4(mix(c, FogColor, vfog), 0.62 * (1.0 - vfog * 0.6));
}
"#;

const CLOUD_VS: &str = r#"#version 100
attribute vec3 position;
attribute vec4 color0;
varying lowp float vfog;
uniform mat4 Model;
uniform mat4 Projection;
uniform vec3 CamPos;
uniform vec3 Offset;
void main() {
    vec4 wpos = Model * vec4(position + Offset, 1.0);
    gl_Position = Projection * wpos;
    float d = length(wpos.xz - CamPos.xz);
    vfog = clamp((d - 200.0) / 160.0, 0.0, 1.0);
}
"#;

const CLOUD_FS: &str = r#"#version 100
precision mediump float;
varying lowp float vfog;
uniform vec3 CloudColor;
void main() {
    gl_FragColor = vec4(CloudColor, 0.72 * (1.0 - vfog));
}
"#;

fn conf() -> Conf {
    Conf {
        window_title: "minicraft".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: false,
        ..Default::default()
    }
}

fn make_materials() -> (Material, Material, Material) {
    let fog_uniforms = vec![
        UniformDesc::new("CamPos", UniformType::Float3),
        UniformDesc::new("FogStart", UniformType::Float1),
        UniformDesc::new("FogEnd", UniformType::Float1),
        UniformDesc::new("FogColor", UniformType::Float3),
        UniformDesc::new("LightColor", UniformType::Float3),
    ];
    let opaque = PipelineParams {
        depth_write: true,
        depth_test: Comparison::LessOrEqual,
        ..Default::default()
    };
    let blend = BlendState::new(
        Equation::Add,
        BlendFactor::Value(BlendValue::SourceAlpha),
        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
    );
    let transparent = PipelineParams {
        depth_write: false,
        depth_test: Comparison::LessOrEqual,
        color_blend: Some(blend),
        ..Default::default()
    };

    let terrain = load_material(
        ShaderSource::Glsl {
            vertex: TERRAIN_VS,
            fragment: TERRAIN_FS,
        },
        MaterialParams {
            pipeline_params: opaque,
            uniforms: fog_uniforms.clone(),
            textures: vec![],
        },
    )
    .expect("terrain material");

    let mut water_uniforms = fog_uniforms.clone();
    water_uniforms.push(UniformDesc::new("Time", UniformType::Float1));
    let water = load_material(
        ShaderSource::Glsl {
            vertex: WATER_VS,
            fragment: WATER_FS,
        },
        MaterialParams {
            pipeline_params: transparent,
            uniforms: water_uniforms,
            textures: vec![],
        },
    )
    .expect("water material");

    let cloud = load_material(
        ShaderSource::Glsl {
            vertex: CLOUD_VS,
            fragment: CLOUD_FS,
        },
        MaterialParams {
            pipeline_params: transparent,
            uniforms: vec![
                UniformDesc::new("CamPos", UniformType::Float3),
                UniformDesc::new("Offset", UniformType::Float3),
                UniformDesc::new("CloudColor", UniformType::Float3),
            ],
            textures: vec![],
        },
    )
    .expect("cloud material");

    (terrain, water, cloud)
}

/// 海より上の地表となるスポーン地点を探す
fn find_spawn(seed: u32) -> (i32, i32) {
    for r in 0..64 {
        for (x, z) in [(r * 8, 0), (-r * 8, r * 4), (r * 4, -r * 8), (0, r * 8)] {
            let (h, _) = world::surface(seed, x, z);
            if h > SEA + 1 && h < 60 {
                return (x, z);
            }
        }
    }
    (0, 0)
}

/// プレイヤー周辺のチャンク生成とメッシュ更新(フレーム予算つき)。
/// 戻り値は半径内の未完了数(ロード進捗用)
fn update_chunks(
    w: &mut World,
    atlas: &Texture2D,
    pcx: i32,
    pcz: i32,
    radius: i32,
    mut gen_budget: i32,
    mut mesh_budget: i32,
) -> i32 {
    // 生成(メッシュ境界条件のため半径+1まで)
    let mut want: Vec<(i32, i32, i32)> = Vec::new();
    for dz in -(radius + 1)..=(radius + 1) {
        for dx in -(radius + 1)..=(radius + 1) {
            let key = (pcx + dx, pcz + dz);
            if !w.chunks.contains_key(&key) {
                want.push((dx * dx + dz * dz, key.0, key.1));
            }
        }
    }
    want.sort_unstable();
    let mut pending = 0;
    for &(_, cx, cz) in &want {
        if gen_budget > 0 {
            let c = world::generate_chunk(w.seed, cx, cz);
            w.chunks.insert((cx, cz), c);
            gen_budget -= 1;
        } else {
            pending += 1;
        }
    }

    // メッシュ(8近傍が生成済みのチャンクのみ)
    let mut dirty: Vec<(i32, i32, i32)> = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let key = (pcx + dx, pcz + dz);
            let Some(c) = w.chunks.get(&key) else {
                pending += 1;
                continue;
            };
            if c.dirty {
                dirty.push((dx * dx + dz * dz, key.0, key.1));
            }
        }
    }
    dirty.sort_unstable();
    for &(_, cx, cz) in &dirty {
        let neighbors_ready = (-1..=1)
            .all(|dz| (-1..=1).all(|dx| w.chunks.contains_key(&(cx + dx, cz + dz))));
        if !neighbors_ready || mesh_budget <= 0 {
            pending += 1;
            continue;
        }
        let (solid, water) = mesher::mesh_chunk(w, cx, cz, atlas);
        let c = w.chunks.get_mut(&(cx, cz)).unwrap();
        c.meshes = solid;
        c.water_meshes = water;
        c.dirty = false;
        mesh_budget -= 1;
    }
    pending
}

#[macroquad::main(conf)]
async fn main() {
    let seed = (macroquad::miniquad::date::now() as u32) | 1;
    let atlas = textures::build_atlas();
    let (terrain_mat, water_mat, cloud_mat) = make_materials();

    let mut w = World::new(seed);
    let (sx, sz) = find_spawn(seed);
    let (sh, _) = world::surface(seed, sx, sz);
    let mut player = Player::new(vec3(sx as f32 + 0.5, sh as f32 + 2.0, sz as f32 + 0.5));
    let mut clouds = sky::Clouds::new(seed);

    let mut radius: i32 = 4;
    let mut ready = false;
    let mut grabbed = false;
    let mut grab_cooldown = 0.0f32;
    let mut sel: usize = 0;
    let mut show_debug = false;
    let mut edit_cd = 0.0f32;
    let mut frame_no: u64 = 0;

    loop {
        let dt = get_frame_time().min(0.05);
        let time = get_time();
        frame_no += 1;

        // --- チャンクストリーミング ---
        let pcx = (player.pos.x.floor() as i32).div_euclid(CS);
        let pcz = (player.pos.z.floor() as i32).div_euclid(CS);
        let (gb, mb) = if ready { (3, 2) } else { (24, 12) };
        let pending = update_chunks(&mut w, &atlas, pcx, pcz, radius, gb, mb);
        let total = (2 * radius + 1) * (2 * radius + 1);
        if !ready && pending == 0 {
            // 地表(木を含む)の上に立たせる
            player.pos.y = w.height_at(sx, sz) as f32 + 1.01;
            player.vel = Vec3::ZERO;
            ready = true;
        }
        // 遠方チャンクの破棄(たまに実行)
        if frame_no % 120 == 0 {
            let keep = radius + 4;
            w.chunks
                .retain(|&(cx, cz), _| (cx - pcx).abs() <= keep && (cz - pcz).abs() <= keep);
        }

        // --- 入力: マウス捕捉 ---
        grab_cooldown = (grab_cooldown - dt).max(0.0);
        if is_mouse_button_pressed(MouseButton::Left) {
            if !grabbed {
                grabbed = true;
                grab_cooldown = 0.15;
            }
            // ブラウザ側でポインタロックが外れていても再要求(無害)
            set_cursor_grab(true);
            show_mouse(false);
        }
        if is_key_pressed(KeyCode::Escape) && grabbed {
            grabbed = false;
            set_cursor_grab(false);
            show_mouse(true);
        }
        if is_key_pressed(KeyCode::Tab) {
            show_debug = !show_debug;
        }
        if is_key_pressed(KeyCode::Minus) {
            radius = (radius - 1).max(3);
        }
        if is_key_pressed(KeyCode::Equal) {
            radius = (radius + 1).min(7);
        }

        // --- ホットバー選択 ---
        if grabbed {
            const KEYS: [KeyCode; 9] = [
                KeyCode::Key1,
                KeyCode::Key2,
                KeyCode::Key3,
                KeyCode::Key4,
                KeyCode::Key5,
                KeyCode::Key6,
                KeyCode::Key7,
                KeyCode::Key8,
                KeyCode::Key9,
            ];
            for (i, k) in KEYS.iter().enumerate() {
                if is_key_pressed(*k) {
                    sel = i;
                }
            }
            let (_, wheel) = mouse_wheel();
            if wheel < -0.01 {
                sel = (sel + 1) % 9;
            } else if wheel > 0.01 {
                sel = (sel + 8) % 9;
            }
        }

        // --- プレイヤー更新 ---
        if ready {
            player.update(&w, dt, grabbed, grab_cooldown > 0.0);
        }
        let eye = player.eye();
        let look = player.dir();

        // --- ブロック編集 ---
        edit_cd = (edit_cd - dt).max(0.0);
        let target = if ready { w.raycast(eye, look, 6.0) } else { None };
        if grabbed && ready && grab_cooldown <= 0.0 {
            if let Some((bp, n)) = target {
                let break_now = is_mouse_button_pressed(MouseButton::Left)
                    || (is_mouse_button_down(MouseButton::Left) && edit_cd <= 0.0);
                let place_now = is_mouse_button_pressed(MouseButton::Right)
                    || (is_mouse_button_down(MouseButton::Right) && edit_cd <= 0.0);
                if break_now && bp.y > 0 {
                    w.set_block(bp.x, bp.y, bp.z, Block::Air);
                    edit_cd = 0.22;
                } else if place_now && n != IVec3::ZERO {
                    let pp = bp + n;
                    if w.get_block(pp.x, pp.y, pp.z).replaceable() && !player.intersects_block(pp)
                    {
                        w.set_block(pp.x, pp.y, pp.z, HOTBAR[sel]);
                        edit_cd = 0.22;
                    }
                }
            }
        }

        // --- 空と光 ---
        let ds = sky::day_state(time);
        let underwater = {
            let e = eye.floor();
            w.get_block(e.x as i32, e.y as i32, e.z as i32) == Block::Water
        };
        let fend = (radius * CS) as f32 - 6.0;
        let (fog_start, fog_end, fog_color) = if underwater {
            (2.0, 14.0, vec3(0.05, 0.18, 0.38) * (0.3 + 0.7 * ds.lf))
        } else {
            (fend * 0.5, fend, ds.fog_color)
        };

        // --- 描画 ---
        set_default_camera();
        if underwater {
            clear_background(Color::new(fog_color.x, fog_color.y, fog_color.z, 1.0));
        } else {
            clear_background(ds.top);
            sky::draw_gradient(player.pitch, FOVY, ds.top, ds.horizon);
        }

        let cam = Camera3D {
            position: eye,
            target: eye + look,
            up: vec3(0.0, 1.0, 0.0),
            fovy: FOVY,
            ..Default::default()
        };
        if !underwater {
            sky::draw_celestial(&cam, eye, &ds);
        }
        set_camera(&cam);

        for m in [&terrain_mat, &water_mat] {
            m.set_uniform("CamPos", eye);
            m.set_uniform("FogStart", fog_start);
            m.set_uniform("FogEnd", fog_end);
            m.set_uniform("FogColor", fog_color);
            m.set_uniform("LightColor", ds.light);
        }
        water_mat.set_uniform("Time", time as f32);

        // 可視チャンク(粗い視錐台カリング)
        let mut visible: Vec<(i32, &world::Chunk)> = Vec::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let Some(c) = w.chunks.get(&(pcx + dx, pcz + dz)) else {
                    continue;
                };
                let center = vec3(
                    ((pcx + dx) * CS + 8) as f32,
                    48.0,
                    ((pcz + dz) * CS + 8) as f32,
                );
                let v = center - eye;
                let d2 = v.x * v.x + v.z * v.z;
                if d2 > 28.0 * 28.0 && v.normalize().dot(look) < 0.2 {
                    continue;
                }
                visible.push((d2 as i32, c));
            }
        }

        let mut tri_count = 0;
        gl_use_material(&terrain_mat);
        for (_, c) in &visible {
            for m in &c.meshes {
                tri_count += m.indices.len() / 3;
                draw_mesh(m);
            }
        }

        // 雲
        if !underwater {
            let off = clouds.update(player.pos, time as f32);
            cloud_mat.set_uniform("CamPos", eye);
            cloud_mat.set_uniform("Offset", off);
            cloud_mat.set_uniform("CloudColor", vec3(0.93, 0.95, 0.99) * (0.25 + 0.75 * ds.lf));
            gl_use_material(&cloud_mat);
            draw_mesh(&clouds.mesh);
        }

        // 水(遠い順に描画)
        visible.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        gl_use_material(&water_mat);
        for (_, c) in &visible {
            for m in &c.water_meshes {
                tri_count += m.indices.len() / 3;
                draw_mesh(m);
            }
        }
        gl_use_default_material();

        // 選択ブロックのハイライト
        if let Some((bp, _)) = target {
            draw_cube_wires(
                bp.as_vec3() + Vec3::splat(0.5),
                Vec3::splat(1.002),
                Color::new(0.05, 0.05, 0.05, 0.85),
            );
        }

        // --- UI ---
        set_default_camera();
        let sw = screen_width();
        let shh = screen_height();

        if underwater {
            draw_rectangle(0.0, 0.0, sw, shh, Color::new(0.1, 0.3, 0.6, 0.18));
        }

        if !ready {
            let done = (total - pending.min(total)) as f32 / total as f32;
            draw_rectangle(0.0, 0.0, sw, shh, Color::new(0.05, 0.07, 0.12, 1.0));
            let t = "MINICRAFT";
            let m = measure_text(t, None, 48, 1.0);
            draw_text(t, (sw - m.width) / 2.0, shh * 0.4, 48.0, WHITE);
            let bw = sw * 0.4;
            draw_rectangle_lines((sw - bw) / 2.0, shh * 0.5, bw, 18.0, 2.0, GRAY);
            draw_rectangle(
                (sw - bw) / 2.0 + 2.0,
                shh * 0.5 + 2.0,
                (bw - 4.0) * done,
                14.0,
                GREEN,
            );
            next_frame().await;
            continue;
        }

        // 照準
        let (ccx, ccy) = (sw / 2.0, shh / 2.0);
        let ch_c = Color::new(1.0, 1.0, 1.0, 0.85);
        draw_rectangle(ccx - 1.0, ccy - 9.0, 2.0, 18.0, ch_c);
        draw_rectangle(ccx - 9.0, ccy - 1.0, 18.0, 2.0, ch_c);

        // ホットバー
        let s = 46.0;
        let x0 = (sw - 9.0 * s) / 2.0;
        let y0 = shh - s - 12.0;
        draw_rectangle(
            x0 - 4.0,
            y0 - 4.0,
            9.0 * s + 8.0,
            s + 8.0,
            Color::new(0.0, 0.0, 0.0, 0.45),
        );
        for (i, b) in HOTBAR.iter().enumerate() {
            let x = x0 + i as f32 * s;
            let (tx, ty) = b.icon_tile();
            draw_texture_ex(
                &atlas,
                x + 7.0,
                y0 + 7.0,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(s - 14.0, s - 14.0)),
                    source: Some(Rect::new(tx as f32 * 16.0, ty as f32 * 16.0, 16.0, 16.0)),
                    ..Default::default()
                },
            );
            draw_text(&format!("{}", i + 1), x + 4.0, y0 + 14.0, 16.0, GRAY);
            if i == sel {
                draw_rectangle_lines(x - 1.0, y0 - 1.0, s + 2.0, s + 2.0, 3.0, WHITE);
            }
        }

        // デバッグ表示
        if show_debug {
            let lines = [
                format!("FPS: {}", get_fps()),
                format!(
                    "pos: {:.1} {:.1} {:.1}  chunk: {} {}",
                    player.pos.x, player.pos.y, player.pos.z, pcx, pcz
                ),
                format!("chunks: {}  tris: {}k", w.chunks.len(), tri_count / 1000),
                format!("radius: {} (-/= to change)  fly: {}", radius, player.fly),
            ];
            for (i, l) in lines.iter().enumerate() {
                draw_text(l, 10.0, 22.0 + i as f32 * 20.0, 20.0, WHITE);
            }
        }

        // 操作ガイド
        if !grabbed {
            let panel_w = 520.0;
            let panel_h = 200.0;
            let px = (sw - panel_w) / 2.0;
            let py = (shh - panel_h) / 2.0 - 40.0;
            draw_rectangle(px, py, panel_w, panel_h, Color::new(0.0, 0.0, 0.0, 0.65));
            let lines: [(&str, f32); 5] = [
                ("MINICRAFT", 34.0),
                ("Click to play", 24.0),
                ("WASD: move  Space: jump  F: fly  Shift: sprint", 18.0),
                ("L-click: break  R-click: place  1-9 / wheel: select", 18.0),
                ("Tab: debug  Esc: release mouse", 18.0),
            ];
            let mut ty = py + 44.0;
            for (l, size) in lines {
                let m = measure_text(l, None, size as u16, 1.0);
                draw_text(l, px + (panel_w - m.width) / 2.0, ty, size, WHITE);
                ty += size + 14.0;
            }
        }

        next_frame().await;
    }
}
