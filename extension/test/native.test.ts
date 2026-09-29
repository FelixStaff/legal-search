import { test } from 'node:test';
import assert from 'node:assert/strict';
import { NativeClient, isNotInstalledError, type PortLike } from '../src/native.js';
import { HostError, HostUnavailable } from '../src/messages.js';

class FakePort implements PortLike {
  sent: any[] = [];
  private msgFns: ((m: unknown) => void)[] = [];
  private discFns: (() => void)[] = [];
  onMessage = { addListener: (f: (m: unknown) => void) => void this.msgFns.push(f) };
  onDisconnect = { addListener: (f: () => void) => void this.discFns.push(f) };
  postMessage(m: unknown) { this.sent.push(m); }
  disconnect() {}
  reply(m: unknown) { for (const f of this.msgFns) f(m); }
  drop() { for (const f of this.discFns) f(); }
}

const SHA = 'a'.repeat(64);

test('correlates responses by id and forwards progress', async () => {
  const port = new FakePort();
  const c = new NativeClient(() => port);
  const progress: number[] = [];
  const p = c.request({ type: 'index_end', sha256: SHA }, (e) => progress.push(e.pct));
  const id = port.sent[0].id;
  assert.equal(port.sent[0].type, 'index_end');
  port.reply({ type: 'progress', id, done: 1, total: 2, pct: 50 });
  port.reply({ type: 'ack', id: id + 99 }); // unrelated id is ignored
  port.reply({ type: 'ready', id, sha256: SHA, chunkCount: 2 });
  const r = await p;
  assert.equal(r.type, 'ready');
  assert.deepEqual(progress, [50]);
});

test('host error codes become HostError', async () => {
  const port = new FakePort();
  const c = new NativeClient(() => port);
  const p = c.request({ type: 'search', sha256: SHA, query: 'x' });
  port.reply({ type: 'error', id: port.sent[0].id, code: 'not_indexed' });
  await assert.rejects(p, (e) => e instanceof HostError && e.code === 'not_indexed');
});

test('unparseable-message error (id 0) fails the oldest request', async () => {
  const port = new FakePort();
  const c = new NativeClient(() => port);
  const p = c.request({ type: 'hello' });
  port.reply({ type: 'error', id: 0, code: 'invalid_request' });
  await assert.rejects(p, HostError);
});

test('malformed messages are ignored', async () => {
  const port = new FakePort();
  const c = new NativeClient(() => port);
  const p = c.request({ type: 'hello' });
  for (const junk of [null, 'x', 42, { id: 'x' }, { type: 'hello' }]) port.reply(junk);
  port.reply({ type: 'hello', id: port.sent[0].id, version: '1', protocol: 1 });
  assert.equal((await p).type, 'hello');
});

test('host not installed: pending requests fail with notInstalled, then reconnects', async () => {
  let n = 0;
  const ports: FakePort[] = [];
  let lastError: string | undefined = 'Specified native messaging host not found.';
  const c = new NativeClient(() => { n++; const p = new FakePort(); ports.push(p); return p; }, () => lastError);
  const p = c.request({ type: 'hello' });
  ports[0].drop();
  await assert.rejects(p, (e) => e instanceof HostUnavailable && e.notInstalled);
  lastError = undefined;
  const p2 = c.request({ type: 'hello' });
  assert.equal(n, 2);
  ports[1].reply({ type: 'hello', id: ports[1].sent[0].id, version: '1', protocol: 1 });
  await p2;
});

test('isNotInstalledError', () => {
  assert.ok(isNotInstalledError('Specified native messaging host not found.'));
  assert.ok(isNotInstalledError('Access to the specified native messaging host is forbidden.'));
  assert.ok(!isNotInstalledError('Native host has exited.'));
  assert.ok(!isNotInstalledError(undefined));
});
