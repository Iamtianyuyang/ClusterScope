// leak-check: did the program write outside the places it is allowed to?
// Replaces the POSIX-only `find /tmp ~ -newer <marker>` so the same check runs on Windows.
//   leak-check --mark                    record the start time (before running the program)
//   leak-check [--roots a,b] [--ignore a,b] [--depth N]
//                                        list files under the roots modified after the mark
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { readJson, writeJson } from './util.mjs';

const MAX_FILES = 200000;   // scan budget: a home directory can be huge; report truncation instead of hanging

const markFile = (cfg) => cfg.out('leak-mark.json');
const splitList = (v) => (typeof v === 'string' ? v.split(',').map((s) => s.trim()).filter(Boolean) : []);
const expandHome = (p) => (p === '~' || p.startsWith('~/') || p.startsWith('~\\') ? path.join(os.homedir(), p.slice(1)) : p);
const norm = (p) => { const r = path.resolve(expandHome(p)); return process.platform === 'win32' ? r.toLowerCase() : r; };
const inside = (p, dir) => p === dir || p.startsWith(dir.endsWith(path.sep) ? dir : dir + path.sep);

export function leakMark(cfg) {
  const at = Date.now();
  writeJson(markFile(cfg), { at, iso: new Date(at).toISOString() });
  return { at };
}

/** Default roots: the system temp dir and the home dir — the places a program writes to by accident. */
function defaultRoots() {
  return [...new Set([os.tmpdir(), os.homedir()])];
}

/** Paths that change while the agent itself works: the repo, the harness home, the mark file. */
function defaultIgnores(cfg) {
  const own = [cfg.root];
  if (process.env.DSH_HOME) own.push(process.env.DSH_HOME);
  return own;
}

export function leakCheck(cfg, opts = {}) {
  const mark = readJson(markFile(cfg), null);
  if (!mark?.at) throw new Error('leak-check: no mark — run `leak-check --mark` before starting the program');
  const roots = (opts.roots?.length ? opts.roots : defaultRoots()).map((p) => path.resolve(expandHome(p)));
  const shownIgnores = [...defaultIgnores(cfg), ...(opts.ignore || [])].map((p) => path.resolve(expandHome(p)));
  const ignores = shownIgnores.map(norm);
  const depth = Number.isFinite(+opts.depth) && +opts.depth > 0 ? +opts.depth : 8;
  const found = [];
  let scanned = 0, truncated = false;

  const visit = (dir, level) => {
    if (truncated || ignores.some((ig) => inside(norm(dir), ig))) return;
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }   // unreadable: skip
    for (const e of entries) {
      if (++scanned > MAX_FILES) { truncated = true; return; }
      const p = path.join(dir, e.name);
      if (e.isSymbolicLink()) continue;
      if (ignores.some((ig) => inside(norm(p), ig))) continue;
      if (e.isDirectory()) { if (level < depth) visit(p, level + 1); continue; }
      if (!e.isFile()) continue;
      let st;
      try { st = fs.statSync(p); } catch { continue; }
      if (st.mtimeMs > mark.at) found.push(p);
    }
  };
  for (const r of roots) visit(r, 1);
  return { since: mark.iso, roots, ignores: shownIgnores, depth, scanned: Math.min(scanned, MAX_FILES), truncated, files: found.sort(), pass: found.length === 0 && !truncated };
}

export function printLeak(r) {
  console.log(`leak-check since ${r.since}`);
  console.log(`  roots:   ${r.roots.join(', ')}`);
  console.log(`  ignored: ${r.ignores.join(', ')}`);
  console.log(`  scanned ${r.scanned} entries (depth ≤ ${r.depth})${r.truncated ? ' — TRUNCATED: narrow --roots or --depth' : ''}`);
  if (r.files.length) {
    console.log(`  ${r.files.length} file(s) written outside the allowed places:`);
    for (const f of r.files.slice(0, 50)) console.log(`    ${f}`);
    if (r.files.length > 50) console.log(`    … ${r.files.length - 50} more`);
  }
  console.log(`LEAK-CHECK: ${r.pass ? 'PASS' : 'FAIL'}`);
}

export { splitList };
