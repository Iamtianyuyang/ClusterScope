// Golden-master comparison of two result files (old program vs new program, CPU vs GPU, ...).
// Binary arrays:
//   node .gauntlet/gauntlet.mjs compare <expected> <actual> --dtype f32 --header 0 --frame 1048576 \
//        --tol 1e-6 --name "stencil/cpu snapshot" --record qa/equivalence.json
//   Per frame: relative L2 error ||a-b||/||a||. Also max |a-b|, NaN/Inf mismatches, bit-identical flag.
//   Streams both files in chunks, so multi-GB outputs are fine.
// Text (logs, CSV, JSON, tables, any printed output):
//   node .gauntlet/gauntlet.mjs compare <expected> <actual> --text [--tol 1e-9] [--atol 1e-12]
//   Line by line: the words must match exactly (whitespace is ignored), every number is compared
//   numerically, |a-b| <= atol or |a-b| <= tol*|a| (both 0 by default: numerically equal, so 1.0 == 1.00).
import fs from 'node:fs';
import path from 'node:path';
import { readJson, writeJson, relToRoot, pathOf } from './util.mjs';

const DTYPES = { f32: [4, Float32Array], f64: [8, Float64Array], i32: [4, Int32Array], i16: [2, Int16Array], u8: [1, Uint8Array] };

export function compareFiles(a, b, { dtype = 'f32', header = 0, frame = 0, chunkElems = 1 << 20 } = {}) {
  const [size, View] = DTYPES[dtype] || [];
  if (!size) throw new Error(`unknown --dtype ${dtype} (use ${Object.keys(DTYPES).join('|')})`);
  const sa = fs.statSync(a).size, sb = fs.statSync(b).size;
  if (sa !== sb) return { error: `size differs: ${sa} vs ${sb} bytes` };
  if ((sa - header) % size) return { error: `payload ${sa - header} bytes is not a multiple of ${dtype}` };
  const n = (sa - header) / size;
  const frameSize = frame > 0 ? frame : n;
  if (n % frameSize) return { error: `${n} elements is not a multiple of --frame ${frameSize}` };
  const fa = fs.openSync(a, 'r'), fb = fs.openSync(b, 'r');
  const bufA = new ArrayBuffer(chunkElems * size), bufB = new ArrayBuffer(chunkElems * size);
  const frames = [];
  let num = 0, den = 0, maxAbs = 0, maxAbsAt = -1, nanMismatch = 0, identical = true;
  let headerDiff = false;
  if (header) {
    const ha = Buffer.alloc(header), hb = Buffer.alloc(header);
    fs.readSync(fa, ha, 0, header, 0); fs.readSync(fb, hb, 0, header, 0);
    headerDiff = !ha.equals(hb);
  }
  try {
    let done = 0;
    while (done < n) {
      const take = Math.min(chunkElems, n - done, frameSize - (done % frameSize));
      fs.readSync(fa, new Uint8Array(bufA, 0, take * size), 0, take * size, header + done * size);
      fs.readSync(fb, new Uint8Array(bufB, 0, take * size), 0, take * size, header + done * size);
      const va = new View(bufA, 0, take), vb = new View(bufB, 0, take);
      const ua = new Uint8Array(bufA, 0, take * size), ub = new Uint8Array(bufB, 0, take * size);
      if (identical) for (let i = 0; i < ua.length; i++) if (ua[i] !== ub[i]) { identical = false; break; }
      for (let i = 0; i < take; i++) {
        const x = va[i], y = vb[i];
        const fx = Number.isFinite(x), fy = Number.isFinite(y);
        if (!fx || !fy) { if (fx !== fy || (Number.isNaN(x) !== Number.isNaN(y))) nanMismatch++; continue; }
        const d = x - y;
        num += d * d; den += x * x;
        const ad = Math.abs(d);
        if (ad > maxAbs) { maxAbs = ad; maxAbsAt = done + i; }
      }
      done += take;
      if (done % frameSize === 0) {
        const rel = den > 0 ? Math.sqrt(num / den) : (num > 0 ? Infinity : 0);
        frames.push(rel);
        num = 0; den = 0;
      }
    }
  } finally { fs.closeSync(fa); fs.closeSync(fb); }
  let worst = 0, worstFrame = 0;
  frames.forEach((r, i) => { if (r > worst) { worst = r; worstFrame = i; } });
  return { elements: n, frameSize, frames: frames.length, perFrame: frames, worstRelL2: worst, worstFrame, maxAbs, maxAbsAt, nanMismatch, bitIdentical: identical && !headerDiff, headerDiff };
}

const NUM = /(?<![\w.])[-+]?(?:\d+\.?\d*|\.\d+)(?:[eEdD][-+]?\d+)?(?![\w.])|(?<![\w])[-+]?(?:nan|inf(?:inity)?)(?![\w])/gi;
const toNum = (t) => { const x = t.toLowerCase().replace(/d/, 'e'); return /nan/.test(x) ? NaN : /inf/.test(x) ? (x.startsWith('-') ? -Infinity : Infinity) : Number(x); };

/** split a line into literal text (whitespace runs normalized) and numbers */
function tokens(line) {
  const text = [], nums = [];
  let last = 0;
  for (const m of line.matchAll(NUM)) {
    text.push(line.slice(last, m.index));
    nums.push(toNum(m[0]));
    last = m.index + m[0].length;
  }
  text.push(line.slice(last));
  return { text: text.map((t) => t.replace(/\s+/g, '')), nums };   // layout (padding, spaces after commas) is not content
}

export function compareText(a, b, { tol = 0, atol = 0 } = {}) {
  const rawA = fs.readFileSync(a), rawB = fs.readFileSync(b);
  const lines = (buf) => {
    const L = buf.toString('utf8').replace(/\r\n?/g, '\n').split('\n').map((l) => l.replace(/\s+$/, ''));
    while (L.length && !L[L.length - 1]) L.pop();
    return L;
  };
  const la = lines(rawA), lb = lines(rawB);
  const mismatches = [];
  let worstRel = 0, maxAbs = 0, numbers = 0, nanMismatch = 0;
  const note = (line, why) => { if (mismatches.length < 20) mismatches.push({ line, why, expected: (la[line - 1] ?? '').slice(0, 200), actual: (lb[line - 1] ?? '').slice(0, 200) }); };
  const n = Math.max(la.length, lb.length);
  for (let i = 0; i < n; i++) {
    if (i >= la.length || i >= lb.length) { note(i + 1, '行数不同'); continue; }
    const ta = tokens(la[i]), tb = tokens(lb[i]);
    if (ta.nums.length !== tb.nums.length || ta.text.some((t, k) => t !== tb.text[k])) { note(i + 1, '文字不同'); continue; }
    for (let k = 0; k < ta.nums.length; k++) {
      const x = ta.nums[k], y = tb.nums[k];
      numbers++;
      if (Number.isNaN(x) || Number.isNaN(y) || !Number.isFinite(x) || !Number.isFinite(y)) {
        if (!(Object.is(x, y) || (Number.isNaN(x) && Number.isNaN(y)))) { nanMismatch++; note(i + 1, '非有限值不同'); }
        continue;
      }
      const d = Math.abs(x - y);
      const rel = x === 0 ? (d === 0 ? 0 : Infinity) : d / Math.abs(x);
      if (d > maxAbs) maxAbs = d;
      if (d > atol && rel > worstRel) worstRel = rel;
      if (d > atol && rel > tol) note(i + 1, `数值超出容差：${x} vs ${y}`);
    }
  }
  return { lines: n, numbers, worstRel, maxAbs, nanMismatch, mismatches, bitIdentical: rawA.equals(rawB) };
}

export function runCompare(cfg, a, b, opts) {
  const tol = opts.tol === undefined ? 0 : Number(opts.tol);
  if (opts.text) return recordEntry(cfg, opts, compareTextEntry(cfg, a, b, opts, tol));
  const r = compareFiles(a, b, { dtype: opts.dtype || 'f32', header: Number(opts.header || 0), frame: Number(opts.frame || 0) });
  const status = r.error ? 'fail' : r.nanMismatch === 0 && (opts.exact ? r.bitIdentical : r.worstRelL2 <= tol) ? 'pass' : 'fail';
  const entry = {
    name: opts.name || `${path.basename(a)} vs ${path.basename(b)}`,
    expected: relToRoot(cfg, a), actual: relToRoot(cfg, b),
    dtype: opts.dtype || 'f32', header: Number(opts.header || 0),
    tol: opts.exact ? 0 : tol, rule: opts.exact ? 'bit-identical' : 'relL2 <= tol per frame',
    ...(r.error ? { error: r.error } : { elements: r.elements, frames: r.frames, worstRelL2: r.worstRelL2, worstFrame: r.worstFrame, maxAbs: r.maxAbs, nanMismatch: r.nanMismatch, bitIdentical: r.bitIdentical, perFrame: r.perFrame.length <= 200 ? r.perFrame : undefined }),
    tags: opts.tags || {},
    status,
    at: new Date().toISOString(),
  };
  return recordEntry(cfg, opts, entry);
}

function compareTextEntry(cfg, a, b, opts, tol) {
  const atol = opts.atol === undefined ? 0 : Number(opts.atol);
  const r = compareText(a, b, { tol: opts.exact ? 0 : tol, atol: opts.exact ? 0 : atol });
  const ok = opts.exact ? r.bitIdentical : r.mismatches.length === 0;
  return {
    name: opts.name || `${path.basename(a)} vs ${path.basename(b)}`,
    expected: relToRoot(cfg, a), actual: relToRoot(cfg, b),
    mode: 'text', tol: opts.exact ? 0 : tol, atol: opts.exact ? 0 : atol,
    rule: opts.exact ? 'byte-identical' : '逐行比较：文字一致（忽略空白），数字 |a-b| <= atol 或相对误差 <= tol',
    lines: r.lines, numbers: r.numbers,
    // worstRelL2 / maxAbs / nanMismatch / bitIdentical keep the evidence table and chart working for both modes
    worstRelL2: Number.isFinite(r.worstRel) ? r.worstRel : Number.MAX_VALUE, maxAbs: r.maxAbs, nanMismatch: r.nanMismatch, bitIdentical: r.bitIdentical,
    ...(r.mismatches.length ? { error: `第 ${r.mismatches[0].line} 行${r.mismatches[0].why}：期望「${r.mismatches[0].expected}」，实际「${r.mismatches[0].actual}」`, mismatches: r.mismatches } : {}),
    tags: opts.tags || {},
    status: ok ? 'pass' : 'fail',
    at: new Date().toISOString(),
  };
}

function recordEntry(cfg, opts, entry) {
  if (opts.record) {
    const file = path.resolve(cfg.root, opts.record);
    const doc = readJson(file, { entries: [] });
    doc.entries = [...doc.entries.filter((e) => e.name !== entry.name), entry];
    writeJson(file, doc);
  }
  return entry;
}

export function printCompare(e) {
  if (e.mode === 'text') {
    console.log(`COMPARE ${e.name}: text, ${e.lines} lines, ${e.numbers} numbers, worst rel ${e.worstRelL2.toExponential(3)}, max |a-b| ${e.maxAbs.toExponential(3)} -> ${e.status.toUpperCase()}`);
    for (const m of e.mismatches || []) console.log(`  line ${m.line}: ${m.why}\n    expected: ${m.expected}\n    actual:   ${m.actual}`);
    return;
  }
  if (e.error) { console.log(`COMPARE ${e.name}: ERROR ${e.error}`); return; }
  console.log(`COMPARE ${e.name}: frames=${e.frames} worst relL2=${e.worstRelL2.toExponential(3)} (frame ${e.worstFrame}) maxAbs=${e.maxAbs.toExponential(3)} NaN-mismatch=${e.nanMismatch} bit-identical=${e.bitIdentical} tol=${e.tol} -> ${e.status.toUpperCase()}`);
}

/** Gate over qa/equivalence.json. required=true means the file must exist and hold at least one entry. */
export function equivalenceGate(cfg, { required }) {
  const doc = readJson(pathOf(cfg, 'qa', 'equivalence.json'), null);
  if (!doc) return { pass: !required, entries: 0, failed: 0, missing: true };
  const failed = doc.entries.filter((e) => e.status !== 'pass');
  return { pass: doc.entries.length > 0 && failed.length === 0, entries: doc.entries.length, failed: failed.length };
}
