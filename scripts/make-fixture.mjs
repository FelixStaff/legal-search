// Generates reproducible test PDFs with zero dependencies (byte-identical on every run).
//
//   node scripts/make-fixture.mjs
//
// Outputs (tests/fixtures/):
//   fixture.pdf   12 pages of Spanish legal text from corpus.json (text layer)
//   scanned.pdf   3 pages with only vector drawings, no text (simulates a scan)
//   bench.pdf     300 pages (corpus pages repeated with numbering) for timing
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = join(root, 'tests', 'fixtures');
const corpus = JSON.parse(readFileSync(join(outDir, 'corpus.json'), 'utf8'));

const W = 595, H = 842, MARGIN = 60, FONT = 11, LEADING = 15;
const MAX_LINE_PT = W - 2 * MARGIN;

// Approximate Helvetica advance widths (em units): keeps every glyph inside the
// page, otherwise PDF viewers/extractors legitimately drop off-page text.
function widthPt(s) {
  let em = 0;
  for (const ch of s) {
    if (ch === ' ') em += 0.278;
    else if (/[A-ZÁÉÍÓÚÑ]/.test(ch)) em += ch === 'M' || ch === 'W' ? 0.833 : 0.722;
    else if (/[0-9]/.test(ch)) em += 0.556;
    else if (/[.,:;'!|il]/.test(ch)) em += 0.278;
    else if (/[mw]/.test(ch)) em += 0.833;
    else em += 0.556;
  }
  return em * FONT;
}

function wrap(text) {
  const lines = [];
  let cur = '';
  for (const w of text.split(/\s+/)) {
    if (cur && widthPt(cur + ' ' + w) > MAX_LINE_PT) { lines.push(cur); cur = w; }
    else cur = cur ? cur + ' ' + w : w;
  }
  if (cur) lines.push(cur);
  return lines;
}

// PDF literal string in WinAnsi (== Latin-1 for the Spanish characters we use).
function pdfString(s) {
  for (const ch of s) {
    if (ch.codePointAt(0) > 0xff) throw new Error(`char not encodable: ${ch}`);
  }
  return '(' + s.replace(/\\/g, '\\\\').replace(/\(/g, '\\(').replace(/\)/g, '\\)') + ')';
}

function textPage(text) {
  const lines = wrap(text);
  let s = `BT /F1 ${FONT} Tf ${LEADING} TL ${MARGIN} ${H - MARGIN} Td\n`;
  for (const l of lines) s += `${pdfString(l)} Tj T*\n`;
  return s + 'ET\n';
}

function drawingPage(i) {
  // Grey "scanned page" blocks: no text operators at all.
  let s = '0.85 g\n';
  for (let y = H - MARGIN - 20; y > MARGIN; y -= 22) s += `${MARGIN} ${y} ${W - 2 * MARGIN - (i * 37 + y) % 90} 10 re f\n`;
  return s;
}

function buildPdf(contents) {
  const objs = [];
  const add = (body) => { objs.push(body); return objs.length; };
  const catalog = add(null);
  const pages = add(null);
  const font = add('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>');
  const kids = [];
  for (const c of contents) {
    const buf = Buffer.from(c, 'latin1');
    const stream = add(Buffer.concat([
      Buffer.from(`<< /Length ${buf.length} >>\nstream\n`, 'latin1'), buf, Buffer.from('endstream', 'latin1'),
    ]));
    kids.push(add(`<< /Type /Page /Parent ${pages} 0 R /MediaBox [0 0 ${W} ${H}] ` +
      `/Resources << /Font << /F1 ${font} 0 R >> >> /Contents ${stream} 0 R >>`));
  }
  objs[catalog - 1] = `<< /Type /Catalog /Pages ${pages} 0 R >>`;
  objs[pages - 1] = `<< /Type /Pages /Kids [${kids.map((k) => `${k} 0 R`).join(' ')}] /Count ${kids.length} >>`;

  const parts = [Buffer.from('%PDF-1.4\n%\xe2\xe3\xcf\xd3\n', 'latin1')];
  let offset = parts[0].length;
  const offsets = [];
  objs.forEach((body, i) => {
    const b = Buffer.concat([
      Buffer.from(`${i + 1} 0 obj\n`, 'latin1'),
      Buffer.isBuffer(body) ? body : Buffer.from(body, 'latin1'),
      Buffer.from('\nendobj\n', 'latin1'),
    ]);
    offsets.push(offset);
    offset += b.length;
    parts.push(b);
  });
  let xref = `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`;
  for (const o of offsets) xref += `${String(o).padStart(10, '0')} 00000 n \n`;
  xref += `trailer\n<< /Size ${objs.length + 1} /Root ${catalog} 0 R >>\nstartxref\n${offset}\n%%EOF\n`;
  parts.push(Buffer.from(xref, 'latin1'));
  return Buffer.concat(parts);
}

function write(name, buf) {
  writeFileSync(join(outDir, name), buf);
  console.log(`${name}  ${buf.length} bytes  sha256=${createHash('sha256').update(buf).digest('hex')}`);
}

write('fixture.pdf', buildPdf(corpus.pages.map(textPage)));
write('scanned.pdf', buildPdf([0, 1, 2].map(drawingPage)));
const bench = [];
for (let i = 0; i < 300; i++) {
  const base = corpus.pages[i % corpus.pages.length];
  bench.push(textPage(`Folio ${i + 1}. ${base} ${corpus.pages[(i * 7 + 3) % corpus.pages.length]}`));
}
write('bench.pdf', buildPdf(bench));
