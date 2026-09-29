// Byte-level helpers without browser-extension dependencies (unit-tested in Node).

/** "%PDF-" must appear within the first 1024 bytes (PDF spec implementation note). */
export function hasPdfHeader(head: Uint8Array): boolean {
  const sig = [0x25, 0x50, 0x44, 0x46, 0x2d];
  outer: for (let i = 0; i + sig.length <= head.length && i < 1024; i++) {
    for (let j = 0; j < sig.length; j++) if (head[i + j] !== sig[j]) continue outer;
    return true;
  }
  return false;
}

/** SHA-256 of the exact PDF bytes, lowercase hex: the document's cache identity. */
export async function sha256Hex(buf: ArrayBuffer | Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest('SHA-256', buf as BufferSource));
  return Array.from(d, (b) => b.toString(16).padStart(2, '0')).join('');
}
