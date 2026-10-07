#!/usr/bin/env node
// Gauntlet gate helper for this Rust workspace.
// Runs every test target through the project's own toolchain (cargo + rustc) with LLVM
// source-based coverage instrumentation, and writes the two standard reports the "commands"
// adapter expects:
//
//   <out>/junit.xml   every test of every target (unit / integration / doc) with pass-fail-skip
//   <out>/lcov.info   line coverage of the instrumented crates
//
// Usage: node gauntlet-tools/rust-gate.mjs --out gauntlet-out
//
// Why not cargo-llvm-cov / cargo-nextest: this machine has no network access, so no extra cargo
// subcommand can be installed. The pipeline below is what cargo-llvm-cov does internally:
//
//   RUSTFLAGS=-Cinstrument-coverage + LLVM_PROFILE_FILE -> llvm-profdata merge -> llvm-cov export
//
// The LLVM binaries come from the system clang install (/usr/bin/llvm-profdata[.21],
// /usr/bin/llvm-cov[.21]); they read this rustc's (LLVM 22) profraw files unchanged - verified
// during the stage-0 survey. Coverage is exported from the workspace's own instrumented test
// binaries only, so registry sources never enter the report.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const argv = process.argv.slice(2);
const argOf = (name, dflt) => {
  const i = argv.indexOf(name);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : dflt;
};
const out = path.resolve(argOf('--out', 'gauntlet-out'));
const repo = path.resolve(argOf('--root', '.'));
const quiet = argv.includes('--quiet');

// ---------------------------------------------------------------- process helpers

function sh(cmd, args, { env = {}, cwd = repo } = {}) {
  const r = spawnSync(cmd, args, {
    cwd,
    env: { ...process.env, ...env },
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
  });
  return { code: r.status ?? (r.error ? 127 : 1), stdout: r.stdout || '', stderr: r.stderr || '', error: r.error };
}

function findTool(names) {
  for (const n of names) {
    const r = sh('sh', ['-c', `command -v ${n}`], { cwd: '/' });
    if (r.code === 0 && r.stdout.trim()) return r.stdout.trim().split('\n')[0].trim();
  }
  return null;
}

const log = (...a) => { if (!quiet) console.log(...a); };

// ---------------------------------------------------------------- arguments / environment

fs.mkdirSync(out, { recursive: true });
const profrawDir = path.join(out, 'profraw');
fs.rmSync(profrawDir, { recursive: true, force: true });
fs.mkdirSync(profrawDir, { recursive: true });

const cargoBin = path.join(os.homedir(), '.cargo', 'bin');
const env = {
  PATH: process.env.PATH?.includes(cargoBin) ? process.env.PATH : `${cargoBin}:${process.env.PATH || ''}`,
  RUSTFLAGS: `${process.env.RUSTFLAGS || ''} -Cinstrument-coverage`.trim(),
  LLVM_PROFILE_FILE: path.join(profrawDir, 'cov-%p-%m.profraw'),
  CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || path.join(out, 'cov-target'),
  CARGO_TERM_COLOR: 'never',
};

// ---------------------------------------------------------------- 1. build all test targets

log(`$ cargo test --workspace --offline --no-run   (CARGO_TARGET_DIR=${env.CARGO_TARGET_DIR})`);
const t0 = Date.now();
const build = sh('cargo', ['test', '--workspace', '--offline', '--no-run', '--message-format=json'], { env });
const buildMs = Date.now() - t0;

const artifacts = [];
const rendered = [];
for (const line of build.stdout.split('\n')) {
  const s = line.trim();
  if (!s.startsWith('{')) continue;
  let m;
  try { m = JSON.parse(s); } catch { continue; }
  if (m.reason === 'compiler-artifact' && m.executable) {
    // `cargo test` also builds plain product binaries: every bin an integration test references
    // through `env!("CARGO_BIN_EXE_<name>")` is built as a real (non-test) executable and shows up
    // in this stream with `profile.test === false`. Those are products, not test harnesses: libtest
    // never runs, and a daemon like clusterscope-agent simply never returns -- spawning it would
    // hang the whole gate. Only `profile.test === true` artifacts are executed; the others stay in
    // the list below so the coverage export still has their object files.
    artifacts.push({
      exe: m.executable,
      name: m.target?.name || path.basename(m.executable),
      kind: (m.target?.kind || []).join('/'),
      isTest: m.profile?.test === true,
    });
  } else if (m.reason === 'compiler-message' && m.message?.rendered) {
    rendered.push(m.message.rendered);
  }
}
const testArtifacts = artifacts.filter((a) => a.isTest);
log(`  build+link of test targets: ${(buildMs / 1000).toFixed(1)}s, ${testArtifacts.length} test binaries${artifacts.length > testArtifacts.length ? ` (+${artifacts.length - testArtifacts.length} product binary/binaries, built for CARGO_BIN_EXE_*, not run)` : ''}`);

const suites = [];
let compileFailed = false;
if (build.code !== 0) {
  compileFailed = true;
  const diag = rendered.length ? rendered.join('') : `${build.stderr}\n${build.stdout}`;
  suites.push({
    name: 'cargo test --no-run (build)',
    tests: [{ name: 'build workspace test targets', status: 'failed', time: 0, message: 'cargo test --no-run failed', detail: tailLines(diag) }],
    time: buildMs / 1000,
  });
} else {
  // ------------------------------------------------------------ 2. run each test binary
  for (const a of [...artifacts].filter((x) => x.isTest).sort((x, y) => x.exe.localeCompare(y.exe))) {
    const label = `${a.name} (${a.kind})`;
    log(`$ ${a.exe}`);
    const s0 = Date.now();
    const r = sh(a.exe, [], { env });
    const ms = Date.now() - s0;
    suites.push({ name: label, ...parseLibtest(`${r.stdout}\n${r.stderr}`, r.code, ms) });
  }

  // ------------------------------------------------------------ 3. doc tests (not instrumented)
  const doc = sh('cargo', ['test', '--workspace', '--offline', '--doc'], { env: { ...env, RUSTFLAGS: '', LLVM_PROFILE_FILE: '' } });
  const docText = `${doc.stdout}\n${doc.stderr}`;
  for (const m of docText.matchAll(/^\s*Doc-tests ([^\s]+)/gm)) {
    // one suite per crate; the block runs until the next Doc-tests header
    const start = m.index;
    const next = docText.indexOf('Doc-tests ', start + 10);
    const block = docText.slice(start, next < 0 ? undefined : next);
    suites.push({ name: `Doc-tests ${m[1]}`, ...parseLibtest(block, 0, 0) });
  }
  if (doc.code !== 0 && !suites.some((s) => s.name.startsWith('Doc-tests'))) {
    suites.push({ name: 'Doc-tests', tests: [{ name: 'cargo test --doc', status: 'failed', time: 0, message: 'cargo test --doc failed', detail: tailLines(docText) }], time: 0 });
  }
}

// ---------------------------------------------------------------- 4. JUnit XML

const all = suites.flatMap((s) => s.tests);
const counts = {
  tests: all.length,
  failures: all.filter((t) => t.status === 'failed').length,
  skipped: all.filter((t) => t.status === 'skipped').length,
};
const xml = [];
xml.push('<?xml version="1.0" encoding="utf-8"?>');
xml.push(`<testsuites name="cargo test --workspace" tests="${counts.tests}" failures="${counts.failures}" errors="0" skipped="${counts.skipped}">`);
for (const s of suites) {
  const f = s.tests.filter((t) => t.status === 'failed').length;
  const k = s.tests.filter((t) => t.status === 'skipped').length;
  xml.push(`  <testsuite name="${esc(s.name)}" tests="${s.tests.length}" failures="${f}" errors="0" skipped="${k}" time="${(s.time || 0).toFixed(3)}">`);
  for (const t of s.tests) {
    const attrs = `classname="${esc(s.name)}" name="${esc(t.name)}" time="${(t.time || 0).toFixed(3)}"`;
    if (t.status === 'failed') {
      xml.push(`    <testcase ${attrs}><failure message="${esc(t.message || 'test failed')}">${esc(t.detail || '')}</failure></testcase>`);
    } else if (t.status === 'skipped') {
      xml.push(`    <testcase ${attrs}><skipped message="${esc(t.message || 'ignored')}"/></testcase>`);
    } else {
      xml.push(`    <testcase ${attrs}/>`);
    }
  }
  xml.push('  </testsuite>');
}
xml.push('</testsuites>');
fs.writeFileSync(path.join(out, 'junit.xml'), xml.join('\n') + '\n');

// ---------------------------------------------------------------- 5. coverage

const profraws = fs.readdirSync(profrawDir).filter((f) => f.endsWith('.profraw')).map((f) => path.join(profrawDir, f));
let covNote = '';
if (!artifacts.length || !profraws.length) {
  covNote = `no coverage: ${profraws.length} profraw file(s), ${artifacts.length} test binary(ies)`;
} else {
  const profdata = findTool(['llvm-profdata', 'llvm-profdata-21']);
  const llvmcov = findTool(['llvm-cov', 'llvm-cov-21']);
  if (!profdata || !llvmcov) {
    covNote = `no coverage: ${profdata ? '' : 'llvm-profdata'} ${llvmcov ? '' : 'llvm-cov'} not found`;
  } else {
    const merged = path.join(out, 'coverage.profdata');
    const listFile = path.join(out, 'profraw.list.txt');
    fs.writeFileSync(listFile, profraws.join('\n') + '\n');
    const m = sh(profdata, ['merge', '--sparse', `--input-files=${listFile}`, '-o', merged]);
    if (m.code !== 0) {
      covNote = `no coverage: llvm-profdata merge failed: ${tailLines(m.stderr, 5)}`;
    } else {
      const exes = artifacts.map((a) => a.exe);
      const args = ['export', `--instr-profile=${merged}`, '--format=lcov',
        // drop dependency sources (registry / rust std / rustlib): the kit keeps only files inside this repo anyway
        '--ignore-filename-regex=(^|/)(\\.cargo|rustc|rustlib)/',
        ...exes.flatMap((e) => ['-object', e])];
      const e = sh(llvmcov, args);
      if (e.code !== 0 || !e.stdout.includes('SF:')) {
        covNote = `no coverage: llvm-cov export failed: ${tailLines(e.stderr || e.stdout, 5)}`;
      } else {
        fs.writeFileSync(path.join(out, 'lcov.info'), e.stdout);
        const files = (e.stdout.match(/^SF:/gm) || []).length;
        covNote = `${files} instrumented file(s) in lcov.info`;
      }
    }
  }
}

// ---------------------------------------------------------------- 6. summary

const per = suites.map((s) => `${s.name}: ${s.tests.filter((t) => t.status === 'passed').length}/${s.tests.length}`).join(', ');
log(`tests: ${counts.tests} total, ${counts.failures} failed, ${counts.skipped} ignored -> ${path.join(out, 'junit.xml')}`);
log(`  ${per}`);
log(`coverage: ${covNote}`);
if (counts.failures) {
  const failed = all.filter((t) => t.status === 'failed');
  log(`failed tests: ${failed.map((t) => t.name).slice(0, 20).join(', ')}`);
}

process.exit(compileFailed || counts.failures ? 1 : 0);

// ---------------------------------------------------------------- helpers

function esc(s) {
  return String(s)
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, '')
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;').replace(/'/g, '&apos;');
}

function tailLines(text, n = 40) {
  const lines = String(text).split(/\r?\n/).filter((l) => l.trim());
  return lines.slice(-n).join('\n');
}

/** libtest's human-readable output -> one test record per "test <name> ... <status>" line. */
function parseLibtest(text, code, ms) {
  const tests = [];
  const details = new Map();
  // failure blocks:  ---- <name> stdout ----  ... until the next ---- or "failures:"
  const blocks = [...text.matchAll(/^---- (.+?) (?:stdout|stderr) ----$/gm)];
  blocks.forEach((b, i) => {
    const from = b.index + b[0].length;
    const to = i + 1 < blocks.length ? blocks[i + 1].index : text.length;
    const body = text.slice(from, to).replace(/^failures:$/m, '');
    details.set(b[1], (details.get(b[1]) || '') + body.trim() + '\n');
  });
  for (const line of text.split(/\r?\n/)) {
    const m = /^test\s+(.+?)\s+\.\.\.\s+(ok|FAILED|ignored|allowed_fail|bench:.*)$/.exec(line.trim());
    if (!m) continue;
    const [, name, state] = m;
    if (state === 'ok' || state === 'allowed_fail') tests.push({ name, status: 'passed', time: 0 });
    else if (state === 'FAILED') tests.push({ name, status: 'failed', time: 0, message: 'FAILED', detail: details.get(name) || '' });
    else tests.push({ name, status: 'skipped', time: 0, message: line.trim() });
  }
  if (!tests.length && code !== 0) {
    tests.push({ name: 'harness', status: 'failed', time: 0, message: `test binary exited with ${code}`, detail: tailLines(text) });
  }
  return { tests, time: ms / 1000 };
}
