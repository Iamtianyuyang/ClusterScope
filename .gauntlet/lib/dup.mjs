// Copy-paste detection (CPD-style) over ALL production code. Comments, strings and preprocessor lines
// are ignored; a clone is a run of >= duplication.minTokens identical tokens in two places.
// For "merge two implementations" tasks this is the gate that proves shared logic really was merged.
import fs from 'node:fs';
import { listSources, relToRoot, writeJson } from './util.mjs';
import { mask, langOf } from './lang.mjs';
import { codeLines, isCodeLine, loadAccepted } from './static.mjs';

const TOKEN_RE = /[A-Za-z_]\w*|\d[\w.]*|->|::|<<|>>|[<>=!+\-*/%&|^]=|&&|\|\||\+\+|--|\S/g;

export function tokenize(src, lang = 'clike') {
  const masked = mask(lang, src);
  const tokens = [];
  let line = 1;
  let last = 0;
  for (const m of masked.matchAll(TOKEN_RE)) {
    for (let i = last; i < m.index; i++) if (masked[i] === '\n') line++;
    last = m.index;
    tokens.push({ t: m[0], line });
  }
  return tokens;
}

function hashStr(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
  return h >>> 0;
}

/** files: [{file, tokens}] -> clone groups [{lines, tokens, a:{file,start,end}, b:{...}}] and per-file duplicated line sets */
export function findClones(files, minTokens) {
  // Rabin-Karp over token hashes, arithmetic mod 2^32 (Math.imul keeps it exact); collisions are
  // harmless because every candidate pair is verified token by token.
  const B = 1000003;
  let pow = 1;
  for (let i = 0; i < minTokens - 1; i++) pow = Math.imul(pow, B) >>> 0;
  const windows = new Map();
  files.forEach((f, fi) => {
    const hs = f.tokens.map((x) => hashStr(x.t));
    f.hs = hs;
    if (hs.length < minTokens) return;
    let h = 0;
    for (let i = 0; i < minTokens; i++) h = (Math.imul(h, B) + hs[i]) >>> 0;
    for (let i = 0; ; i++) {
      (windows.get(h) || windows.set(h, []).get(h)).push([fi, i]);
      if (i + minTokens >= hs.length) break;
      h = (Math.imul((h - Math.imul(hs[i], pow)) >>> 0, B) + hs[i + minTokens]) >>> 0;
    }
  });
  const same = (a, b) => {
    const fa = files[a[0]].hs, fb = files[b[0]].hs;
    for (let k = 0; k < minTokens; k++) if (fa[a[1] + k] !== fb[b[1] + k]) return false;
    return true;
  };
  // pairs of matching windows, then grow each pair into a maximal clone
  const covered = new Set();
  const clones = [];
  const dupLines = files.map(() => new Set());
  for (const occ of windows.values()) {
    if (occ.length < 2) continue;
    for (let x = 0; x < occ.length; x++) for (let y = x + 1; y < occ.length; y++) {
      const [a, b] = [occ[x], occ[y]];
      if (a[0] === b[0] && Math.abs(a[1] - b[1]) < minTokens) continue; // overlapping self-match
      const key = `${a[0]}:${a[1]}|${b[0]}:${b[1]}`;
      if (covered.has(key) || !same(a, b)) continue;
      let s = 0;
      while (a[1] - s - 1 >= 0 && b[1] - s - 1 >= 0 && files[a[0]].hs[a[1] - s - 1] === files[b[0]].hs[b[1] - s - 1]) s++;
      let e = minTokens;
      while (a[1] + e < files[a[0]].hs.length && b[1] + e < files[b[0]].hs.length && files[a[0]].hs[a[1] + e] === files[b[0]].hs[b[1] + e]) e++;
      const ia = a[1] - s, ib = b[1] - s, len = e + s;
      for (let k = 0; k + minTokens <= len; k++) covered.add(`${a[0]}:${ia + k}|${b[0]}:${ib + k}`);
      const ta = files[a[0]].tokens, tb = files[b[0]].tokens;
      const ra = { file: files[a[0]].file, start: ta[ia].line, end: ta[ia + len - 1].line };
      const rb = { file: files[b[0]].file, start: tb[ib].line, end: tb[ib + len - 1].line };
      for (let l = ra.start; l <= ra.end; l++) dupLines[a[0]].add(l);
      for (let l = rb.start; l <= rb.end; l++) dupLines[b[0]].add(l);
      clones.push({ tokens: len, lines: Math.max(ra.end - ra.start, rb.end - rb.start) + 1, a: ra, b: rb });
    }
  }
  clones.sort((p, q) => q.tokens - p.tokens);
  return { clones, dupLines };
}

export function runDuplication(cfg) {
  const minTokens = cfg.duplication.minTokens;
  const files = listSources(cfg).map((abs) => {
    const src = fs.readFileSync(abs, 'utf8');
    const lang = langOf(abs) || 'clike';
    return { file: relToRoot(cfg, abs), tokens: tokenize(src, lang), codeLines: codeLines(src, lang), src, lang };
  });
  const { clones, dupLines } = findClones(files, minTokens);
  const accepted = loadAccepted(cfg).filter((a) => a.kind === 'duplication');
  const isAccepted = (c) => accepted.find((a) => (a.file === c.a.file || a.file === c.b.file) && (!a.other || a.other === c.a.file || a.other === c.b.file));
  // count only non-blank code lines inside clones
  let dup = 0;
  const perFile = files.map((f, i) => {
    const masked = mask(f.lang, f.src).split('\n');
    let n = 0;
    for (const l of dupLines[i]) if (isCodeLine(masked[l - 1])) n++;
    dup += n;
    return { file: f.file, codeLines: f.codeLines, duplicatedLines: n };
  });
  const total = files.reduce((s, f) => s + f.codeLines, 0);
  const ratio = total ? Math.min(1, dup / total) : 0;
  const live = clones.map((c) => ({ ...c, accepted: isAccepted(c)?.reason }));
  const result = {
    minTokens,
    threshold: cfg.thresholds.duplicationMax,
    summary: { files: files.length, codeLines: total, duplicatedLines: dup, ratio: Math.round(ratio * 1000) / 1000, clones: clones.length, crossDirectory: clones.filter((c) => c.a.file.split('/')[0] !== c.b.file.split('/')[0]).length },
    // the ratio counts every clone, accepted or not: accepting a clone documents it, it does not hide it
    pass: ratio <= cfg.thresholds.duplicationMax,
    clones: live.slice(0, 200),
    files: perFile.filter((f) => f.duplicatedLines).sort((a, b) => b.duplicatedLines - a.duplicatedLines),
  };
  writeJson(cfg.out('duplication.json'), result);
  return result;
}

export function printDuplication(r) {
  const s = r.summary;
  console.log(`\nDUPLICATION  ${(s.ratio * 100).toFixed(1)}% of ${s.codeLines} code lines in ${s.clones} clone(s) (${s.crossDirectory} across top-level directories), min ${r.minTokens} tokens`);
  for (const c of r.clones.slice(0, 12)) console.log(`  ${String(c.lines).padStart(4)} lines  ${c.a.file}:${c.a.start}-${c.a.end}  ==  ${c.b.file}:${c.b.start}-${c.b.end}${c.accepted ? '  (accepted)' : ''}`);
  console.log(r.pass ? 'DUPLICATION gate: PASS' : `DUPLICATION gate: FAIL (> ${(r.threshold * 100).toFixed(1)}%)`);
}
