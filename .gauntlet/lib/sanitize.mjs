// Run-time checkers the tests alone cannot replace:
//   host C/C++   AddressSanitizer + UndefinedBehaviorSanitizer (clang / gcc): cmake-clang builds a separate
//                <buildDir>-asan tree and runs the whole CTest suite; the commands adapter runs the project's
//                own `commands.sanitize` (a build + test run with sanitizers) and reads its output.
//   CUDA         NVIDIA compute-sanitizer (memcheck / racecheck / initcheck / synccheck) wraps every test of
//                the normal build (cmake-clang), so out-of-bounds device accesses and shared-memory races fail.
// mode "auto": a toolchain that cannot build with sanitizers, or a missing compute-sanitizer, is recorded as
// "not run" (the evidence pack asks the human); mode true makes that a failure. Findings always fail.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run, which, writeJson, listSources, relToRoot, makeSourceMatcher } from './util.mjs';
import { generate } from './gen.mjs';

const ASAN_RE = /ERROR: AddressSanitizer: ([\w-]+)|runtime error: (.*)|ERROR: LeakSanitizer: (.*)|WARNING: ThreadSanitizer: (.*)|ERROR: MemorySanitizer: (.*)/;
const FRAME_RE = /#\d+ 0x[0-9a-f]+ in \S+ ((?:[A-Za-z]:)?[^:\s]+):(\d+)/;
const CUDA_ERR_RE = /^=========\s+(Invalid __\w+__ \w+.*|Race reported.*|Uninitialized __\w+__ memory read.*|Barrier error.*|Program hit .*|.*error.*)$/i;
const CUDA_AT_RE = /=========\s+at (?:0x[0-9a-f]+ in )?((?:[A-Za-z]:)?[^:\s]+\.(?:cu|cuh|h|hpp|cpp|cc|c)):(\d+)/i;

const RUNTIME_BROKEN = /interception_win: unhandled instruction[^\n]*|AddressSanitizer: CHECK failed[^\n]*|AddressSanitizer failed to allocate[^\n]*|Shadow memory range interleaves[^\n]*|failed to intercept[^\n]*/;

const required = (cfg) => cfg.sanitize.mode === true;

/** Sanitizer reports in any program output -> findings located in production code where possible. */
export function parseSanitizerOutput(cfg, text, baseDir, tool = 'asan/ubsan') {
  const findings = [];
  const isSrc = makeSourceMatcher(cfg);
  const lines = text.split(/\r?\n/);
  lines.forEach((l, i) => {
    const m = ASAN_RE.exec(l);
    if (!m) return;
    const kind = m[1] ? `AddressSanitizer: ${m[1]}` : m[2] ? `UndefinedBehavior: ${m[2]}` : m[3] ? `LeakSanitizer: ${m[3]}` : m[4] ? `ThreadSanitizer: ${m[4]}` : `MemorySanitizer: ${m[5]}`;
    // first stack frame inside production code (or the runtime-error location itself)
    let where = /^((?:[A-Za-z]:)?[^:\s]+):(\d+):\d+: runtime error/.exec(l.trim());
    for (let k = i + 1; !where && k < Math.min(lines.length, i + 40); k++) {
      const f = FRAME_RE.exec(lines[k]);
      if (f && isSrc(relToRoot(cfg, path.resolve(baseDir, f[1])))) where = f;
    }
    findings.push({ tool, kind: kind.slice(0, 200), file: where ? relToRoot(cfg, path.resolve(baseDir, where[1])) : '?', line: where ? +where[2] : 0, excerpt: l.trim().slice(0, 300) });
  });
  return findings;
}

function listTestCommands(buildDir) {
  const res = run('ctest', ['--test-dir', buildDir, '--show-only=json-v1'], { quiet: true });
  const j = JSON.parse(res.stdout);
  return (j.tests || []).map((t) => {
    const props = Object.fromEntries((t.properties || []).map((p) => [p.name, p.value]));
    const env = {};
    for (const kv of props.ENVIRONMENT || []) { const i = kv.indexOf('='); env[kv.slice(0, i)] = kv.slice(i + 1); }
    return { name: t.name, command: t.command || [], cwd: props.WORKING_DIRECTORY || buildDir, env, willFail: props.WILL_FAIL === true };
  }).filter((t) => t.command.length);
}

function hostSanitizers(cfg) {
  const bdir = cfg.abs(`${cfg.buildDir}-asan`);
  if (!fs.existsSync(path.join(bdir, 'CMakeCache.txt'))) {
    // Windows: clang's ASan runtime cannot link the debug CRT (_ITERATOR_DEBUG_LEVEL mismatch), so the
    // sanitizer tree uses the release CRT there; elsewhere Debug keeps assertions on.
    const crt = process.platform === 'win32' ? ['-DCMAKE_BUILD_TYPE=RelWithDebInfo', '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL'] : ['-DCMAKE_BUILD_TYPE=Debug'];
    const c = run('cmake', ['-S', cfg.abs(cfg.sourceDir), '-B', bdir, '-G', cfg.cmake.generator, '-DGAUNTLET_SANITIZE=ON', ...crt,
      '-DCMAKE_C_COMPILER=clang', '-DCMAKE_CXX_COMPILER=clang++', ...cfg.cmake.configureArgs], { allowFail: true, quiet: true });
    if (c.code !== 0) return { ran: false, reason: `sanitizer 构建配置失败：${(c.stdout + c.stderr).split(/\r?\n/).filter((l) => /error/i.test(l)).slice(0, 3).join(' | ')}`, findings: [] };
  }
  generate(cfg);
  const b = run('cmake', ['--build', bdir, ...cfg.cmake.buildArgs], { allowFail: true, quiet: true });
  if (b.code !== 0) {
    fs.rmSync(path.join(bdir, 'CMakeCache.txt'), { force: true });
    return { ran: false, reason: `工具链不能带 sanitizer 构建：${(b.stdout + b.stderr).split(/\r?\n/).filter((l) => /error/i.test(l)).slice(0, 3).join(' | ')}`, findings: [] };
  }
  const env = {
    ASAN_OPTIONS: 'abort_on_error=1:detect_stack_use_after_return=1:strict_init_order=1' + (process.platform === 'linux' ? ':detect_leaks=1' : ''),
    UBSAN_OPTIONS: 'print_stacktrace=1:halt_on_error=1',
    LLVM_PROFILE_FILE: path.join(os.tmpdir(), 'gauntlet-asan-%p.profraw'),
    GAUNTLET_REPORT_DIR: '-',
  };
  if (process.platform === 'win32') {
    // the DLL sanitizer runtime lives in clang's resource dir: tests must find it at run time
    const res = run(cfg.llvm.clang, ['-print-resource-dir'], { quiet: true, allowFail: true }).stdout.trim();
    const dir = res && path.join(res, 'lib', 'windows');
    if (dir && fs.existsSync(dir)) env.PATH = `${dir}${path.delimiter}${process.env.PATH || process.env.Path || ''}`;
  }
  const res = run('ctest', ['--test-dir', bdir, '--output-on-failure', '-j', String(Math.max(1, os.cpus().length))], { quiet: true, allowFail: true, env });
  const findings = parseSanitizerOutput(cfg, res.stdout + res.stderr, bdir);
  // the sanitizer runtime itself cannot work on this machine (typically a compiler older than the OS):
  // that is an environment problem for the human, not a finding in the code
  const broken = (res.stdout + res.stderr).match(RUNTIME_BROKEN);
  if (res.code !== 0 && !findings.length && broken) {
    return { ran: false, reason: `sanitizer 运行时在这台机器上不可用（${broken[0].trim().slice(0, 120)}）：通常是编译器自带的 sanitizer 运行时比操作系统旧，升级 LLVM 后重试`, findings: [] };
  }
  const failedTests = [...(res.stdout.matchAll(/\d+ - (\S+) \((Failed|SEGFAULT|Subprocess aborted|Exception|Timeout)\)/g))].map((m) => m[1]);
  if (res.code !== 0 && !findings.length) findings.push({ tool: 'asan/ubsan', kind: 'test failed under sanitizers', file: '?', line: 0, excerpt: failedTests.join(', ') || 'see ctest output' });
  return { ran: true, findings, failedTests };
}

/** compute-sanitizer on PATH, or next to nvcc in the CUDA toolkit (<cuda>/compute-sanitizer/). */
export function computeSanitizerExe(cfg) {
  const direct = which(cfg.sanitize.computeSanitizer);
  if (direct) return direct;
  const nvcc = which('nvcc');
  if (!nvcc) return null;
  const root = path.resolve(path.dirname(nvcc), '..');
  for (const cand of ['compute-sanitizer/compute-sanitizer.exe', 'compute-sanitizer/compute-sanitizer', 'bin/compute-sanitizer']) {
    if (fs.existsSync(path.join(root, cand))) return path.join(root, cand);
  }
  return null;
}

function cudaSanitizers(cfg) {
  const exe = computeSanitizerExe(cfg);
  if (!exe) return { ran: false, reason: 'compute-sanitizer 没找到（PATH 和 CUDA 安装目录里都没有）', findings: [] };
  const tests = listTestCommands(cfg.abs(cfg.buildDir));
  const findings = [];
  const isSrc = makeSourceMatcher(cfg);
  for (const tool of cfg.sanitize.cudaTools) {
    for (const t of tests) {
      const r = spawnSync(exe, ['--tool', tool, '--error-exitcode', '86', ...(tool === 'memcheck' ? ['--leak-check', 'full'] : []), ...t.command], {
        cwd: t.cwd, env: { ...process.env, ...t.env, LLVM_PROFILE_FILE: path.join(os.tmpdir(), 'gauntlet-cs-%p.profraw'), GAUNTLET_REPORT_DIR: '-' },
        encoding: 'utf8', maxBuffer: 256 << 20, timeout: (cfg.sanitize.testTimeoutSec || 1800) * 1000,
      });
      if (r.status !== 86) continue; // only compute-sanitizer's own error exit counts here
      const lines = (r.stdout + r.stderr).split(/\r?\n/);
      lines.forEach((l, i) => {
        const m = CUDA_ERR_RE.exec(l);
        if (!m || /ERROR SUMMARY|0 errors/i.test(l)) return;
        let at = null;
        for (let k = i + 1; !at && k < Math.min(lines.length, i + 30); k++) {
          const a = CUDA_AT_RE.exec(lines[k]);
          if (a && isSrc(relToRoot(cfg, path.resolve(t.cwd, a[1])))) at = a;
        }
        findings.push({ tool: `compute-sanitizer ${tool}`, kind: m[1].trim().slice(0, 160), test: t.name, file: at ? relToRoot(cfg, path.resolve(t.cwd, at[1])) : '?', line: at ? +at[2] : 0, excerpt: l.trim().slice(0, 300) });
      });
    }
  }
  return { ran: true, findings, tests: tests.length };
}

function finish(cfg, host, cuda) {
  const seen = new Set();
  const findings = [...host.findings, ...cuda.findings].filter((f) => { const k = `${f.tool}|${f.kind}|${f.file}:${f.line}`; if (seen.has(k)) return false; seen.add(k); return true; });
  const notRun = [host, cuda].filter((x) => x.expected && !x.ran);
  const pass = findings.length === 0 && (!required(cfg) || notRun.length === 0);
  const r = {
    pass, mode: cfg.sanitize.mode,
    ran: host.ran || cuda.ran,
    host: { ran: host.ran, reason: host.reason, failedTests: host.failedTests, command: host.command },
    cuda: { ran: cuda.ran, reason: cuda.reason, tests: cuda.tests, tools: cfg.sanitize.cudaTools },
    notRun: notRun.map((x) => x.reason).filter(Boolean),
    findings,
  };
  writeJson(cfg.out('sanitize.json'), r);
  return r;
}

/** cmake-clang: ASan/UBSan build + compute-sanitizer for CUDA projects. */
export function runSanitize(cfg) {
  const hasCuda = listSources(cfg).some((f) => /\.(cu|cuh)$/i.test(f));
  const host = cfg.sanitize.host === false ? { ran: false, findings: [] } : { expected: true, ...hostSanitizers(cfg) };
  const cuda = !hasCuda || cfg.sanitize.cuda === false ? { ran: false, findings: [] } : { expected: true, ...cudaSanitizers(cfg) };
  return finish(cfg, host, cuda);
}

/** commands adapter: the project's own sanitizer run; null when the project does not configure one. */
export async function runSanitizeCommand(cfg, shell, spec) {
  if (!spec?.run) return null;
  const r = await shell(cfg, spec, { quiet: true, timeoutMs: (cfg.sanitize.testTimeoutSec || 1800) * 1000 });
  const findings = parseSanitizerOutput(cfg, `${r.stdout}\n${r.stderr}`, cfg.abs(spec.cwd || '.'), spec.name || 'sanitizer');
  if (r.code !== 0 && !findings.length) findings.push({ tool: spec.name || 'sanitizer', kind: `sanitizer run failed (exit ${r.code})`, file: '?', line: 0, excerpt: `${r.stdout}\n${r.stderr}`.trim().split(/\r?\n/).slice(-3).join(' | ').slice(0, 300) });
  return finish(cfg, { expected: true, ran: true, findings, command: spec.run }, { ran: false, findings: [] });
}

export function printSanitize(r) {
  if (!r) { console.log('\nSANITIZE: 没有配置（commands.sanitize）'); return; }
  console.log(`\nSANITIZE  host=${r.host.ran ? 'ran' : `not run${r.host.reason ? `: ${r.host.reason}` : ''}`}  cuda=${r.cuda.ran ? `ran on ${r.cuda.tests} test(s): ${r.cuda.tools.join('/')}` : `not run${r.cuda.reason ? ` (${r.cuda.reason})` : ''}`}  findings=${r.findings.length}`);
  for (const f of r.findings.slice(0, 20)) console.log(`  ✗ [${f.tool}] ${f.kind}  ${f.file}:${f.line}${f.test ? `  (test ${f.test})` : ''}`);
  console.log(r.pass ? 'SANITIZE gate: PASS' : 'SANITIZE gate: FAIL');
}
