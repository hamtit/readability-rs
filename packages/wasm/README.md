# readability-rs-wasm

[readability-rs](https://github.com/hamtit/readability-rs)（Mozilla Readability 0.6.0 の Rust 移植）の WebAssembly 版。Node.js・Bun・Deno・ブラウザ・Cloudflare Workers などで動く。出力は Node-API 版と同じ。

```js
import createReadability from 'readability-rs-wasm';

const readability = await createReadability(); // dist/readability.wasm を読み込む
const article = readability.parse(html, { url: 'https://example.com/post/1' });
readability.isProbablyReaderable(html);
```

- 初期化のあとは同期 API。呼び出したスレッドで動くので、ブラウザでは Worker で使う
- `createReadability({ wasmBinary })` でバイト列を、`createReadability({ module })` でコンパイル済みの `WebAssembly.Module` を渡せる（バンドラーや Workers の wasm import 向け）
- スレッドは使わない（`parseAsync` はない）
