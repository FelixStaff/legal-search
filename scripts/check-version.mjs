// Verifies that every place carrying the product version agrees.
//
//   node scripts/check-version.mjs            print the versions, fail if inconsistent
//   node scripts/check-version.mjs v1.1.0-rc1 also require them to match this git tag
//
// Mapping (SemVer pre-release "1.1.0-rc1"):
//   native/Cargo.toml version, extension/package.json version, manifest "version",
//   installer FileVersion  -> "1.1.0"      (numeric part)
//   installer AppVersion   -> "1.1.0-rc1"  (full version)
//   manifest "version_name"-> "1.1.0 RC1"  (display; optional for final releases)
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');

const cargo = /^version\s*=\s*"([^"]+)"/m.exec(read('native/Cargo.toml'))?.[1];
const pkg = JSON.parse(read('extension/package.json')).version;
const manifest = JSON.parse(read('extension/manifest.template.json'));
const iss = read('installer/LegalSearch.iss');
const appVersion = /#define AppVersion "([^"]+)"/.exec(iss)?.[1];
const fileVersion = /#define FileVersion "([^"]+)"/.exec(iss)?.[1];

const found = {
  'native/Cargo.toml': cargo,
  'extension/package.json': pkg,
  'manifest version': manifest.version,
  'manifest version_name': manifest.version_name ?? '(none)',
  'installer AppVersion': appVersion,
  'installer FileVersion': fileVersion,
};
for (const [k, v] of Object.entries(found)) console.log(`${k.padEnd(24)} ${v}`);

const errors = [];
const numeric = fileVersion;
if (!/^\d+\.\d+\.\d+$/.test(numeric ?? '')) errors.push(`installer FileVersion must be X.Y.Z, got ${numeric}`);
for (const [k, v] of [['native/Cargo.toml', cargo], ['extension/package.json', pkg], ['manifest version', manifest.version]]) {
  if (v !== numeric) errors.push(`${k} (${v}) != FileVersion (${numeric})`);
}
const [appNumeric, pre] = (appVersion ?? '').split('-');
if (appNumeric !== numeric) errors.push(`AppVersion (${appVersion}) does not start with ${numeric}`);
if (pre) {
  const expectedName = `${numeric} ${pre.toUpperCase()}`;
  if (manifest.version_name !== expectedName) errors.push(`manifest version_name should be "${expectedName}"`);
} else if (manifest.version_name && manifest.version_name !== numeric) {
  errors.push(`manifest version_name (${manifest.version_name}) should be absent or ${numeric} for a final release`);
}

const tag = process.argv[2];
if (tag) {
  const want = tag.replace(/^v/, '');
  if (want !== appVersion) errors.push(`tag ${tag} != installer AppVersion ${appVersion}`);
}

if (errors.length) {
  console.error('\nVersion mismatch:\n  ' + errors.join('\n  '));
  process.exit(1);
}
console.log(`\nOK: version ${appVersion}${tag ? ` matches tag ${tag}` : ''}`);
