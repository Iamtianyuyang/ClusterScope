// The universal adapter ("adapter": "commands"): any language, any build system.
// The project says HOW to build / test / lint in gauntlet.config.json; the tools only have to write
// standard reports (JUnit, LCOV/Cobertura, SARIF, mutation-testing-report-schema). This module turns
// those reports into the same gauntlet-out/*.json files the CMake+clang adapter writes, so every gate,
// `next` and the evidence pack work unchanged.
//
//   "adapter": "commands",
//   "commands": {
//     "build":    "make -j8",                                         (optional)
//     "test":     { "run": "...", "junit": ["{out}/junit.xml"], "lcov": ["{out}/lcov.info"] },   (or "cobertura")
//     "lint":     [{ "name": "eslint", "run": "...", "sarif": "{out}/eslint.sarif", "gate": "lint" | "warnings" },
//                  { "name": "compiler", "run": "<a full build>", "parse": "diagnostics", "gate": "warnings" }],
//                  parse "diagnostics" = read file:line: warning: ... lines of ANY compiler from the command output
//     "arch":     { "run": "npx depcruise src --config" },             (optional; exit code 0 = no violation)
//     "mutation": { "run": "npx stryker run", "report": "reports/mutation/mutation.json" },  (optional; default: built-in)
//     "mutationTest": "..."                                            (optional faster test command for built-in mutation)
//   }
// {out} / {root} in commands and report paths expand to the output directory / project root.
import fs from 'node:fs';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { listSources, makeSourceMatcher, relToRoot, writeJson, readJson, toPosix, ensureDir } from './util.mjs';
import { parseFeatureDir } from './gherkin.mjs';
import { irOf } from './gen.mjs';
import { codeLines, loadAccepted } from './static.mjs';
import { crapScore } from './crap.mjs';
import { analyzeFile, runLizard, lizardExe } from './complexity.mjs';
import { langOf } from './lang.mjs';
import { parseJunit, parseLcov, parseCobertura, parseSarif, parseDiagnostics, parseMutationElements, resolveReported } from './formats.mjs';
import { mutateWith, functionRanges, writeMutationReport } from './mutate.mjs';

const list = (x) => (x == null ? [] : Array.isArray(x) ? x : [x]);
export const cmdSpec = (x) => (x == null ? null : typeof x === 'string' ? { run: x } : x);

function subst(cfg, s, cwd = cfg.root) {
  const rel = (p) => toPosix(path.relative(cwd, p)) || '.';
  return String(s).replace(/\{out\}/g, rel(cfg.out())).replace(/\{root\}/g, rel(cfg.root));
}
const reportPath = (cfg, p) => path.resolve(cfg.root, subst(cfg, p));

function killTree(pid) {
  if (process.platform === 'win32') spawnSync('taskkill', ['/pid', String(pid), '/T', '/F'], { windowsHide: true });
  else { try { process.kill(-pid, 'SIGKILL'); } catch { /* already gone */ } }
}

/** Run one configured command through the shell. Never throws; the caller decides what a failure means. */
export function shell(cfg, spec, { quiet = false, timeoutMs = 0 } = {}) {
  const s = cmdSpec(spec);
  const cwd = cfg.abs(s.cwd || '.');
  const cmd = subst(cfg, s.run, cwd);
  if (!quiet) console.log(`$ ${cmd}`);
  return new Promise((resolve) => {
    const t0 = Date.now();
    const child = spawn(cmd, {
      cwd, shell: true, windowsHide: true, detached: process.platform !== 'win32',
      env: { ...process.env, GAUNTLET_OUT: cfg.out(), GAUNTLET_ROOT: cfg.root, ...Object.fromEntries(Object.entries(s.env || {}).map(([k, v]) => [k, subst(cfg, v, cwd)])) },
    });
    let stdout = '', stderr = '', timedOut = false;
    child.stdout.on('data', (d) => { stdout += d; if (!quiet) process.stdout.write(d); });
    child.stderr.on('data', (d) => { stderr += d; if (!quiet) process.stderr.write(d); });
    const timer = timeoutMs ? setTimeout(() => { timedOut = true; killTree(child.pid); }, timeoutMs) : null;
    const done = (code, extra = '') => { if (timer) clearTimeout(timer); resolve({ code, stdout, stderr: stderr + extra, timedOut, ms: Date.now() - t0 }); };
    child.on('error', (e) => done(127, e.message));
    child.on('close', (code) => done(code ?? 1));
  });
}

const tail = (r, n = 15) => (r.stdout + r.stderr).split(/\r?\n/).filter((l) => l.trim()).slice(-n).join('\n');

// ---------------- spec / build ----------------

/** Gherkin is language-neutral: the gate only checks that the features parse and contain scenarios. */
export function specGeneric(cfg) {
  const features = parseFeatureDir(cfg.abs(cfg.features));
  const scenarios = features.reduce((n, f) => n + f.scenarios.length, 0);
  console.log(`spec: ${features.length} feature(s), ${scenarios} scenario(s)（commands 适配器：场景与测试按名称对应）`);
  return { pass: scenarios > 0, scenarios, undefinedSteps: 0, ambiguous: 0 };
}

export async function buildGeneric(cfg) {
  const b = cmdSpec(cfg.commands?.build);
  if (!b) return;
  const r = await shell(cfg, b);
  if (r.code !== 0) throw new Error(`build failed (exit ${r.code}):\n${tail(r)}`);
}

// ---------------- tests + coverage + acceptance ----------------

function readJunit(cfg, files) {
  const cases = [];
  for (const f of files) {
    const abs = reportPath(cfg, f);
    if (!fs.existsSync(abs)) continue;
    const xmls = fs.statSync(abs).isDirectory() ? fs.readdirSync(abs).filter((x) => x.endsWith('.xml')).map((x) => path.join(abs, x)) : [abs];
    for (const x of xmls) cases.push(...parseJunit(x));
  }
  return cases;
}

function readCoverage(cfg, t, cwd) {
  const files = {};
  const put = (rel, map) => {
    const cur = files[rel] || (files[rel] = {});
    for (const [l, h] of map) cur[l] = Math.max(cur[l] || 0, h);
  };
  let source = null;
  for (const f of list(t.lcov)) {
    const abs = reportPath(cfg, f);
    if (!fs.existsSync(abs)) continue;
    source = 'lcov';
    for (const [p, m] of parseLcov(fs.readFileSync(abs, 'utf8'))) put(resolveReported(cfg, p, [cwd, path.dirname(abs)]), m);
  }
  for (const f of list(t.cobertura)) {
    const abs = reportPath(cfg, f);
    if (!fs.existsSync(abs)) continue;
    source = 'cobertura';
    const { files: fm, sources } = parseCobertura(fs.readFileSync(abs, 'utf8'));
    for (const [p, m] of fm) put(resolveReported(cfg, p, [...sources.map((s) => path.resolve(cwd, s)), cwd]), m);
  }
  const isSrc = makeSourceMatcher(cfg);
  for (const k of Object.keys(files)) if (!isSrc(k)) delete files[k];
  return { source, files };
}

/**
 * Acceptance contract of the commands adapter: every Gherkin scenario has a test whose name contains
 * the scenario name (cucumber/behave/pytest-bdd do this by default; plain tests just use the name).
 * A Scenario Outline needs one test per Examples row (matched in order).
 */
export function matchAcceptance(cfg, cases) {
  const features = irOf(cfg, parseFeatureDir(cfg.abs(cfg.features))).features;
  const scenarios = features.flatMap((f) => f.scenarios.map((s) => ({ ...s, feature: f.name, file: f.file })));
  const norm = (s) => String(s).replace(/\s+/g, ' ').trim().toLowerCase();
  const groups = new Map();
  for (const s of scenarios) {
    const base = s.name.replace(/ \[[^\]]*\]$/, '');
    (groups.get(base) || groups.set(base, []).get(base)).push(s);
  }
  const used = new Set();
  const accDir = cfg.out('acceptance');
  fs.rmSync(accDir, { recursive: true, force: true });
  ensureDir(accDir);
  const missing = [], failed = [];
  let passed = 0;
  // longest names first, so "加法溢出" is not taken by "加法"
  for (const [base, list] of [...groups].sort((a, b) => b[0].length - a[0].length)) {
    const cand = cases.map((c, i) => [c, i]).filter(([c, i]) => !used.has(i) && (norm(c.name).includes(norm(base)) || norm(`${c.classname || ''} ${c.name}`).includes(norm(base))));
    list.forEach((s, k) => {
      const hit = cand[k];
      if (!hit) { missing.push(s.name); return; }
      const [c, i] = hit;
      used.add(i);
      const status = c.status;
      if (status === 'passed') passed++; else failed.push(s.name);
      c.title = c.name;
      c.name = `acc.${s.id}`;
      writeJson(path.join(accDir, `${s.id}.json`), {
        id: s.id, feature: s.feature, name: s.name, location: `${s.file}:${s.line}`, status, ms: (c.time || 0) * 1000, test: c.title,
        steps: s.steps.map((st) => ({ keyword: st.keyword, text: st.text, status: status === 'passed' ? 'passed' : 'skipped', message: '' })),
      });
    });
  }
  return { pass: scenarios.length > 0 && !missing.length && !failed.length, scenarios: scenarios.length, passed, failed, missing };
}

export async function testsGeneric(cfg) {
  const t = cmdSpec(cfg.commands?.test);
  if (!t?.run) throw new Error('commands.test.run 未配置：告诉 gauntlet 怎么跑测试（并输出 JUnit XML）');
  if (!list(t.junit).length) throw new Error('commands.test.junit 未配置：测试命令必须输出 JUnit XML，并在这里写出路径');
  for (const f of [...list(t.junit), ...list(t.lcov), ...list(t.cobertura)]) fs.rmSync(reportPath(cfg, f), { recursive: true, force: true });
  ensureDir(cfg.out());
  const cwd = cfg.abs(t.cwd || '.');
  const res = await shell(cfg, t);
  const cases = readJunit(cfg, list(t.junit));
  let acceptance;
  try { acceptance = matchAcceptance(cfg, cases); } catch (e) { acceptance = { pass: false, error: e.message }; }
  const tests = { code: res.code, ms: res.ms, total: cases.length, failed: cases.filter((c) => c.status === 'failed').length, cases };
  writeJson(cfg.out('tests.json'), tests);
  const cov = readCoverage(cfg, t, cwd);
  writeJson(cfg.out('coverage.lines.json'), cov);
  if (!cases.length) console.log(`\n✗ 测试命令没有产生 JUnit 报告（${list(t.junit).join(', ')}）`);
  return { tests, acceptance };
}

// ---------------- lint (SARIF) ----------------

let lintCache = null;

/** Run every commands.lint entry once per process -> findings tagged with their gate. */
export async function lintGeneric(cfg) {
  if (lintCache) return lintCache;
  const isSrc = makeSourceMatcher(cfg);
  const findings = [];
  const ran = [];
  const errors = [];
  for (const raw of list(cfg.commands?.lint)) {
    const l = cmdSpec(raw);
    const name = l.name || String(l.run).split(/\s+/)[0];
    if (l.sarif) fs.rmSync(reportPath(cfg, l.sarif), { force: true });
    const r = await shell(cfg, l, { quiet: true });
    ran.push(name);
    if (l.parse === 'diagnostics') {
      for (const d of parseDiagnostics(`${r.stdout}\n${r.stderr}`)) {
        const file = resolveReported(cfg, d.uri, [cfg.abs(l.cwd || '.')]);
        if (!isSrc(file)) continue;
        findings.push({ gate: l.gate || 'warnings', file, line: d.line, col: d.col, check: `${name}${d.ruleId ? `/${d.ruleId}` : ''}`, message: `${d.level}: ${d.message}` });
      }
      if (r.code !== 0 && !findings.some((f) => f.check.startsWith(name))) errors.push(`${name}: 命令失败（exit ${r.code}）\n${tail(r, 5)}`);
      continue;
    }
    if (!l.sarif) { if (r.code !== 0) findings.push({ gate: l.gate || 'lint', file: '-', line: 0, col: 0, check: name, message: tail(r, 5) }); continue; }
    const abs = reportPath(cfg, l.sarif);
    if (!fs.existsSync(abs)) { errors.push(`${name}: 没有生成 ${l.sarif}（exit ${r.code}）\n${tail(r, 5)}`); continue; }
    for (const f of parseSarif(readJson(abs, {}))) {
      const file = resolveReported(cfg, f.uri, [cfg.abs(l.cwd || '.')]);
      if (!isSrc(file) || f.level === 'none' || f.level === 'note') continue;
      findings.push({ gate: l.gate || 'lint', file, line: f.line, col: f.col, check: `${f.tool}/${f.ruleId}`, message: f.message });
    }
  }
  const accepted = loadAccepted(cfg);
  for (const f of findings) {
    const kind = f.gate === 'warnings' ? 'warning' : 'tidy';
    const acc = accepted.find((a) => a.kind === kind && a.file === f.file && (!a.check || a.check === f.check) && (!a.match || f.message.includes(a.match)));
    if (acc) f.accepted = acc.reason;
  }
  lintCache = { ran, findings, errors };
  return lintCache;
}

/** lint findings -> tidy.json (the "static analysis findings" gate) */
export async function tidyGeneric(cfg) {
  const lint = await lintGeneric(cfg);
  const entries = list(cfg.commands?.lint).map(cmdSpec).filter((l) => (l.gate || (l.parse === 'diagnostics' ? 'warnings' : 'lint')) === 'lint');
  let r;
  if (!entries.length) r = { ran: false, label: '静态检查（lint）', pass: true, reason: '没有配置静态检查器（commands.lint 里 gate 为 "lint" 的条目）', findings: [], summary: { findings: 0 } };
  else {
    const findings = lint.findings.filter((f) => f.gate === 'lint').map(({ gate, ...f }) => f);
    const live = findings.filter((f) => !f.accepted);
    const byCheck = {};
    for (const f of live) byCheck[f.check] = (byCheck[f.check] || 0) + 1;
    r = {
      ran: true, label: '静态检查（lint）', checks: entries.map((l) => l.name || String(l.run).split(/\s+/)[0]).join(', '),
      pass: !lint.errors.length && live.length <= cfg.thresholds.tidyMax, error: lint.errors.join('\n') || undefined,
      summary: { findings: live.length, accepted: findings.length - live.length, byCheck }, findings,
    };
  }
  writeJson(cfg.out('tidy.json'), r);
  return r;
}

// ---------------- static: functions + scope + warnings ----------------

export async function staticGeneric(cfg) {
  const prod = listSources(cfg);
  const lizard = lizardExe(cfg);
  const liz = lizard ? new Map([...runLizard(lizard, prod)].map(([k, v]) => [relToRoot(cfg, k), v])) : null;
  const accepted = loadAccepted(cfg);
  const th = cfg.thresholds;
  const files = [];
  const functions = [];
  for (const abs of prod) {
    const rel = relToRoot(cfg, abs);
    const src = fs.readFileSync(abs, 'utf8');
    const lang = langOf(abs);
    const fns = liz ? (liz.get(rel) || (lang ? [] : null)) : analyzeFile(abs, src);
    let status = fns ? 'ok' : 'unsupported';
    const acc = status !== 'ok' && accepted.find((a) => a.kind === 'unbuilt' && a.file === rel);
    files.push({ file: rel, lines: codeLines(src, lang || 'clike'), status: acc ? 'accepted' : status, reason: acc ? acc.reason : undefined });
    for (const f of fns || []) {
      const v = [];
      if (f.complexity > th.complexityMax) v.push(`圈复杂度 ${f.complexity} > ${th.complexityMax}`);
      if (f.lines > th.functionLinesMax) v.push(`长度 ${f.lines} 行 > ${th.functionLinesMax}`);
      if (f.nesting != null && f.nesting > th.nestingMax) v.push(`嵌套 ${f.nesting} 层 > ${th.nestingMax}`);
      if (f.params > th.paramsMax) v.push(`参数 ${f.params} 个 > ${th.paramsMax}`);
      const a = v.length && accepted.find((x) => x.kind === 'function' && x.file === rel && (!x.match || f.name.includes(x.match)));
      functions.push({ ...f, file: rel, violations: a ? [] : v, accepted: a ? a.reason : undefined });
    }
  }
  functions.sort((a, b) => b.complexity - a.complexity || b.lines - a.lines);
  const lint = await lintGeneric(cfg);
  const warnings = lint.findings.filter((f) => f.gate === 'warnings').map((f) => ({ file: f.file, line: f.line, col: f.col, message: f.message, flag: f.check, accepted: f.accepted }));
  const liveWarnings = warnings.filter((w) => !w.accepted);
  const totalLines = files.reduce((n, f) => n + f.lines, 0);
  const okLines = files.filter((f) => f.status === 'ok' || f.status === 'accepted').reduce((n, f) => n + f.lines, 0);
  const scope = totalLines ? okLines / totalLines : 1;
  const offenders = functions.filter((f) => f.violations.length);
  const max = (k) => Math.max(0, ...functions.map((f) => f[k] ?? 0));
  const result = {
    engine: lizard ? 'lizard' : 'builtin',
    thresholds: th,
    summary: {
      productionFiles: files.length, translationUnits: files.length, failedUnits: 0,
      functions: functions.length, cudaFunctions: 0,
      maxComplexity: max('complexity'), maxLines: max('lines'), maxNesting: max('nesting'), maxParams: max('params'),
      offenders: offenders.length, warnings: liveWarnings.length, codeLines: totalLines, scope,
    },
    gates: {
      scope: { pass: files.length > 0 && scope >= th.staticScopeMin },
      complexity: { pass: offenders.length === 0 },
      warnings: { pass: liveWarnings.length <= th.warningsMax },
    },
    files, failed: [], functions, warnings,
  };
  writeJson(cfg.out('static.json'), result);
  return result;
}

// ---------------- CRAP + coverage ----------------

export function crapGeneric(cfg) {
  const stat = readJson(cfg.out('static.json'), null);
  if (!stat) throw new Error('static.json missing: run `static` first');
  const cov = readJson(cfg.out('coverage.lines.json'), null);
  if (!cov?.source) throw new Error('没有覆盖率报告：在 commands.test 里配置 lcov 或 cobertura 路径，并让测试命令生成它');
  const th = cfg.thresholds;
  const functions = stat.functions.map((f) => {
    const hits = cov.files[f.file];
    let total = 0, hit = 0;
    if (hits) for (let l = f.line; l <= f.endLine; l++) if (l in hits) { total++; if (hits[l] > 0) hit++; }
    const coverage = total ? hit / total : hits ? 1 : 0;
    const crap = crapScore(f.complexity, coverage);
    return {
      name: f.name, file: f.file, line: f.line, endLine: f.endLine, calls: null, complexity: f.complexity,
      coverage: Math.round(coverage * 1000) / 1000, crap: Math.round(crap * 100) / 100,
      violations: [
        ...(crap > th.crapMax ? [`CRAP ${crap.toFixed(1)} > ${th.crapMax}`] : []),
        ...(f.complexity > th.complexityMax ? [`complexity ${f.complexity} > ${th.complexityMax}`] : []),
      ],
    };
  }).sort((a, b) => b.crap - a.crap);
  // files the tests never loaded are absent from the report: they count as 0% covered
  const files = stat.files.filter((f) => f.status === 'ok').map((f) => {
    const hits = cov.files[f.file];
    if (!hits) return { file: f.file, lines: f.lines, covered: 0, percent: f.lines ? 0 : 1 };
    const vals = Object.values(hits);
    const covered = vals.filter((h) => h > 0).length;
    return { file: f.file, lines: vals.length, covered, percent: vals.length ? covered / vals.length : 1 };
  });
  const lines = files.reduce((a, f) => a + f.lines, 0);
  const covered = files.reduce((a, f) => a + f.covered, 0);
  const lineCoverage = lines ? covered / lines : 1;
  const offenders = functions.filter((f) => f.violations.length);
  const result = {
    thresholds: th,
    coverageSource: cov.source,
    summary: {
      functions: functions.length, maxCrap: functions[0]?.crap ?? 0,
      maxComplexity: Math.max(0, ...functions.map((f) => f.complexity)), offenders: offenders.length,
      lineCoverage: Math.round(lineCoverage * 1000) / 1000, linesTotal: lines, linesCovered: covered,
    },
    pass: offenders.length === 0 && lineCoverage >= th.lineCoverageMin,
    functions,
    files: files.sort((a, b) => a.percent - b.percent),
  };
  writeJson(cfg.out('crap.json'), result);
  return result;
}

// ---------------- architecture ----------------

export async function archGeneric(cfg) {
  const a = cmdSpec(cfg.commands?.arch);
  let r;
  if (!a) r = { pass: true, skipped: '没有配置 commands.arch（架构依赖检查）', modules: [], edges: [], violations: [], unmapped: [] };
  else {
    if (a.sarif) fs.rmSync(reportPath(cfg, a.sarif), { force: true });
    const res = await shell(cfg, a, { quiet: true });
    const violations = [];
    if (a.sarif && fs.existsSync(reportPath(cfg, a.sarif))) {
      for (const f of parseSarif(readJson(reportPath(cfg, a.sarif), {}))) {
        violations.push({ message: f.message, refs: [{ file: resolveReported(cfg, f.uri, [cfg.abs(a.cwd || '.')]), line: f.line }] });
      }
    }
    if (res.code !== 0 && !violations.length) violations.push({ message: `${a.name || String(a.run).split(/\s+/)[0]} 失败（exit ${res.code}）：${tail(res, 8)}`, refs: [] });
    r = { pass: violations.length === 0, tool: a.name || a.run, modules: [], edges: [], violations, unmapped: [], output: tail(res, 40) };
  }
  writeJson(cfg.out('arch.json'), r);
  return r;
}

// ---------------- mutation ----------------

function hitsMap(cfg) {
  const cov = readJson(cfg.out('coverage.lines.json'), null);
  if (!cov?.source) return null;
  return new Map(Object.entries(cov.files).map(([f, m]) => [f, new Map(Object.entries(m).map(([l, h]) => [+l, h]))]));
}

export async function mutationGeneric(cfg, opts = {}) {
  const ext = cmdSpec(cfg.commands?.mutation);
  if (ext) {
    // external tool writing mutation-testing-report-schema JSON (Stryker, mull, Infection, mutmut, ...)
    const report = reportPath(cfg, ext.report || 'reports/mutation/mutation.json');
    if (ext.run) {
      fs.rmSync(report, { force: true });
      const r = await shell(cfg, ext);
      if (!fs.existsSync(report)) throw new Error(`变异测试工具没有生成报告 ${ext.report}（exit ${r.code}）`);
    }
    const isSrc = makeSourceMatcher(cfg);
    const cwd = cfg.abs(ext.cwd || '.');
    const results = parseMutationElements(readJson(report, {}))
      .map(({ reportedFile, ...m }) => ({ ...m, file: resolveReported(cfg, reportedFile, [cwd, path.dirname(report)]) }))
      .filter((m) => isSrc(m.file));
    return writeMutationReport(cfg, { scope: `外部工具：${ext.name || String(ext.run || ext.report).split(/\s+/)[0]}`, results, tool: ext.name || ext.run });
  }
  const test = cmdSpec(cfg.commands?.mutationTest || cfg.commands?.test);
  if (!test?.run) throw new Error('commands.test.run 未配置');
  const build = cmdSpec(cfg.commands?.build);
  const hits = hitsMap(cfg);
  return mutateWith(cfg, opts, {
    files: listSources(cfg),
    lang: (f) => langOf(f),   // null = unsupported: reported by the gate, never skipped silently
    hits,
    missingFileIsUncovered: true,   // the coverage report lists every file the tests loaded
    fnRanges: functionRanges(cfg, () => []),
    baseline: async () => {
      if (build && (await shell(cfg, build, { quiet: true })).code !== 0) return { ok: false, ms: 0 };
      const r = await shell(cfg, test, { quiet: true });
      return { ok: r.code === 0, ms: r.ms };
    },
    build: async () => (build ? (await shell(cfg, build, { quiet: true })).code : 0),
    test: async (timeoutSec) => {
      const r = await shell(cfg, test, { quiet: true, timeoutMs: timeoutSec * 1000 });
      return r.timedOut ? 'TIMEOUT' : r.code === 0 ? 'SURVIVED' : 'KILLED';
    },
    finish: async () => { if (build) await shell(cfg, build, { quiet: true }); },
  });
}
