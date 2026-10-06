// Shared helpers: config loading, process execution, globbing, output paths.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const KIT_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const DEFAULT_CONFIG = {
  // 'commands':    the project's own build / test / lint commands + standard reports (any language, any toolchain).
  // 'cmake-clang': optional deep analysis for projects that already build with CMake and compile with clang
  //                (clang AST, source-based coverage); must be chosen explicitly.
  adapter: 'commands',
  commands: {},
  complexity: { engine: 'auto' },   // commands adapter: 'auto' = lizard when installed, else built-in
  // the branch this work is compared against (changed lines for ratchet / mutation, evidence diff).
  // 'auto' = GAUNTLET_BASE env, `base:` in the project profile, the remote's default branch, then common names.
  base: 'auto',
  // where the committed hand-offs live (relative to the project root)
  paths: { qa: 'qa', architecture: 'architecture.json', qualityAccepted: 'quality-accepted.json', mutationAccepted: 'mutation-accepted.json', profile: 'GAUNTLET.md' },
  sourceDir: '.',
  buildDir: 'build-gauntlet',
  outDir: 'gauntlet-out',
  cmake: { generator: 'Ninja', configureArgs: [], buildArgs: [] },
  // Production code = EVERY source file in the repository except tests, generated code and builds. The
  // stage-0 survey writes the project's real list; this fallback covers what the built-in analyzers read.
  // Narrowing the list is a governance change: the evidence pack flags it for the human.
  sources: ['**/*.{c,cc,cpp,cxx,cu,cuh,h,hh,hpp,hxx,inl,ipp}', '**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}', '**/*.{java,kt,kts,scala,cs,go,rs,swift,dart,php,m,mm}', '**/*.{py,pyw}', '**/*.{f,for,f77,f90,f95,f03,f08}'],
  exclude: [
    '**/test/**', '**/tests/**', '**/testing/**', '**/*_test.*', '**/*_tests.*', '**/test_*.*',
    '**/*.test.*', '**/*.spec.*', '**/__tests__/**', '**/conftest.py', '**/node_modules/**',
    'acceptance/**', 'features/**', 'examples/**', 'docs/**', 'tmp/**', 'build*/**', 'cmake-build*/**',
    '**/third_party/**', '**/vendor/**', '**/external/**', '**/generated/**', '**/_deps/**', '.gauntlet/**',
  ],
  features: 'features',
  stepsDir: 'acceptance/steps',
  generatedDir: 'acceptance/generated',
  thresholds: {
    crapMax: 8,              // Uncle Bob: humans <= 4, agents relaxed to 6-8 (instrumented functions)
    complexityMax: 10,       // cyclomatic complexity, EVERY function (static analysis, incl. CUDA)
    functionLinesMax: 60,    // lines per function body
    nestingMax: 4,           // nested control statements
    paramsMax: 7,            // parameters per function
    lineCoverageMin: 0.9,    // over instrumented production lines
    mutationScoreMin: 1.0,   // "zero surviving mutants"
    duplicationMax: 0.03,    // share of production lines inside copy-pasted blocks
    warningsMax: 0,          // compiler warnings (-Wall -Wextra) in production code
    tidyMax: 0,              // clang-tidy findings (when clang-tidy is installed)
    staticScopeMin: 1.0,     // share of production files that static analysis actually parsed
    cppcheckMax: 0,          // cppcheck findings (cmake-clang, when cppcheck is installed)
  },
  static: {
    warnings: ['-Wall', '-Wextra'],
    cudaArch: 'sm_80',       // only used to let clang parse .cu files
    extraArgs: [],
    tidy: 'auto',            // 'auto' = run when clang-tidy exists, true = required, false = off
    tidyChecks: 'bugprone-*,performance-*,clang-analyzer-*,-bugprone-easily-swappable-parameters,-clang-analyzer-security.insecureAPI.*',
  },
  duplication: { minTokens: 100 },
  // scope 'all' mutates every production line (slow, thorough); 'changed' only lines changed vs base
  // (base 'auto' = the top-level `base`).
  // maxMutants 0 = unlimited.
  mutation: { scope: 'all', base: 'auto', maxMutants: 0, timeoutFactor: 3, minTimeoutSec: 10 },
  llvm: { profdata: 'llvm-profdata', cov: 'llvm-cov', cxxfilt: 'llvm-cxxfilt', clang: 'clang', tidy: 'clang-tidy' },
  // second static analyzer (cmake-clang): 'auto' = run when installed, true = required, false = off
  cppcheck: { mode: 'auto', exe: 'cppcheck', enable: 'warning,performance,portability' },
  // run-time checkers: 'auto' = run where the toolchain supports it (else "not run", shown to the human),
  // true = required, false = off. cmake-clang: ASan + UBSan build, compute-sanitizer for CUDA tests;
  // commands adapter: the project's own `commands.sanitize`.
  sanitize: { mode: 'auto', host: true, cuda: true, computeSanitizer: 'compute-sanitizer', cudaTools: ['memcheck', 'racecheck', 'initcheck', 'synccheck'], testTimeoutSec: 1800 },
  loop: { stallRounds: 3 },
  // ratchet mode for existing code: baseline debt is tolerated while it does not get worse; new code and
  // changed lines must meet the thresholds. mutation: 'changed' (default) = mutate changed lines only.
  ratchet: { enabled: false, baseline: 'gauntlet-baseline.json', mutation: 'changed' },   // `gauntlet next` reports STALLED after this many rounds without progress
  tools: { archify: { repo: 'https://github.com/tt-a1i/archify.git', ref: 'v3.0.1', dir: '' } },
};

function deepMerge(a, b) {
  if (Array.isArray(b) || typeof b !== 'object' || b === null) return b;
  const out = { ...a };
  for (const [k, v] of Object.entries(b)) out[k] = k in out ? deepMerge(out[k], v) : v;
  return out;
}

export function readJson(file, fallback) {
  if (!fs.existsSync(file)) return fallback;
  return JSON.parse(fs.readFileSync(file, 'utf8').replace(/^﻿/, ''));
}

export function writeJson(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n');
}

/** gauntlet.config.json (committed) merged with gauntlet.local.json (machine-specific, gitignored). */
export function loadConfig(root) {
  let cfg = deepMerge(DEFAULT_CONFIG, readJson(path.join(root, 'gauntlet.config.json'), {}));
  cfg = deepMerge(cfg, readJson(path.join(root, 'gauntlet.local.json'), {}));
  cfg.root = root;
  cfg.abs = (p) => path.resolve(root, p);
  cfg.out = (...p) => path.resolve(root, cfg.outDir, ...p);
  return cfg;
}

export function findRoot(start = process.cwd()) {
  let dir = path.resolve(start);
  for (;;) {
    if (fs.existsSync(path.join(dir, 'gauntlet.config.json'))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) return path.resolve(start);
    dir = parent;
  }
}

export function run(cmd, args, opts = {}) {
  const { quiet = false, env, cwd, timeoutMs, allowFail = false, input } = opts;
  if (!quiet) console.log(`$ ${[cmd, ...args].join(' ')}`);
  const res = spawnSync(cmd, args, {
    cwd,
    input,
    env: env ? { ...process.env, ...env } : process.env,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    timeout: timeoutMs,
    shell: false,
    windowsHide: true,
  });
  const out = { code: res.status ?? (res.error ? 127 : 1), stdout: res.stdout || '', stderr: res.stderr || '', timedOut: res.error?.code === 'ETIMEDOUT', error: res.error };
  if (!quiet && out.stdout) process.stdout.write(out.stdout);
  if (!quiet && out.stderr) process.stderr.write(out.stderr);
  if (out.code !== 0 && !allowFail) {
    throw new Error(`command failed (${out.code}): ${cmd} ${args.join(' ')}${res.error ? `\n${res.error.message}` : ''}`);
  }
  return out;
}

export function which(cmd) {
  const probe = process.platform === 'win32' ? run('where', [cmd], { quiet: true, allowFail: true }) : run('which', [cmd], { quiet: true, allowFail: true });
  return probe.code === 0 ? probe.stdout.split(/\r?\n/)[0].trim() : null;
}

export const toPosix = (p) => p.split(path.sep).join('/');

export function globToRegExp(glob) {
  let re = '';
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*') {
      if (glob[i + 1] === '*') {
        i++;
        if (glob[i + 1] === '/') { i++; re += '(?:.*/)?'; } else re += '.*';
      } else re += '[^/]*';
    } else if (c === '?') re += '[^/]';
    else if (c === '{' && glob.indexOf('}', i) > i) {
      // brace alternatives: *.{c,cpp,h}
      const end = glob.indexOf('}', i);
      re += `(?:${glob.slice(i + 1, end).split(',').map((p) => p.replace(/[.+^${}()|[\]\\]/g, '\\$&')).join('|')})`;
      i = end;
    }
    else re += c.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${re}$`, 'i');
}

/** Is a repo-relative posix path production code that the gauntlet measures? */
export function makeSourceMatcher(cfg) {
  const inc = cfg.sources.map(globToRegExp);
  const exc = [...cfg.exclude, `${toPosix(cfg.buildDir)}/**`, `${toPosix(cfg.outDir)}/**`].map(globToRegExp);
  // files outside the project (../kit, /usr/include, C:/...) are never production code of this project
  const outside = (rel) => rel.startsWith('../') || rel === '..' || /^([A-Za-z]:)?\//.test(rel);
  return (rel) => !outside(rel) && inc.some((r) => r.test(rel)) && !exc.some((r) => r.test(rel));
}

export function relToRoot(cfg, file) {
  return toPosix(path.relative(cfg.root, path.resolve(file)));
}

export function walk(dir, filter = () => true, acc = []) {
  if (!fs.existsSync(dir)) return acc;
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    if (ent.name.startsWith('.') && ent.name !== '.') continue;
    const p = path.join(dir, ent.name);
    if (ent.isDirectory()) walk(p, filter, acc);
    else if (filter(p)) acc.push(p);
  }
  return acc;
}

/** Walk the repo, pruning build/output/excluded directories early. keep(rel) decides on files. */
export function listRepoFiles(cfg, keep) {
  const exc = [...cfg.exclude, `${toPosix(cfg.buildDir)}/**`, `${toPosix(cfg.outDir)}/**`].map(globToRegExp);
  const prune = (rel) => rel === '.git' || rel.endsWith('/node_modules') || rel === 'node_modules' || exc.some((r) => r.test(`${rel}/`));
  const files = [];
  const rec = (dir) => {
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, ent.name);
      const rel = relToRoot(cfg, p);
      if (ent.isDirectory()) { if (!prune(rel)) rec(p); }
      else if (keep(rel)) files.push(p);
    }
  };
  rec(cfg.root);
  return files.sort();
}

export function listSources(cfg) {
  return listRepoFiles(cfg, makeSourceMatcher(cfg));
}

export const CODE_EXT = /\.(c|cc|cpp|cxx|cu|cuh|h|hh|hpp|hxx|inl|ipp)$/i;
export const isHeader = (f) => /\.(h|hh|hpp|hxx|cuh|inl|ipp)$/i.test(f);

export function ensureDir(d) { fs.mkdirSync(d, { recursive: true }); return d; }

/** A committed hand-off file: pathOf(cfg, 'qa', 'qa-report.json'), pathOf(cfg, 'architecture'), ... */
export function pathOf(cfg, key, ...rest) {
  return path.resolve(cfg.root, cfg.paths?.[key] ?? key, ...rest);
}

/**
 * The commit this branch is compared against, and how it was found. null when git or every candidate is
 * unavailable. Never guesses silently: callers that need a base must treat null as an error.
 */
export function resolveBase(cfg, base) {
  const git = (args) => run('git', args, { cwd: cfg.root, quiet: true, allowFail: true });
  if (git(['rev-parse', '--is-inside-work-tree']).code !== 0) return null;
  const explicit = base && base !== 'auto' ? base : cfg.base && cfg.base !== 'auto' ? cfg.base : process.env.GAUNTLET_BASE || null;
  const cands = [];
  if (explicit) cands.push([explicit, 'configured']);
  else {
    try {
      const prof = fs.readFileSync(pathOf(cfg, 'profile'), 'utf8').match(/\bbase[:：]\s*`?([\w./-]+)`?/i);
      if (prof) cands.push([prof[1], 'GAUNTLET.md'], [`origin/${prof[1]}`, 'GAUNTLET.md']);
    } catch { /* no profile yet */ }
    const head = git(['symbolic-ref', '--quiet', '--short', 'refs/remotes/origin/HEAD']).stdout.trim();
    if (head) cands.push([head, 'remote default branch']);
    for (const c of ['origin/main', 'origin/master', 'main', 'master', 'origin/develop', 'develop', 'origin/trunk', 'trunk']) cands.push([c, 'common name']);
  }
  for (const [cand, how] of cands) {
    const mb = git(['merge-base', 'HEAD', cand]);
    if (mb.code === 0) return { ref: mb.stdout.trim(), name: cand, how };
    if (how === 'configured') return null;   // an explicit base that does not exist is an error, not a hint
  }
  return null;
}

export function gitChangedLines(cfg, base) {
  // Map<relPath, Set<line>> of lines added/changed vs base. null when git or base is unavailable.
  const b = resolveBase(cfg, base);
  if (!b) return null;
  const ref = b.ref;
  const top = run('git', ['rev-parse', '--show-toplevel'], { cwd: cfg.root, quiet: true }).stdout.trim();
  const diff = run('git', ['diff', '-U0', '--no-color', ref, '--', '.'], { cwd: cfg.root, quiet: true, allowFail: true });
  if (diff.code !== 0) return null;
  const map = new Map();
  let cur = null;
  for (const line of diff.stdout.split(/\r?\n/)) {
    const f = line.match(/^\+\+\+ b\/(.*)$/);
    if (f) { cur = relToRoot(cfg, path.join(top, f[1])); if (!map.has(cur)) map.set(cur, new Set()); continue; }
    const h = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/);
    if (h && cur) {
      const start = +h[1]; const n = h[2] === undefined ? 1 : +h[2];
      for (let i = 0; i < n; i++) map.get(cur).add(start + i);
    }
  }
  // Untracked files count as fully changed.
  const untracked = run('git', ['ls-files', '--others', '--exclude-standard'], { cwd: cfg.root, quiet: true, allowFail: true });
  for (const f of untracked.stdout.split(/\r?\n/).filter(Boolean)) map.set(relToRoot(cfg, path.join(cfg.root, f)), 'all');
  return { ref, map, base: b };
}
