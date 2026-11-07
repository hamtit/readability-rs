import type { Article, ParseOptions, ReaderableOptions } from '../../index.js';

export type { Article, ParseOptions, ReaderableOptions };

export interface Readability {
  /** Extracts the article. Returns `null` when none is found. Runs synchronously. */
  parse(html: string, options?: ParseOptions): Article | null;
  /** Decides whether or not the document is reader-able without parsing the whole thing. */
  isProbablyReaderable(html: string, options?: ReaderableOptions): boolean;
  versions(): { readabilityRs: string; readabilityJs: string };
}

export interface CreateOptions {
  /** The module bytes, instead of loading dist/readability.wasm. */
  wasmBinary?: ArrayBuffer | Uint8Array;
  /** An already compiled module (e.g. a Cloudflare Workers wasm import). */
  module?: WebAssembly.Module;
}

export default function createReadability(options?: CreateOptions): Promise<Readability>;
