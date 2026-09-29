// Wire types shared with the native engine (see native/src/protocol.rs).

export interface PageText {
  page: number;
  text: string;
}

export type Request =
  | { type: 'hello'; id: number }
  | { type: 'open'; id: number; sha256: string; filename: string; pageCount: number }
  | { type: 'index_begin'; id: number; sha256: string; filename: string; pageCount: number }
  | { type: 'index_pages'; id: number; sha256: string; pages: PageText[] }
  | { type: 'index_end'; id: number; sha256: string }
  | { type: 'search'; id: number; sha256: string; query: string; k?: number };

export type RequestBody = Request extends infer R ? (R extends { id: number } ? Omit<R, 'id'> : never) : never;

export type ErrorCode =
  | 'invalid_request'
  | 'not_indexed'
  | 'no_text'
  | 'indexing_in_progress'
  | 'model_missing'
  | 'internal';

export interface SearchHit {
  page: number;
  chunkIndex: number;
  text: string;
  score: number;
}

export type Response =
  | { type: 'hello'; id: number; version: string; protocol: number }
  | { type: 'opened'; id: number; sha256: string; status: 'ready' | 'needs_index'; chunkCount: number }
  | { type: 'ack'; id: number }
  | { type: 'progress'; id: number; done: number; total: number; pct: number }
  | { type: 'ready'; id: number; sha256: string; chunkCount: number }
  | { type: 'results'; id: number; results: SearchHit[]; mode?: 'dense' | 'hybrid'; elapsedMs: number }
  | { type: 'error'; id: number; code: ErrorCode };

/** Error raised by the client for a host `error` response. */
export class HostError extends Error {
  constructor(public code: ErrorCode) {
    super(code);
  }
}

/** Raised when the native host cannot be started or disconnects. */
export class HostUnavailable extends Error {
  constructor(public notInstalled: boolean, detail: string) {
    super(detail);
  }
}
