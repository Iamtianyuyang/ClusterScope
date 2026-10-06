// cppcheck (https://github.com/cppcheck-opensource/cppcheck): a second static analyzer with different
// strengths than clang-tidy (uninitialized data, buffer/index errors, resource leaks across branches).
// Fed with the same translated include paths / defines as static analysis, so .cu files are checked too.
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { makeSourceMatcher, relToRoot, writeJson, which } from './util.mjs';
import { loadCompileDb, clangArgsFor } from './compdb.mjs';
import { loadAccepted } from './static.mjs';

const LINE_RE = /^((?:[A-Za-z]:)?[^:\n]+):(\d+):(error|warning|style|performance|portability|information):([\w-]+):(.*)$/;

function cppcheckOne(cfg, exe, entry) {
  const { lang, args } = clangArgsFor(entry, cfg);
  const keep = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '-I' || a === '-isystem' || a === '-iquote' || a === '-D' || a === '-U') { keep.push(a === '-isystem' || a === '-iquote' ? '-I' : a, args[++i]); continue; }
    if (/^-(I|D|U)/.test(a)) keep.push(a);
    else if (/^-(isystem|iquote)/.test(a)) keep.push(`-I${a.replace(/^-(isystem|iquote)/, '')}`);
    else if (/^-std=/.test(a)) keep.push(`--std=${a.slice(5).replace(/^gnu/, 'c')}`);
  }
  const argv = [
    entry.file, `--language=${lang === 'c' ? 'c' : 'c++'}`, ...keep,
    `--enable=${cfg.cppcheck.enable}`, '--inline-suppr', '--quiet', '--suppress=missingIncludeSystem', '--suppress=unmatchedSuppression',
    '--template={file}:{line}:{severity}:{id}:{message}',
    ...(lang === 'cuda' ? ['-D__global__=', '-D__device__=', '-D__host__=', '-D__shared__=', '-D__constant__=', '-D__forceinline__=inline', '-D__restrict__='] : []),
  ];
  return new Promise((resolve) => {
    const child = spawn(exe, argv, { cwd: entry.directory, windowsHide: true });
    let err = '';
    child.stderr.on('data', (d) => { err += d; });
    child.stdout.on('data', () => {});
    child.on('error', () => resolve(''));
    child.on('close', () => resolve(err));
  });
}

export async function runCppcheck(cfg, { quiet = false } = {}) {
  const mode = cfg.cppcheck.mode;
  const exe = mode === false ? null : which(cfg.cppcheck.exe);
  if (!exe) {
    const r = { ran: false, pass: mode !== true, reason: mode === false ? 'disabled in gauntlet.config.json' : `${cfg.cppcheck.exe} not found`, findings: [], summary: { findings: 0 } };
    writeJson(cfg.out('cppcheck.json'), r);
    return r;
  }
  const isSrc = makeSourceMatcher(cfg);
  const tus = loadCompileDb(cfg).filter((e) => isSrc(relToRoot(cfg, e.file)));
  const accepted = loadAccepted(cfg).filter((a) => a.kind === 'cppcheck');
  const found = new Map();
  let i = 0;
  const limit = Math.max(1, Math.min(8, Math.floor(os.cpus().length / 2)));
  await Promise.all(Array.from({ length: Math.min(limit, tus.length) }, async () => {
    while (i < tus.length) {
      const e = tus[i++];
      const out = await cppcheckOne(cfg, exe, e);
      if (!quiet) process.stdout.write(`  cppcheck ${relToRoot(cfg, e.file)}\n`);
      for (const line of out.split(/\r?\n/)) {
        const m = LINE_RE.exec(line.trim());
        if (!m || m[3] === 'information') continue;
        const file = relToRoot(cfg, path.resolve(e.directory, m[1]));
        if (!isSrc(file)) continue;
        const key = `${file}:${m[2]}:${m[4]}`;
        if (found.has(key)) continue;
        const acc = accepted.find((a) => a.file === file && (!a.check || a.check === m[4]) && (!a.match || m[5].includes(a.match)));
        found.set(key, { file, line: +m[2], severity: m[3], check: m[4], message: m[5].trim(), accepted: acc ? acc.reason : undefined });
      }
    }
  }));
  const findings = [...found.values()].sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
  const live = findings.filter((f) => !f.accepted);
  const r = { ran: true, enable: cfg.cppcheck.enable, pass: live.length <= cfg.thresholds.cppcheckMax, summary: { findings: live.length, accepted: findings.length - live.length }, findings };
  writeJson(cfg.out('cppcheck.json'), r);
  return r;
}

export function printCppcheck(r) {
  if (!r.ran) { console.log(`\nCPPCHECK: not run (${r.reason})${r.pass ? '' : ' — required'}`); return; }
  console.log(`\nCPPCHECK  findings=${r.summary.findings} accepted=${r.summary.accepted}`);
  for (const f of r.findings.filter((x) => !x.accepted).slice(0, 15)) console.log(`  ⚠ ${f.file}:${f.line} [${f.severity}/${f.check}] ${f.message}`);
  console.log(r.pass ? 'CPPCHECK gate: PASS' : 'CPPCHECK gate: FAIL');
}
