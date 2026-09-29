// Request/response client over a Chrome Native Messaging port.
// The port is injected so the client can be unit-tested without Chrome.

import { HostError, HostUnavailable, type RequestBody, type Response } from './messages.js';

export interface PortLike {
  postMessage(msg: unknown): void;
  disconnect(): void;
  onMessage: { addListener(fn: (msg: unknown) => void): void };
  onDisconnect: { addListener(fn: () => void): void };
}

type ProgressEvent = Extract<Response, { type: 'progress' }>;

interface Pending {
  resolve: (r: Response) => void;
  reject: (e: Error) => void;
  onProgress?: (p: ProgressEvent) => void;
}

/** Chrome's wording when the host manifest/registry entry is missing or forbids us. */
export function isNotInstalledError(message: string | undefined): boolean {
  return !!message && /not found|forbidden|specified native messaging host/i.test(message);
}

function isResponse(m: unknown): m is Response {
  return (
    typeof m === 'object' && m !== null &&
    typeof (m as { type?: unknown }).type === 'string' &&
    typeof (m as { id?: unknown }).id === 'number'
  );
}

export class NativeClient {
  private port: PortLike | null = null;
  private nextId = 1;
  private pending = new Map<number, Pending>();

  constructor(
    private readonly connect: () => PortLike,
    private readonly lastError: () => string | undefined = () => undefined,
  ) {}

  private ensurePort(): PortLike {
    if (this.port) return this.port;
    const port = this.connect();
    port.onMessage.addListener((m) => this.onMessage(m));
    port.onDisconnect.addListener(() => {
      const detail = this.lastError() ?? 'native host disconnected';
      if (this.port === port) this.port = null;
      const err = new HostUnavailable(isNotInstalledError(detail), detail);
      for (const p of this.pending.values()) p.reject(err);
      this.pending.clear();
    });
    this.port = port;
    return port;
  }

  private onMessage(m: unknown): void {
    if (!isResponse(m)) return; // ignore anything malformed
    if (m.id === 0 && m.type === 'error') {
      // The host could not parse a message: fail the oldest request.
      const first = this.pending.keys().next();
      if (!first.done) this.settle(first.value, m);
      return;
    }
    const p = this.pending.get(m.id);
    if (!p) return;
    if (m.type === 'progress') {
      p.onProgress?.(m);
      return;
    }
    this.settle(m.id, m);
  }

  private settle(id: number, m: Response): void {
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    if (m.type === 'error') p.reject(new HostError(m.code));
    else p.resolve(m);
  }

  /** Sends a request and resolves with its final (non-progress) response. */
  request(body: RequestBody, onProgress?: (p: ProgressEvent) => void): Promise<Response> {
    return new Promise((resolve, reject) => {
      let port: PortLike;
      try {
        port = this.ensurePort();
      } catch (e) {
        reject(new HostUnavailable(true, String(e)));
        return;
      }
      const id = this.nextId++;
      this.pending.set(id, { resolve, reject, onProgress });
      try {
        port.postMessage({ ...body, id });
      } catch (e) {
        this.pending.delete(id);
        reject(new HostUnavailable(false, String(e)));
      }
    });
  }

  close(): void {
    this.port?.disconnect();
    this.port = null;
  }
}
