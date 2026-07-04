// BGM/効果音: 起動時にプロシージャル合成する短い音(外部アセットなし)
// 22.05kHz mono 16bit の WAV を組み立てて macroquad の audio に渡す

use macroquad::audio::{load_sound_from_bytes, Sound};

const RATE: usize = 22050;
const BPM: f32 = 72.0;

fn hz(semi: f32) -> f32 {
    // A4=440Hz 基準の半音
    440.0 * (2.0f32).powf(semi / 12.0)
}

/// 1音を加算合成(正弦波+弱い倍音)
fn add_note(buf: &mut [f32], start: f32, dur: f32, semi: f32, amp: f32, pluck: bool) {
    let f = hz(semi);
    let s0 = (start * RATE as f32) as usize;
    let n = (dur * RATE as f32) as usize;
    for i in 0..n {
        let idx = s0 + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f32 / RATE as f32;
        let env = if pluck {
            // 撥弦風: 速い立ち上がり+指数減衰
            (1.0 - (-t * 60.0).exp()) * (-t * 2.8).exp()
        } else {
            // パッド風: ゆっくり立ち上がり、終端でフェード
            (t / 0.7).min(1.0) * ((dur - t) / 0.9).clamp(0.0, 1.0)
        };
        let w = t * f * std::f32::consts::TAU;
        let s = w.sin() + 0.35 * (w * 2.0).sin() + 0.12 * (w * 3.0).sin();
        buf[idx] += s * env * amp;
    }
}

/// 8小節(C - Am - F - G ×2)の穏やかなループ曲を合成
fn compose() -> Vec<f32> {
    let beat = 60.0 / BPM;
    let bar = beat * 4.0;
    let len = (bar * 8.0 * RATE as f32) as usize;
    let mut buf = vec![0.0f32; len];

    // コードパッド(半音はA4基準: C3 C4 E4 G4 など)
    const CHORDS: [[f32; 4]; 4] = [
        [-21.0, -9.0, -5.0, -2.0],   // C
        [-24.0, -12.0, -9.0, -5.0],  // Am
        [-28.0, -16.0, -12.0, -9.0], // F
        [-26.0, -14.0, -10.0, -7.0], // G
    ];
    for bar_no in 0..8 {
        let ch = &CHORDS[bar_no % 4];
        let t0 = bar_no as f32 * bar;
        for (k, &s) in ch.iter().enumerate() {
            let amp = if k == 0 { 0.10 } else { 0.06 };
            add_note(&mut buf, t0, bar, s, amp, false);
        }
    }

    // メロディ(Cペンタトニック、1拍ごと、Noneは休符)
    const MEL: [Option<f32>; 32] = [
        Some(7.0),
        Some(10.0),
        Some(12.0),
        Some(10.0),
        Some(7.0),
        Some(5.0),
        Some(3.0),
        None,
        Some(3.0),
        Some(5.0),
        Some(7.0),
        Some(10.0),
        Some(5.0),
        Some(7.0),
        Some(5.0),
        None,
        Some(10.0),
        Some(7.0),
        Some(5.0),
        Some(3.0),
        Some(0.0),
        Some(3.0),
        Some(5.0),
        None,
        Some(7.0),
        Some(5.0),
        Some(3.0),
        Some(0.0),
        Some(-2.0),
        Some(0.0),
        Some(3.0),
        None,
    ];
    for (i, m) in MEL.iter().enumerate() {
        if let Some(s) = m {
            add_note(&mut buf, i as f32 * beat, beat * 1.7, *s, 0.085, true);
        }
    }

    // 軽いエコー(ループ境界をまたいで折り返す)
    let d = (beat * 0.75 * RATE as f32) as usize;
    for i in 0..len {
        let j = (i + len - d) % len;
        buf[i] += buf[j] * 0.30;
    }

    // ピーク正規化
    let peak = buf.iter().fold(1e-6f32, |a, &b| a.max(b.abs()));
    let g = 0.55 / peak;
    for s in &mut buf {
        *s *= g;
    }
    buf
}

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let n = samples.len() * 2;
    let mut w = Vec::with_capacity(44 + n);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((36 + n) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // fmtチャンクサイズ
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&(RATE as u32).to_le_bytes());
    w.extend_from_slice(&((RATE * 2) as u32).to_le_bytes()); // byte rate
    w.extend_from_slice(&2u16.to_le_bytes()); // block align
    w.extend_from_slice(&16u16.to_le_bytes()); // bits
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(n as u32).to_le_bytes());
    for &s in samples {
        w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    w
}

pub async fn load_bgm() -> Option<Sound> {
    load_sound_from_bytes(&wav_bytes(&compose())).await.ok()
}

// --- 効果音(SFX) ---
// BGMと同じ加算合成の手法で短いノイズ/トーン波形を作る。ホワイトノイズは
// 座標無しの決定的LCGで生成(依存クレート追加なし)。

/// 決定的ホワイトノイズ生成器(xorshift32)
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// ノイズバースト+指数減衰(破壊音・足音・着地音のベース)
fn noise_burst(dur: f32, decay: f32, seed: u32, lowpass: f32) -> Vec<f32> {
    let n = (dur * RATE as f32) as usize;
    let mut rng = Noise(seed | 1);
    let mut buf = vec![0.0f32; n];
    let mut prev = 0.0f32;
    for (i, s) in buf.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let raw = rng.next();
        // 簡易ローパス(こもった音にして高域のジリジリ感を抑える)
        prev += (raw - prev) * lowpass;
        *s = prev * (-t * decay).exp();
    }
    buf
}

/// 短い正弦波トーン(設置音・着水音の基調)
fn tone_burst(dur: f32, freq0: f32, freq1: f32, decay: f32) -> Vec<f32> {
    let n = (dur * RATE as f32) as usize;
    let mut buf = vec![0.0f32; n];
    let mut phase = 0.0f32;
    for (i, s) in buf.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let f = freq0 + (freq1 - freq0) * (t / dur).min(1.0);
        phase += f * std::f32::consts::TAU / RATE as f32;
        *s = phase.sin() * (-t * decay).exp();
    }
    buf
}

fn normalize(buf: &mut [f32], peak_target: f32) {
    let peak = buf.iter().fold(1e-6f32, |a, &b| a.max(b.abs()));
    let g = peak_target / peak;
    for s in buf.iter_mut() {
        *s *= g;
    }
}

fn mix_into(dst: &mut [f32], src: &[f32]) {
    for (i, &s) in src.iter().enumerate() {
        if i < dst.len() {
            dst[i] += s;
        }
    }
}

/// ブロック破壊: 短く硬いノイズバースト
fn compose_break() -> Vec<f32> {
    let mut buf = noise_burst(0.16, 34.0, 0xB1E4, 0.5);
    normalize(&mut buf, 0.5);
    buf
}

/// ブロック設置: こもったノイズ+低めのトーンで「コツン」とした感触
fn compose_place() -> Vec<f32> {
    let mut buf = noise_burst(0.12, 40.0, 0x9A21, 0.35);
    let tone = tone_burst(0.1, 180.0, 120.0, 26.0);
    mix_into(&mut buf, &tone);
    normalize(&mut buf, 0.45);
    buf
}

/// 足音: 非常に短いノイズパルス(周期的に鳴らす前提の単発音)
fn compose_step() -> Vec<f32> {
    let mut buf = noise_burst(0.09, 55.0, 0x5C3D, 0.4);
    normalize(&mut buf, 0.32);
    buf
}

/// 着地音: 足音より低くやや長め
fn compose_land() -> Vec<f32> {
    let mut buf = noise_burst(0.14, 30.0, 0x77F0, 0.3);
    let tone = tone_burst(0.12, 90.0, 60.0, 22.0);
    mix_into(&mut buf, &tone);
    normalize(&mut buf, 0.5);
    buf
}

/// 水しぶき(入水・出水共通、上りトーン+ノイズ)
fn compose_splash() -> Vec<f32> {
    let mut buf = noise_burst(0.28, 12.0, 0x44D2, 0.25);
    let tone = tone_burst(0.24, 260.0, 520.0, 9.0);
    mix_into(&mut buf, &tone);
    normalize(&mut buf, 0.5);
    buf
}

/// モブを叩く音: 硬めのパンチ音(高めのノイズ+短いトーン)
fn compose_hit() -> Vec<f32> {
    let mut buf = noise_burst(0.1, 45.0, 0x2E71, 0.6);
    let tone = tone_burst(0.08, 500.0, 260.0, 30.0);
    mix_into(&mut buf, &tone);
    normalize(&mut buf, 0.5);
    buf
}

/// 全効果音をまとめて保持する
pub struct Sfx {
    pub break_block: Option<Sound>,
    pub place_block: Option<Sound>,
    pub step: Option<Sound>,
    pub land: Option<Sound>,
    pub splash: Option<Sound>,
    pub hit: Option<Sound>,
}

pub async fn load_sfx() -> Sfx {
    Sfx {
        break_block: load_sound_from_bytes(&wav_bytes(&compose_break())).await.ok(),
        place_block: load_sound_from_bytes(&wav_bytes(&compose_place())).await.ok(),
        step: load_sound_from_bytes(&wav_bytes(&compose_step())).await.ok(),
        land: load_sound_from_bytes(&wav_bytes(&compose_land())).await.ok(),
        splash: load_sound_from_bytes(&wav_bytes(&compose_splash())).await.ok(),
        hit: load_sound_from_bytes(&wav_bytes(&compose_hit())).await.ok(),
    }
}
