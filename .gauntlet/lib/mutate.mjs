// Portable source-level mutation testing (no Mull/LLVM plugin needed), for any adapter.
// For each mutant: patch one token in one file -> build (if the project has a build) -> tests -> restore.
//   KILLED       tests failed (good)
//   SURVIVED     tests still pass (a hole in the tests)
//   NO_COVERAGE  line never executed by any test (a hole; not even run)
//   TIMEOUT      counted as killed
//   COMPILE_ERROR mutant did not compile (ignored)
//   ACCEPTED     listed in mutation-accepted.json as an equivalent mutant (shown to the human reviewer)
import fs from 'node:fs';
import path from 'node:path';
import { listSources, relToRoot, gitChangedLines, writeJson, readJson, CODE_EXT, pathOf } from './util.mjs';
import { build, ctest } from './build.mjs';
import { lineHits, computeCrap } from './crap.mjs';
import { mask, maskSource, isFortran, langOf } from './lang.mjs';

export { maskSource };

// Binary operators are only mutated when written with spaces around them (clang-format style),
// which keeps template angle brackets, pointers and references out of the way.
const SPACED = [
  ['<', '<='], ['<=', '<'], ['>', '>='], ['>=', '>'],
  ['==', '!='], ['!=', '=='], ['===', '!=='], ['!==', '==='],
  ['&&', '||'], ['||', '&&'],
  ['+', '-'], ['-', '+'], ['*', '/'], ['/', '*'], ['%', '*'],
  ['+=', '-='], ['-=', '+='],
];
// Fortran: "/=" is not-equal, logic and relations also come as dotted words (case-insensitive)
const SPACED_FORTRAN = [
  ['<', '<='], ['<=', '<'], ['>', '>='], ['>=', '>'], ['==', '/='], ['/=', '=='],
  ['+', '-'], ['-', '+'], ['*', '/'], ['/', '*'],
];
const DOTTED = [
  ['.and.', '.or.'], ['.or.', '.and.'], ['.eq.', '.ne.'], ['.ne.', '.eq.'], ['.lt.', '.le.'], ['.le.', '.lt.'],
  ['.gt.', '.ge.'], ['.ge.', '.gt.'], ['.true.', '.false.'], ['.false.', '.true.'], ['.eqv.', '.neqv.'], ['.neqv.', '.eqv.'],
];
const WORDS = {
  clike: [['true', 'false'], ['false', 'true']],
  python: [['True', 'False'], ['False', 'True'], ['and', 'or'], ['or', 'and']],
};

export function findMutants(src, { onlyLines = null, lang = 'clike' } = {}) {
  const masked = mask(lang, src);
  const lineStarts = [0];
  for (let i = 0; i < src.length; i++) if (src[i] === '\n') lineStarts.push(i + 1);
  const lineOf = (pos) => { let lo = 0, hi = lineStarts.length - 1; while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (lineStarts[mid] <= pos) lo = mid; else hi = mid - 1; } return lo + 1; };
  const mutants = [];
  const add = (pos, len, replacement, op) => {
    const line = lineOf(pos);
    if (onlyLines && !onlyLines.has(line)) return;
    mutants.push({ pos, len, original: src.substr(pos, len), replacement, op, line, col: pos - lineStarts[line - 1] + 1 });
  };
  const fortran = isFortran(lang);
  for (const [from, to] of fortran ? SPACED_FORTRAN : SPACED) {
    const re = new RegExp(` ${from.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} `, 'g');
    let m;
    while ((m = re.exec(masked))) {
      const pos = m.index + 1;
      // skip operator overload declarations
      if (/operator\s*$/.test(masked.slice(Math.max(0, pos - 12), pos))) continue;
      add(pos, from.length, to, `${from} -> ${to}`);
      re.lastIndex = m.index + from.length + 1;
    }
  }
  for (const [from, to] of fortran ? [] : WORDS[lang] || WORDS.clike) {
    const re = new RegExp(`\\b${from}\\b`, 'g');
    let m;
    while ((m = re.exec(masked))) add(m.index, from.length, to, `${from} -> ${to}`);
  }
  if (fortran) {
    for (const [from, to] of DOTTED) {
      const re = new RegExp(from.replace(/\./g, '\\.'), 'gi');
      let m;
      while ((m = re.exec(masked))) add(m.index, from.length, to, `${from} -> ${to}`);
    }
    // negate IF conditions: "if (cond)" -> "if (.not.(cond))"
    const re = /\bif\s*\(/gi;
    let m;
    while ((m = re.exec(masked))) {
      const open = m.index + m[0].length - 1;
      let depth = 0, close = -1;
      for (let k = open; k < masked.length && masked[k] !== '\n'; k++) {
        if (masked[k] === '(') depth++;
        else if (masked[k] === ')' && --depth === 0) { close = k; break; }
      }
      if (close < 0) continue;
      add(open + 1, close - open - 1, `.not.(${src.slice(open + 1, close)})`, 'negate if condition');
    }
  } else if (lang === 'python') {
    // negate if / elif / while conditions: "if cond:" -> "if not (cond):"
    const re = /\b(if|elif|while)\s+([^\n]+?):[ \t]*(?=\n|$)/g;
    let m;
    while ((m = re.exec(masked))) {
      const start = m.index + m[0].indexOf(m[2], m[1].length);
      const cond = src.slice(start, start + m[2].length);
      add(start, m[2].length, `not (${cond})`, `negate ${m[1]} condition`);
    }
  } else {
    // ++/-- swap
    const inc = /\+\+|--/g;
    let m;
    while ((m = inc.exec(masked))) add(m.index, 2, m[0] === '++' ? '--' : '++', `${m[0]} -> ${m[0] === '++' ? '--' : '++'}`);
    // negate if / while conditions: "if (cond)" -> "if (!(cond))"
    const re = /\b(if|while)\s*\(/g;
    while ((m = re.exec(masked))) {
      const open = m.index + m[0].length - 1;
      let depth = 0, close = -1;
      for (let k = open; k < masked.length; k++) {
        if (masked[k] === '(') depth++;
        else if (masked[k] === ')' && --depth === 0) { close = k; break; }
      }
      if (close < 0 || /^\s*constexpr\b/.test(masked.slice(m.index + m[1].length))) continue;
      const cond = src.slice(open + 1, close);
      add(open + 1, close - open - 1, `!(${cond})`, `negate ${m[1]} condition`);
    }
  }
  // integer literal boundary: N -> N+1 (compile errors from array sizes etc. are ignored)
  {
    const re = /(?<![\w.])(\d+)(?![\w.'])/g;
    let m;
    while ((m = re.exec(masked))) {
      const v = BigInt(m[1]);
      add(m.index, m[1].length, String(v === 0n ? 1n : v + 1n), `${m[1]} -> ${v === 0n ? 1n : v + 1n}`);
    }
  }
  return mutants.sort((a, b) => a.pos - b.pos);
}

// ---- never leave a mutant behind ----
// Windows briefly locks files (antivirus, indexer, editors): writes are retried. Before a file is first
// mutated its original goes into <outDir>/mutation-restore/ (journal.json maps entries to files); the
// journal is removed only after every file is restored, and every gauntlet command restores a leftover
// journal first, so even a crash or a kill cannot leave mutated source code in the repository.
const TRANSIENT = new Set(['EBUSY', 'EPERM', 'EACCES', 'UNKNOWN', 'EAGAIN', 'EMFILE']);
const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

export function writeFileRetry(file, data) {
  for (let i = 0; ; i++) {
    try { fs.writeFileSync(file, data); return; } catch (e) {
      if (!TRANSIENT.has(e.code) || i >= 40) throw e;
      sleep(Math.min(1000, 50 * (i + 1)));
    }
  }
}

const journalDir = (cfg) => cfg.out('mutation-restore');

function journalAdd(cfg, abs, src) {
  const dir = journalDir(cfg);
  fs.mkdirSync(dir, { recursive: true });
  const jf = path.join(dir, 'journal.json');
  const j = readJson(jf, { files: [] });
  const backup = path.join(dir, `${j.files.length}.orig`);
  writeFileRetry(backup, src);
  j.files.push({ file: abs, backup });
  writeFileRetry(jf, JSON.stringify(j, null, 2));
}

/** Restore every file of a leftover journal (crash / kill / failed restore). Returns the restored files. */
export function recoverMutations(cfg) {
  const jf = path.join(journalDir(cfg), 'journal.json');
  const j = readJson(jf, null);
  if (!j) return [];
  const restored = [];
  for (const e of j.files) {
    writeFileRetry(e.file, fs.readFileSync(e.backup, 'utf8'));
    restored.push(relToRoot(cfg, e.file));
  }
  fs.rmSync(journalDir(cfg), { recursive: true, force: true });
  if (restored.length) console.log(`⚠ 上一次变异测试没有正常结束，已从备份恢复：${restored.join(', ')}`);
  return restored;
}

/** Keep only mutants that sit inside a function body (function ranges from static analysis or coverage). */
function inFunctions(mutants, ranges) {
  if (!ranges) return mutants;
  return mutants.filter((m) => ranges.some(([a, b]) => m.line >= a && m.line <= b));
}

/** Function line ranges per file: static analysis (covers code the tests never ran), else `fallback()`. */
export function functionRanges(cfg, fallback) {
  const fnRanges = new Map();
  const stat = readJson(cfg.out('static.json'), null);
  const push = (f) => (fnRanges.get(f.file) || fnRanges.set(f.file, []).get(f.file)).push([f.line, f.endLine]);
  if (stat) {
    for (const f of stat.files) if (f.status === 'ok') fnRanges.set(f.file, []);
    stat.functions.forEach(push);
  } else fallback().forEach(push);
  return fnRanges;
}

/** CMake + clang adapter: incremental cmake build + ctest, hits from llvm-cov. */
export async function runMutation(cfg, opts = {}) {
  return mutateWith(cfg, opts, {
    files: listSources(cfg),
    lang: (f) => (CODE_EXT.test(f) ? 'clike' : langOf(f)),
    hits: lineHits(cfg),
    // Only files with runtime coverage data can be skipped as NO_COVERAGE; uninstrumented code
    // (CUDA built by nvcc, ...) is always mutated, built and tested for real.
    missingFileIsUncovered: false,
    fnRanges: functionRanges(cfg, () => computeCrap(cfg).functions),
    baseline: () => {
      build(cfg, { quiet: true });
      const b = ctest(cfg, { quiet: true });
      return { ok: b.code === 0, ms: b.ms };
    },
    build: () => build(cfg, { quiet: true, allowFail: true }).code,
    test: (timeoutSec) => {
      const t = ctest(cfg, { quiet: true, timeoutSec, extra: ['--stop-on-failure'] });
      return t.code === 0 ? 'SURVIVED' : /timeout/i.test(t.stdout) ? 'TIMEOUT' : 'KILLED';
    },
    finish: () => build(cfg, { quiet: true, allowFail: true }),
  });
}

/**
 * The mutation loop, independent of build system and language.
 * exec: { files, lang(file), hits: Map<rel, Map<line,count>>|null, missingFileIsUncovered, fnRanges,
 *         baseline() -> {ok, ms}, build() -> exit code, test(timeoutSec) -> KILLED|SURVIVED|TIMEOUT, finish() }
 */
export async function mutateWith(cfg, opts, exec) {
  const base = opts.base ?? (cfg.mutation.base !== 'auto' ? cfg.mutation.base : cfg.base);
  const wantChanged = opts.changed || (!opts.all && cfg.mutation.scope === 'changed');
  const changed = wantChanged ? gitChangedLines(cfg, base) : null;
  if (wantChanged && !changed) console.log('mutation: no git base found, mutating all production code');
  const files = opts.files?.length ? opts.files.map((f) => path.resolve(cfg.root, f)) : exec.files;
  const plan = [];
  const unsupported = [];
  for (const abs of files) {
    const rel = relToRoot(cfg, abs);
    let only = null;
    if (changed) {
      const c = changed.map.get(rel);
      if (!c) continue;
      if (c !== 'all') only = c;
    }
    const lang = exec.lang(abs);
    if (!lang) { unsupported.push(rel); continue; }
    const src = fs.readFileSync(abs, 'utf8');
    const ms = inFunctions(findMutants(src, { onlyLines: only, lang }), exec.fnRanges.get(rel));
    for (const m of ms) plan.push({ ...m, file: rel, abs });
  }
  const scope = changed ? `相对 ${changed.ref.slice(0, 10)} 改动过的行` : '全部产品代码';
  let mutants = plan;
  if (cfg.mutation.maxMutants > 0 && mutants.length > cfg.mutation.maxMutants) {
    const step = mutants.length / cfg.mutation.maxMutants;
    mutants = Array.from({ length: cfg.mutation.maxMutants }, (_, i) => plan[Math.floor(i * step)]);
  }
  console.log(`mutation: ${mutants.length} mutants (${plan.length} candidates) over ${scope}`);
  if (unsupported.length) console.log(`mutation: ⚠ 内置变异引擎不支持这些文件的语言，没有被变异：${unsupported.join(', ')}`);

  // Baseline (also proves the suite is green before mutating).
  const baseline = await exec.baseline();
  if (!baseline.ok) throw new Error('test suite is red before mutation: fix tests first');
  const timeoutSec = Math.max(cfg.mutation.minTimeoutSec, Math.ceil((baseline.ms / 1000) * cfg.mutation.timeoutFactor));

  recoverMutations(cfg);
  const originals = new Map();
  const restoreAll = () => {
    const failed = [];
    for (const [abs, src] of originals) { try { writeFileRetry(abs, src); } catch (e) { failed.push(`${relToRoot(cfg, abs)}: ${e.message}`); } }
    if (failed.length) throw new Error(`变异测试后无法恢复源文件（备份在 ${relToRoot(cfg, journalDir(cfg))}/，下次运行任何 gauntlet 命令会自动恢复）：\n${failed.join('\n')}`);
    fs.rmSync(journalDir(cfg), { recursive: true, force: true });
  };
  const onSig = () => { try { restoreAll(); } catch (e) { console.error(e.message); } process.exit(130); };
  process.on('SIGINT', onSig);
  process.on('SIGTERM', onSig);
  // Equivalent mutants a human agreed to accept: [{file, op, source, reason}] ("source" = substring of the line).
  const accepted = readJson(pathOf(cfg, 'mutationAccepted'), []);
  const acceptedBy = (m, text) => accepted.find((a) => a.file === m.file && a.op === m.op && text.includes(a.source));
  const results = [];
  try {
    for (let k = 0; k < mutants.length; k++) {
      const m = mutants[k];
      const fileHits = exec.hits?.get(m.file);
      const knownUncovered = exec.hits && (fileHits ? !((fileHits.get(m.line) || 0) > 0) : exec.missingFileIsUncovered);
      let status;
      const acc = acceptedBy(m, lineText(cfg, m.file, m.line));
      if (acc) status = 'ACCEPTED';
      else if (knownUncovered) status = 'NO_COVERAGE';
      else {
        if (!originals.has(m.abs)) { const orig = fs.readFileSync(m.abs, 'utf8'); journalAdd(cfg, m.abs, orig); originals.set(m.abs, orig); }
        const src = originals.get(m.abs);
        writeFileRetry(m.abs, src.slice(0, m.pos) + m.replacement + src.slice(m.pos + m.len));
        try {
          status = (await exec.build()) !== 0 ? 'COMPILE_ERROR' : await exec.test(timeoutSec);
        } finally {
          writeFileRetry(m.abs, src);
        }
      }
      const { abs, pos, len, ...rest } = m;
      results.push({ ...rest, status, ...(acc ? { reason: acc.reason } : {}) });
      process.stdout.write(`  [${k + 1}/${mutants.length}] ${status.padEnd(13)} ${m.file}:${m.line}:${m.col}  ${m.op}\n`);
    }
  } finally {
    restoreAll();
    process.off('SIGINT', onSig);
    process.off('SIGTERM', onSig);
    await exec.finish();
  }
  return writeMutationReport(cfg, { scope, candidates: plan.length, results, unsupported });
}

/** Score + mutation.json from per-mutant results (shared with imported reports of external tools). */
export function writeMutationReport(cfg, { scope, candidates, results, tool, unsupported = [] }) {
  const count = (s) => results.filter((r) => r.status === s).length;
  const killed = count('KILLED') + count('TIMEOUT');
  const survived = count('SURVIVED');
  const noCov = count('NO_COVERAGE');
  const denom = killed + survived + noCov;
  const score = denom ? killed / denom : 1;
  const withSource = (r) => ({ ...r, source: r.source ?? lineText(cfg, r.file, r.line) });
  const report = {
    scope,
    ...(tool ? { tool } : {}),
    summary: { total: results.length, candidates: candidates ?? results.length, killed, survived, noCoverage: noCov, compileErrors: count('COMPILE_ERROR'), accepted: count('ACCEPTED'), unsupported: unsupported.length, score: Math.round(score * 1000) / 1000 },
    threshold: cfg.thresholds.mutationScoreMin,
    // code the engine could not mutate at all is not "killed": it needs an external tool or a human decision
    pass: score >= cfg.thresholds.mutationScoreMin && unsupported.length === 0,
    unsupported,
    accepted: results.filter((r) => r.status === 'ACCEPTED').map(withSource),
    survivors: results.filter((r) => r.status === 'SURVIVED' || r.status === 'NO_COVERAGE').map(withSource),
    results,
  };
  writeJson(cfg.out('mutation.json'), report);
  console.log(`\nMUTATION score=${(score * 100).toFixed(1)}%  killed=${killed} survived=${survived} no-coverage=${noCov} compile-errors=${report.summary.compileErrors}`);
  for (const s of report.survivors.slice(0, 20)) console.log(`  ${s.status.padEnd(11)} ${s.file}:${s.line}  ${s.op}   | ${String(s.source).trim()}`);
  if (unsupported.length) console.log(`  ${unsupported.length} 个文件没法变异（语言不支持）：配置 commands.mutation（外部变异工具），或 NEED-HUMAN`);
  console.log(report.pass ? 'MUTATION gate: PASS' : 'MUTATION gate: FAIL');
  return report;
}

function lineText(cfg, file, line) {
  try { return fs.readFileSync(path.resolve(cfg.root, file), 'utf8').split(/\r?\n/)[line - 1] ?? ''; } catch { return ''; }
}
