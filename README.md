# readability-rs

[Mozilla Readability](https://github.com/mozilla/readability)（Firefox のリーダービューで使われている記事本文の抽出ライブラリ）を Rust に移植したもの。Bun と Node.js から使える Node-API アドオン、ブラウザや Workers でも動く WebAssembly 版、Rust のライブラリとして使える。

Readability.js は DOM（jsdom、linkedom など）の上で動くので、抽出そのものより DOM の構築と操作に時間とメモリを使う。HTML の解析から本文の抽出までを Rust で行い、結果を変えずにどこまで速くなるかを確かめるために作った。測定結果は [ベンチマーク](#ベンチマーク) を参照。

## 互換性

- 移植元は Readability.js **0.6.0**（npm で配布されている最新版。タグ `0.6.0` = `04fd32f`）。関数ごとに対応する形で移植しており（`_grabArticle` → `grab_article` など）、上流のコメントも残している
- 出力（`content`、`textContent`、`title`、`byline`、`excerpt`、`siteName`、`dir`、`lang`、`publishedTime`、`length`）は、jsdom 上の Readability.js と**文字単位で同一**になるようにしている。確認した範囲:
  - 上流のテストページ 128 件（`test/test-pages`）: 上流のテスト（`test-readability.js` と `test-isProbablyReaderable.js`）と同じ比較ですべて通る（`tests/test_pages.rs`）
  - 同じ 128 件と、日本語・英語のニュースサイト、技術ブログ、Wikipedia などの実在の記事 113 件: jsdom 30.1.3 + Readability.js 0.6.0 の出力と全フィールドが完全一致（`bench/compare.ts`）
- そのために次の点を JavaScript に合わせている
  - 文字数はすべて UTF-16 のコード単位で数える（JavaScript の `String.prototype.length`。閾値の「25 文字」などが同じ意味になる）
  - 空白は ECMAScript の `\s` と `trim()` の定義（U+3000 や U+FEFF を含む）。`\W` や `\b` は ASCII 基準
  - HTML の解析は html5ever（仕様どおりの HTML パーサー）で、jsdom と同じくスクリプト無効として扱う（`<noscript>` の中身を要素として解析する）
  - URL の解決は WHATWG URL 標準（`url` クレート。ファイル URL のドライブ文字 `C|` → `C:` の正規化だけ補っている）
  - HTML のシリアライズは HTML 標準のフラグメントシリアライズ（属性値の `<` `>` もエスケープする現行の仕様）
- linkedom など仕様どおりでない DOM の上で Readability.js を動かした場合とは、DOM の木の形が違うことがあるので、結果が一致しないことがある

### Readability.js と違うところ

- 入力は DOM ではなく HTML 文字列。渡した文書は変更されない（Readability.js は渡された `document` を書き換える）
- `serializer` と `debug` オプションはない。`content` は常に `innerHTML`
- `allowedVideoRegex` は Rust の `regex` クレートの構文で解釈する（JavaScript の `RegExp` を渡すと `source` とフラグ `i` `m` `s` を引き継ぐ。先読み・後方参照は使えない）
- 値がないフィールドは常に `null`（Readability.js では `excerpt` が `undefined` になることがある）

## 使い方

```ts
import { parse, parseAsync, isProbablyReaderable } from 'readability-rs';

const article = parse(html, { url: 'https://example.com/post/1' });
// article: { title, byline, dir, lang, content, textContent, length, excerpt, siteName, publishedTime } | null

await parseAsync(html, { url }); // スレッドプール（libuv / Bun の Node-API ワーカー）で実行し、JavaScript のスレッドを止めない
isProbablyReaderable(html); // isProbablyReaderable(document) と同じ判定
```

`url` は相対 URL の解決（`<base href>` を含む）に使う。省略すると相対 URL はそのまま残る。その他のオプション（`charThreshold`、`nbTopCandidates`、`classesToPreserve`、`keepClasses`、`disableJSONLD`、`maxElemsToParse`、`linkDensityModifier`）は Readability.js と同じ。型は `index.d.ts` を参照。

WebAssembly 版（`readability-rs-wasm`）は、初期化のあと同じ API を同期で呼ぶ（スレッドを使わないので `parseAsync` はない）。Node-API アドオンが使えない環境（musl、macOS、Windows、ブラウザ、Cloudflare Workers など）向け。

```ts
import createReadability from 'readability-rs-wasm';

const readability = await createReadability(); // dist/readability.wasm を読み込む。{ wasmBinary } / { module } でも渡せる
readability.parse(html, { url });
```

Rust からは:

```rust
let article = readability_rs::parse(html, Some("https://example.com/post/1"), &Default::default())?;
```

## ビルド

Node-API アドオンは Linux（glibc / musl、x64 / arm64）向け。必要なものは Rust だけ（依存はすべて Rust のクレートで、C のライブラリを使わない）。

```bash
./scripts/build.sh                 # readability-rs.linux-<arch>-<libc>.node を作る
./scripts/build-wasm.sh out/wasm   # WebAssembly 版（rustup target add wasm32-unknown-unknown が必要）
./scripts/fetch-test-pages.sh      # 上流のテストページ（約 30MB、コミットしていない）を tests/test-pages/ に取得
cargo test --release --no-default-features
bun install && bun test tests/
(cd bench && bun install && bun compare.ts)   # Readability.js との出力比較（アドオンと、ビルド済みなら wasm 版。PAGES_DIR=<dir> で手元の HTML も比べる）
```

## リリース

リリースは自動で、バージョン番号の置き場所は `Cargo.toml` の `version` だけ（`package.json` には書かない。`Cargo.lock` の `readability-rs` の版は `cargo update -p readability-rs` で揃える。CI が食い違いを検出する）。

1. PR で `Cargo.toml`（と `Cargo.lock`）の `version` を上げる。
2. `main` にマージする。CI が通ったあと、タグ `v<version>` がまだなければ、タグと GitHub Release を作る。

`main` への push でもタグがすでにあれば、ビルドとテストだけを行う。`v<version>` のタグを手で push してもリリースされる（`Cargo.toml` と合わない名前のタグは拒否）。PR と手動実行では、ビルド・テスト・パッケージ作成までを行い、パッケージはワークフローのアーティファクトに残る。

Release には 4 つの npm tarball が付く。npm レジストリには公開していないので、Release の URL で依存する。

| ファイル | 内容 |
|---|---|
| `readability-rs-linux-x64-<version>.tgz` | Linux x64（glibc）のアドオンだけ |
| `readability-rs-linux-arm64-<version>.tgz` | Linux arm64（glibc）のアドオンだけ |
| `readability-rs-<version>.tgz` | x64 と arm64 を 1 つにまとめたもの |
| `readability-rs-wasm-<version>.tgz` | WebAssembly 版（[packages/wasm](packages/wasm/README.md)） |

x64 / arm64 の単体パッケージは `os` / `cpu` を持つので、`optionalDependencies` に両方並べればパッケージマネージャーが合う方だけ入れる。アドオンは Debian bookworm（`oven/bun:*-debian` と同じ glibc）でビルドする。どのパッケージも依存クレートのライセンス文書（`licenses/`）を含む。

```sh
npm install https://github.com/hamtit/readability-rs/releases/download/v0.1.0/readability-rs-0.1.0.tgz
```

CI では、x64 と arm64 のそれぞれで、Rust のテスト（上流のテストページを含む）、アドオンのテスト、Readability.js との出力比較（上流のテストページ）を行う。WebAssembly 版もビルドして同じ出力比較を行い、パッケージをインストールして動作を確かめる。

## ベンチマーク

同じ HTML 文字列から記事を取り出すまで（DOM の構築を含む）を、Readability.js 0.6.0 + jsdom 30.1.3（上流のテストと同じ構成）、Readability.js 0.6.0 + linkedom 0.18.14（軽量な DOM）、このライブラリで比べた。比較側の実装は `bench/run.mjs`。

- 環境: Intel Xeon 2.1 GHz 4 コア、Linux (glibc)、Bun 1.4.4 / Node.js 22.23.4
- 入力: 上流のテストページ 128 件（25.5 MB）と、日本語・英語の実在の記事 113 件（21.8 MB。ニュースサイト、技術ブログ、Wikipedia など。手元に保存したもので、リポジトリには含めていない。同じ測定は任意の HTML を置いたディレクトリを `PAGES_DIR` に指定して行える）
- 方法: 全ページを 1 件ずつ順に処理する 1 周を 5 回測った中央値（最初の 1 周は除外）。1 回ごとに新しいプロセスで 3 回測った中央値（`bench/run-all.mjs`）。`parseAsync` は 4 並列。メモリは RSS のピーク (VmHWM)
- 出力は 3 つとも同じ入力に対して比べており、このライブラリと jsdom 版は全件で一致する（[互換性](#互換性)）

### 実在の記事 113 件

| | 全体の時間 | 1 件の中央値 / p95 | ピーク RSS |
|---|---|---|---|
| Bun: jsdom + Readability.js | 14.67 s | 87.0 / 253 ms | 5523 MB |
| Bun: linkedom + Readability.js | 3.94 s | 17.6 / 78 ms | 500 MB |
| Bun: **readability-rs `parse`** | **0.52 s（jsdom 比 -96%、linkedom 比 -87%）** | **2.9 / 6.9 ms** | **122 MB** |
| Bun: readability-rs `parseAsync`（4 並列） | 0.18 s | | 168 MB |
| Node.js: jsdom + Readability.js | 18.91 s | 101.5 / 328 ms | 5277 MB |
| Node.js: linkedom + Readability.js | 4.19 s | 20.9 / 77 ms | 551 MB |
| Node.js: **readability-rs `parse`** | **0.53 s** | **3.4 / 7.3 ms** | **146 MB** |
| Node.js: readability-rs `parseAsync`（4 並列） | 0.21 s | | 194 MB |

### WebAssembly 版

アドオンとは別の時点で測ったので、同じ条件でアドオンも測り直して並べた（アドオンの値は上の表とほぼ同じ）。

| | Bun: アドオン | Bun: wasm | Node.js: アドオン | Node.js: wasm |
|---|---|---|---|---|
| 実在の記事 113 件 | 0.54 s / 125 MB | 0.64 s / 221 MB | 0.56 s / 147 MB | 0.76 s / 208 MB |
| 上流のテストページ 128 件 | 0.45 s / 128 MB | 0.51 s / 203 MB | 0.54 s / 157 MB | 0.69 s / 221 MB |

（全体の時間 / ピーク RSS）

wasm 版はアドオンより 14〜36% 遅いが、linkedom + Readability.js（Bun で 3.94 s / 500 MB）より 6 倍ほど速い。メモリが多いのは、wasm の線形メモリが一度伸びると縮まないため。

### 上流のテストページ 128 件

| | Bun | Node.js |
|---|---|---|
| jsdom + Readability.js | 19.83 s / 7046 MB | 24.49 s / 5868 MB |
| linkedom + Readability.js | 2.54 s / 334 MB | 3.25 s / 474 MB |
| **readability-rs `parse`** | **0.44 s / 127 MB** | **0.56 s / 156 MB** |
| readability-rs `parseAsync`（4 並列） | 0.17 s / 146 MB | 0.18 s / 191 MB |

（全体の時間 / ピーク RSS）

### 結果の読み方

- 1 スレッドどうしで、jsdom 版の 28〜45 倍、linkedom 版の 6〜8 倍速い。時間の内訳はおおよそ HTML の解析（html5ever）が 6 割、抽出が 4 割。Readability.js 側は DOM 操作と `textContent` の繰り返し計算が大半を占める
- 抽出のアルゴリズムは同じ（計算量も同じ）。速さの差は、DOM を配列上のノードとして持つこと（参照カウントや GC がない）、テキストノードごとの長さ・空白・コンマの集計を一度だけ計算して使い回すこと（テキストは書き換えないので結果は変わらない）、`_getLinkDensity` を部分木の 1 回の走査で計算することから来る
- メモリは jsdom 版の 2〜3%、linkedom 版の 24〜38%。抽出が終わると文書はまとめて解放される
- `parseAsync` は Node-API のスレッドプールで動くので、JavaScript のスレッドを止めずに並列に処理できる。並列にしない場合でも、HTML 文字列の受け渡し（JavaScript の文字列 ↔ UTF-8）の分だけ Rust だけで動かすより遅い（`cargo run --release --no-default-features --example bench -- <HTML のディレクトリ>` で Rust 単体の時間が出る）

## ライセンス

Apache License 2.0（[LICENSE](LICENSE)）。Readability.js（Copyright (c) 2010 Arc90 Inc, Copyright (c) 2010-2026 Mozilla and Contributors）からの派生物で、[NOTICE](NOTICE) を参照。
