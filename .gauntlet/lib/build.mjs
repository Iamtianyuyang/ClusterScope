// Configure / build / test with clang source-based coverage, then export llvm-cov JSON.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run, ensureDir, writeJson, readJson } from './util.mjs';

export function configure(cfg, { force = false } = {}) {
  const build = cfg.abs(cfg.buildDir);
  if (!force && fs.existsSync(path.join(build, 'CMakeCache.txt'))) return;
  const args = [
    '-S', cfg.abs(cfg.sourceDir), '-B', build,
    '-G', cfg.cmake.generator,
    '-DGAUNTLET_COVERAGE=ON',
    '-DCMAKE_BUILD_TYPE=Debug',
    '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON',
    '-DCMAKE_C_COMPILER=clang',
    '-DCMAKE_CXX_COMPILER=clang++',
    ...cfg.cmake.configureArgs,
  ];
  run('cmake', args);
}

export function build(cfg, { quiet = false, allowFail = false } = {}) {
  return run('cmake', ['--build', cfg.abs(cfg.buildDir), ...cfg.cmake.buildArgs], { quiet, allowFail });
}

export function listTests(cfg) {
  const res = run('ctest', ['--test-dir', cfg.abs(cfg.buildDir), '--show-only=json-v1'], { quiet: true });
  const j = JSON.parse(res.stdout);
  return (j.tests || []).map((t) => ({ name: t.name, command: t.command || [], labels: (t.properties || []).find((p) => p.name === 'LABELS')?.value || [] }));
}

/** Run ctest with an explicit profile env. Returns {code, junit}. */
export function ctest(cfg, { profileDir, quiet = false, timeoutSec, extra = [] } = {}) {
  // Without a profileDir this is a throwaway run (mutation): discard coverage and acceptance reports.
  const env = profileDir
    ? { LLVM_PROFILE_FILE: path.join(profileDir, '%p-%m.profraw') }
    : { LLVM_PROFILE_FILE: path.join(os.tmpdir(), 'gauntlet-discard-%p.profraw'), GAUNTLET_REPORT_DIR: '-' };
  const junit = cfg.out('ctest-junit.xml');
  ensureDir(path.dirname(junit));
  const args = ['--test-dir', cfg.abs(cfg.buildDir), '--output-on-failure', '-j', String(Math.max(1, os.cpus().length)), '--output-junit', junit, ...extra];
  if (timeoutSec) args.push('--timeout', String(timeoutSec));
  const t0 = Date.now();
  const res = run('ctest', args, { quiet, allowFail: true, env });
  return { code: res.code, junit, ms: Date.now() - t0, stdout: res.stdout };
}

export function parseJunit(file) {
  if (!fs.existsSync(file)) return [];
  const xml = fs.readFileSync(file, 'utf8');
  const cases = [];
  const re = /<testcase\b([^>]*?)(\/>|>([\s\S]*?)<\/testcase>)/g;
  let m;
  while ((m = re.exec(xml))) {
    const attr = (k) => (m[1].match(new RegExp(`\\b${k}="([^"]*)"`)) || [])[1];
    const body = m[3] || '';
    const status = attr('status');
    const failed = /<failure\b/.test(body) || /<error\b/.test(body) || status === 'fail';
    const skipped = /<skipped\b/.test(body) || status === 'disabled' || status === 'notrun';
    cases.push({ name: decode(attr('name') || ''), classname: decode(attr('classname') || ''), time: Number(attr('time') || 0), status: failed ? 'failed' : skipped ? 'skipped' : 'passed' });
  }
  return cases;
}

const decode = (s) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&');

/** Full instrumented test run -> gauntlet-out/{tests.json, coverage.raw.json}. */
export function testWithCoverage(cfg) {
  const profDir = cfg.out('profraw');
  fs.rmSync(profDir, { recursive: true, force: true });
  ensureDir(profDir);
  const accDir = cfg.out('acceptance');
  fs.rmSync(accDir, { recursive: true, force: true });
  ensureDir(accDir);

  const res = ctest(cfg, { profileDir: profDir });
  const cases = parseJunit(res.junit);
  const tests = { code: res.code, ms: res.ms, total: cases.length, failed: cases.filter((c) => c.status === 'failed').length, cases };
  writeJson(cfg.out('tests.json'), tests);

  const raws = fs.readdirSync(profDir).filter((f) => f.endsWith('.profraw')).map((f) => path.join(profDir, f));
  if (!raws.length) throw new Error('no .profraw produced: is the project built with GAUNTLET_COVERAGE=ON and does ctest register any test?');
  const listFile = cfg.out('profraw.list');
  fs.writeFileSync(listFile, raws.join('\n') + '\n');
  const profdata = cfg.out('merged.profdata');
  run(cfg.llvm.profdata, ['merge', '-sparse', `--input-files=${listFile}`, '-o', profdata], { quiet: true });

  const bins = [...new Set(listTests(cfg).map((t) => t.command[0]).filter((b) => b && fs.existsSync(b)))];
  if (!bins.length) throw new Error('could not find any test executable from ctest --show-only');
  const objArgs = bins.slice(1).flatMap((b) => ['-object', b]);
  const exp = run(cfg.llvm.cov, ['export', bins[0], ...objArgs, `-instr-profile=${profdata}`, '-format=text'], { quiet: true });
  fs.writeFileSync(cfg.out('coverage.raw.json'), exp.stdout);
  return tests;
}

export function loadCoverage(cfg) {
  const j = readJson(cfg.out('coverage.raw.json'), null);
  if (!j) throw new Error('gauntlet-out/coverage.raw.json missing: run `node .gauntlet/gauntlet.mjs test` first');
  return j;
}
