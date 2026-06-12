# minicraft 作業メモ

ブラウザで動く軽量マインクラフト風ゲーム(Rust製)。**v1 完成・動作確認済み。**

リポジトリ: https://github.com/mashPermz/minicraft (private)

## 遊び方

```sh
cd minicraft
./serve.sh        # ビルド + http://localhost:8080 で配信
```

ブラウザで http://localhost:8080 を開く → クリックでマウス捕捉して開始。

### 停止方法

- `./serve.sh` を実行したターミナルで **Ctrl+C**
- バックグラウンドで動いている場合(ポート8080を掴んでいるプロセスを終了):

```sh
lsof -ti:8080 | xargs kill
```

| 操作 | キー |
|---|---|
| 移動 / ジャンプ | WASD / Space |
| ダッシュ | Shift(前進中)/ R で持続ダッシュ切替 |
| 視点切替(一人称/三人称) | V |
| BGM 再生 / 停止 | M |
| 飛行モード切替 | F(Space上昇・Shift下降・Ctrl加速) |
| 破壊 / 設置 | 左クリック / 右クリック(長押しで連続) |
| ブロック選択 | 1〜9 またはホイール |
| 描画距離 | - / =(3〜7チャンク) |
| デバッグ表示 | Tab |
| マウス解放 | Esc |

## 技術スタック(確定)
- **Rust + macroquad 0.4.15**(miniquad 0.4.10)→ `wasm32-unknown-unknown` 直コンパイル。wasm-bindgen不要、**wasm 564KB**
- 配信物は `web/` の3ファイルのみ: index.html / mq_js_bundle.js / minicraft.wasm(静的サーバならどこでも動く)
- グラフィック: カスタムGLSL(地形・水・雲の3マテリアル)+ プロシージャル生成テクスチャ(外部アセットゼロ)

## 実装済み機能(v1)
- 無限風地形: 16x96x16チャンク逐次生成、fBmパーリン(自作noise.rs)、バイオーム(草原/森/砂漠/雪)、海(SEA=36)、木(チャンク境界またぎ対応)、石炭鉱石
- 描画: 隣接面カリング+**頂点AO**(フリップ補正付き)+面方向陰影+**深さ陰影**(掘った穴が暗くなる。列高さheightsを不透明ブロックのみで管理)
- 距離フォグ(水平距離基準)、**昼夜サイクル600秒**(空色・光色・朝夕の赤み)、空グラデーション(ピッチ連動)、太陽・月、**流れる雲レイヤー**(y=100、ドリフトはシームレス)
- 水: 半透明別パス(depth write off、遠→近ソート)、UV揺らぎ、水中フォグ+青オーバーレイ、泳ぎ
- プレイヤー: AABB衝突(軸分離+サブステップでトンネリング防止)、重力/ジャンプ/ダッシュ/飛行
- 編集: DDAレイキャスト(6ブロック)、破壊/設置(長押し連打)、設置時のプレイヤー干渉チェック、ハイライト枠
- UI: ホットバー(アトラスから描画)、照準、ロード画面+進捗バー、操作ガイド、デバッグ表示
- BGM(v1.1): 起動時にWAVをプロシージャル合成(audio.rs、外部アセットなし)、Mで再生/停止
- 三人称視点(v1.1): Vで切替。後方カメラ(地形めり込みは後退距離をレイで制限)+歩行スイング付き簡易ブロックマン(player.rs draw_model)
- パフォーマンス: フレーム予算式チャンク生成/メッシュ(通常3+2/フレーム)、粗い視錐台カリング、遠方チャンク破棄、u16インデックス上限で自動メッシュ分割

## ハマりポイント(重要)
- **wasmリンクエラー `undefined symbol: glBindTexture`**: 新しいrustc(1.87+)はwasmの未定義シンボルをエラーにする。miniquadのGL関数はJS側から実行時注入される設計なので `.cargo/config.toml` で `-C link-arg=--allow-undefined` が必要(設定済み)
- rustc 1.58→1.96更新時、旧`rls-preview`コンポーネントが`rustup update`を阻害 → `rustup component remove rls-preview`で解決
- macroquadのAPI確認は `~/.cargo/registry/src/*/macroquad-0.4.15/` のソースをgrepするのが確実(Vertex.colorは`[u8;4]`、Camera3Dのfovyはラジアン、upのデフォルトは+Z軸なので要指定)
- `mouse_delta_position()` は「前回-今回」を返す(符号が直感の逆)
- **miniquad 0.4.10 は depth_write=false で深度テストごと無効化**(gl.rs の apply_pipeline が glDisable(GL_DEPTH_TEST) を呼ぶ)。半透明パイプラインでも depth_write=true にしないと、水・雲が手前の地形を無視して描かれる
- **macroquad は1ドローコールあたり頂点10000/インデックス5000で黙ってクランプ**(QuadGl::geometry() が超過分を warn だけで切り捨て)。超えそうなメッシュは3200頂点で分割する(mesher.rs / sky.rs の Clouds)。`gl_set_drawcall_buffer_capacity` での拡大は、ドローコール毎にmaxサイズのGPUバッファが確保されるためメモリが膨らみ非推奨

## 調整履歴
- [2026-06-13] 葉・幹が真っ黒になる問題: heightsとAO遮蔽が葉を含んでいたため樹冠の下が「地下」扱いに → どちらも不透明ブロック限定に変更、AO_LUTも軟化 [0.48, 0.69, 0.85, 1.0]。修正後の見た目良好(docs/screenshot.png)
- [2026-06-13] 初回プレイテストFB対応(result_of_test.md): 太陽・月を四角に / 水・雲の深度問題修正 / 岸ジャンプ追加 / WALK 4.4→3.7 / R持続ダッシュ / V三人称 / M BGM。Cargo.toml に macroquad の `audio` feature 追加(web/mq_js_bundle.js は audio プラグイン同梱済みで変更不要)

## 検証状況
- ✅ ヘッドレスChrome(SwiftShader)でスクリーンショット検証: 草原・森・砂漠・雲・AO・空・ホットバー・ロード画面(docs/に保存)
- ✅ FB対応分のスクリーンショット検証: 四角い太陽・雲が木の奥に描画・三人称モデル・海岸のレイヤリング
- ⬜ 実ブラウザでの操作系(岸ジャンプ・BGM・ダッシュ切替・三人称、実GPUでのFPS)→ 人間の実プレイで再確認待ち

## v2候補(見送り分)
- 洞窟(3Dノイズ)、ライティング伝播(松明)、セーブ(localStorage)、音、ガラス以外の追加ブロック、水流、モブ、フルスクリーンボタン、モバイル対応(タッチ)

## 作業ログ
- [2026-06-13] 開始。Rust 1.96更新、設計、全8モジュール実装(~1900行)、wasmビルド成功(564KB)、ヘッドレスChromeで描画検証、葉の暗さ修正。v1完成。
- [2026-06-13] 初回プレイテストのFB対応(v1.1)。不具合5件(深度2件は miniquad の depth_write 挙動が根本原因)+機能3件(R/V/M)。audio.rs 追加で9モジュール、wasm 588KB。ヘッドレスChromeで検証済み、実プレイ再確認待ち。
