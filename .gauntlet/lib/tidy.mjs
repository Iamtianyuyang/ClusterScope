// clang-tidy over every production translation unit (bugprone / performance / clang static analyzer).
// Uses the same translated arguments as static analysis, so CUDA files are checked too.
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { makeSourceMatcher, relToRoot, writeJson, which } from './util.mjs';
import { loadCompileDb, clangArgsFor } from './compdb.mjs';
import { loadAccepted } from './static.mjs';

const DIAG_RE = /^((?:[A-Za-z]:)?[^:\n]+):(\d+):(\d+): (warning|error): (.*?) \[([^\]]+)\]$/;

function tidyOne(cfg, entry) {
  const { args } = clangArgsFor(entry, cfg);
  const argv = [entry.file, `--checks=-*,${cfg.static.tidyChecks}`, '--quiet', '--', ...args];
  return new Promise((resolve) => {
    const child = spawn(cfg.llvm.tidy, argv, { cwd: entry.directory, windowsHide: true });
    let out = '';
    child.stdout.on('data', (d) => { out += d; });
    child.stderr.on('data', () => {});
    child.on('error', (e) => resolve({ out: '', error: e.message }));
    child.on('close', () => resolve({ out }));
  });
}

export async function runTidy(cfg, { quiet = false } = {}) {
  const mode = cfg.static.tidy;
  const exe = mode === false ? null : which(cfg.llvm.tidy);
  if (!exe) {
    const required = mode === true;
    const r = { ran: false, pass: !required, reason: mode === false ? 'disabled in gauntlet.config.json' : `${cfg.llvm.tidy} not found`, findings: [], summary: { findings: 0 } };
    writeJson(cfg.out('tidy.json'), r);
    return r;
  }
  const isSrc = makeSourceMatcher(cfg);
  const tus = loadCompileDb(cfg).filter((e) => isSrc(relToRoot(cfg, e.file)));
  const accepted = loadAccepted(cfg).filter((a) => a.kind === 'tidy');
  const found = new Map();
  let i = 0;
  const limit = Math.max(1, Math.min(8, Math.floor(os.cpus().length / 2)));
  await Promise.all(Array.from({ length: Math.min(limit, tus.length) }, async () => {
    while (i < tus.length) {
      const e = tus[i++];
      const { out } = await tidyOne(cfg, e);
      if (!quiet) process.stdout.write(`  tidy ${relToRoot(cfg, e.file)}\n`);
      for (const line of out.split(/\r?\n/)) {
        const m = DIAG_RE.exec(line);
        if (!m) continue;
        const file = relToRoot(cfg, path.resolve(e.directory, m[1]));
        if (!isSrc(file)) continue;
        const key = `${file}:${m[2]}:${m[6]}`;
        if (found.has(key)) continue;
        const acc = accepted.find((a) => a.file === file && (!a.check || a.check === m[6]) && (!a.match || m[5].includes(a.match)));
        found.set(key, { file, line: +m[2], col: +m[3], message: m[5], check: m[6], accepted: acc ? acc.reason : undefined });
      }
    }
  }));
  const findings = [...found.values()].sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
  const live = findings.filter((f) => !f.accepted);
  const byCheck = {};
  for (const f of live) byCheck[f.check] = (byCheck[f.check] || 0) + 1;
  const r = { ran: true, checks: cfg.static.tidyChecks, pass: live.length <= cfg.thresholds.tidyMax, summary: { findings: live.length, accepted: findings.length - live.length, byCheck }, findings };
  writeJson(cfg.out('tidy.json'), r);
  return r;
}

export function printTidy(r) {
  if (!r.ran) { console.log(`\nTIDY: not run (${r.reason})${r.pass ? '' : ' — required'}`); return; }
  console.log(`\nTIDY  findings=${r.summary.findings} accepted=${r.summary.accepted}`);
  for (const f of r.findings.filter((x) => !x.accepted).slice(0, 15)) console.log(`  ⚠ ${f.file}:${f.line} ${f.message} [${f.check}]`);
  console.log(r.pass ? 'TIDY gate: PASS' : 'TIDY gate: FAIL');
}
