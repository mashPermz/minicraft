// セーブの永続化とシリアライズ。
// wasmではブラウザのlocalStorage(web/index.htmlのプラグインがenvに注入)、
// ネイティブ(テスト実行)では一時ファイルに保存する。

use crate::blocks::Block;
use crate::world::CH;
use macroquad::prelude::*;
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
mod backend {
    extern "C" {
        fn ls_save(ptr: *const u8, len: u32);
        fn ls_size() -> u32;
        fn ls_load(ptr: *mut u8, maxlen: u32) -> u32;
        fn ls_clear();
    }

    pub fn save(data: &str) {
        unsafe { ls_save(data.as_ptr(), data.len() as u32) }
    }

    pub fn load() -> Option<String> {
        let n = unsafe { ls_size() };
        if n == 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        let got = unsafe { ls_load(buf.as_mut_ptr(), n) };
        buf.truncate(got as usize);
        String::from_utf8(buf).ok()
    }

    pub fn clear() {
        unsafe { ls_clear() }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join("minicraft_save.txt")
    }

    pub fn save(data: &str) {
        let _ = std::fs::write(path(), data);
    }

    pub fn load() -> Option<String> {
        std::fs::read_to_string(path()).ok()
    }

    pub fn clear() {
        let _ = std::fs::remove_file(path());
    }
}

pub use backend::{clear, load, save};

pub struct SaveData {
    pub seed: u32,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fly: bool,
    pub run_mode: bool,
    pub edits: HashMap<(i32, i32, i32), u8>,
}

/// 行1: マジックとシード / 行2: プレイヤー / 以降: 編集差分(1行1ブロック)
pub fn serialize(d: &SaveData) -> String {
    let mut s = String::with_capacity(64 + d.edits.len() * 16);
    s.push_str(&format!("MC1 {}\n", d.seed));
    s.push_str(&format!(
        "{} {} {} {} {} {} {}\n",
        d.pos.x, d.pos.y, d.pos.z, d.yaw, d.pitch, d.fly as u8, d.run_mode as u8
    ));
    for (&(x, y, z), &b) in &d.edits {
        s.push_str(&format!("{} {} {} {}\n", x, y, z, b));
    }
    s
}

pub fn parse(s: &str) -> Option<SaveData> {
    let mut tok = s.split_whitespace();
    if tok.next()? != "MC1" {
        return None;
    }
    let seed: u32 = tok.next()?.parse().ok()?;
    let f = |t: &mut std::str::SplitWhitespace| -> Option<f32> { t.next()?.parse().ok() };
    let pos = vec3(f(&mut tok)?, f(&mut tok)?, f(&mut tok)?);
    let yaw = f(&mut tok)?;
    let pitch = f(&mut tok)?;
    let fly = tok.next()? == "1";
    let run_mode = tok.next()? == "1";
    let mut edits = HashMap::new();
    while let Some(t) = tok.next() {
        let x: i32 = t.parse().ok()?;
        let y: i32 = tok.next()?.parse().ok()?;
        let z: i32 = tok.next()?.parse().ok()?;
        let b: u8 = tok.next()?.parse().ok()?;
        // localStorage は手動書き換えや旧フォーマットが残り得る。トークン数が揃って
        // いても、範囲外の y(チャンク生成時に idx() で panic)や未定義のブロックIDは
        // 不正データなので、その行だけ捨ててセーブ全体の起動は守る
        if (0..CH).contains(&y) && b <= Block::FlowerYellow as u8 {
            edits.insert((x, y, z), b);
        }
    }
    Some(SaveData {
        seed,
        pos,
        yaw,
        pitch,
        fly,
        run_mode,
        edits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_roundtrip() {
        let mut edits = HashMap::new();
        edits.insert((5, 40, -3), 12u8);
        edits.insert((-100, 2, 7), 0u8);
        let d = SaveData {
            seed: 42,
            pos: vec3(1.5, 64.25, -8.0),
            yaw: 0.7,
            pitch: -0.2,
            fly: true,
            run_mode: false,
            edits,
        };
        let p = parse(&serialize(&d)).unwrap();
        assert_eq!(p.seed, d.seed);
        assert_eq!(p.pos, d.pos);
        assert_eq!(p.yaw, d.yaw);
        assert_eq!(p.pitch, d.pitch);
        assert_eq!(p.fly, d.fly);
        assert_eq!(p.run_mode, d.run_mode);
        assert_eq!(p.edits, d.edits);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse("").is_none());
        assert!(parse("XX 1").is_none());
        assert!(parse("MC1 1\n0 0 0 0 0 0 0\n1 2 3").is_none()); // 編集行が欠けている
    }

    #[test]
    fn parse_skips_out_of_range_rows() {
        // 構造は揃っていても値が不正な行は捨て、正常な行だけ残す(起動時 panic を防ぐ)
        let s = format!(
            "MC1 1\n0 0 0 0 0 0 0\n1 {CH} 1 3\n2 -5 2 3\n3 40 3 200\n4 40 4 10\n"
        );
        let p = parse(&s).unwrap();
        assert_eq!(p.edits.len(), 1); // y=CH(範囲外)/y=-5/block=200 は除外
        assert_eq!(p.edits.get(&(4, 40, 4)), Some(&10u8));
    }
}
