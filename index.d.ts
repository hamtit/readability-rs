export interface ParseOptions {
  /** Document URL, used to resolve relative links and images (and `<base href>`). */
  url?: string;
  /** Maximum number of elements to parse; parsing fails above it. Default 0 (no limit). */
  maxElemsToParse?: number;
  /** Number of top candidates to consider. Default 5. */
  nbTopCandidates?: number;
  /** Number of characters an article must have. Default 500. */
  charThreshold?: number;
  /** Classes to keep in addition to `page` (when `keepClasses` is false). */
  classesToPreserve?: string[];
  /** Keep all classes. Default false. */
  keepClasses?: boolean;
  /** Do not read metadata from JSON-LD. Default false. */
  disableJSONLD?: boolean;
  /**
   * Embeds (iframe / object / embed) with an attribute matching this are kept.
   * A RegExp is converted to its source (flags i, m, s are kept) and compiled
   * with the Rust `regex` crate, so lookaround and backreferences are not supported.
   */
  allowedVideoRegex?: RegExp | string;
  /** Added to the link density thresholds. Default 0. */
  linkDensityModifier?: number;
}

export interface Article {
  /** Article title. */
  title: string;
  /** Author metadata. */
  byline: string | null;
  /** Content direction (`dir` attribute). */
  dir: string | null;
  /** `lang` attribute of `<html>`. */
  lang: string | null;
  /** HTML of the article: `<div id="readability-page-1" class="page">...</div>`. */
  content: string;
  /** Text content of the article. */
  textContent: string;
  /** Length of `textContent` in UTF-16 code units (JavaScript string length). */
  length: number;
  /** Article description, or the first paragraph. */
  excerpt: string | null;
  /** Name of the site. */
  siteName: string | null;
  /** Published time. */
  publishedTime: string | null;
}

export interface ReaderableOptions {
  /** The minimum node content length used to decide if the document is readerable. Default 140. */
  minContentLength?: number;
  /** The minimum cumulated score used to determine if the document is readerable. Default 20. */
  minScore?: number;
}

/** Extracts the article. Returns `null` when none is found. Runs on the calling thread. */
export function parse(html: string, options?: ParseOptions): Article | null;

/** Same as `parse`, on the Node-API thread pool. */
export function parseAsync(html: string, options?: ParseOptions, signal?: AbortSignal): Promise<Article | null>;

/** Decides whether or not the document is reader-able without parsing the whole thing. */
export function isProbablyReaderable(html: string, options?: ReaderableOptions): boolean;

export function versions(): { readabilityRs: string; readabilityJs: string };
