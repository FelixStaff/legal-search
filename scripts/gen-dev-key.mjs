// Creates config/extension.config.json with a fresh development public key.
// The public key pins a stable extension ID for "Load unpacked"; the private key
// is not needed for development or for the Chrome Web Store and is discarded.
//   node scripts/gen-dev-key.mjs [--force]
import { generateKeyPairSync, createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const file = join(dirname(fileURLToPath(import.meta.url)), '..', 'config', 'extension.config.json');
if (existsSync(file) && !process.argv.includes('--force')) {
  console.log('config exists:', JSON.parse(readFileSync(file, 'utf8')).extensionId);
  process.exit(0);
}
const { publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const der = publicKey.export({ type: 'spki', format: 'der' });
const extensionId = [...createHash('sha256').update(der).digest('hex').slice(0, 32)]
  .map((c) => String.fromCharCode(97 + parseInt(c, 16))).join('');
const config = {
  "_comment": "Single source of truth for the extension identity. See docs/architecture.md#extension-id.",
  hostName: 'com.legalsearch.host',
  extensionId,
  manifestKey: der.toString('base64'),
  webStoreUrl: '',
};
writeFileSync(file, JSON.stringify(config, null, 2) + '\n');
console.log('created', file, extensionId);
