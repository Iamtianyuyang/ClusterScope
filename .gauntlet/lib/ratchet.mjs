// Ratchet mode for existing ("legacy") code bases: "clean as you code".
//   gauntlet-baseline.json (committed, approved by a human) lists the quality DEBT that existed when the
//   gauntlet was introduced: over-threshold functions with their metrics, under-covered files,
//   warning / lint findings, clones, files the analysis could not see, architecture violations.
// With "ratchet": { "enabled": true } the gates then mean:
//   - anything NOT in the baseline (new function, new finding, new clone, ...) must meet the thresholds;
//   - a baseline debt is tolerated while it does not get worse (a touched legacy function may stay
//     long, but not longer);
//   - changed lines (git diff against the base) must reach lineCoverageMin, and mutation testing
//     runs on changed lines only (unless ratchet.mutation = "all").
// The baseline only ever turns one way: `baseline --tighten` lowers entries to the current values and
// drops fixed debts; creating or loosening it is a rule change that the evidence pack flags.
import fs from 'node:fs';
import path from 'node:path';
import { readJson, writeJson, run, gitChangedLines } from './util.mjs';

export const ratchetOn = (cfg) => !!cfg.ratchet?.enabled;
export const baselineFile = (cfg) => cfg.abs(cfg.ratchet?.baseline || 'gauntlet-baseline.json');

const FN_METRICS = ['complexity', 'lines', 'nesting', 'params', 'crap'];
const LIMIT = { complexity: 'complexityMax', lines: 'functionLinesMax', nesting: 'nestingMax', params: 'paramsMax', crap: 'crapMax' };
const LABEL = { complexity: '圈复杂度', lines: '长度', nesting: '嵌套', params: '参数', crap: 'CRAP' };
const fnKey = (f) => `${f.file}|${f.name}`;
const msgKey = (kind, f) => `${kind}|${f.file}|${f.check || f.flag || ''}|${String(f.message || '').replace(/\d+/g, '#').trim()}`;
const cloneKey = (c) => `${[c.a.file, c.b.file].sort().join('|')}|${c.tokens}`;
const archKey = (v) => String(v.message).trim();

function multiset(keys) {
  const m = {};
  for (const k of keys) m[k] = (m[k] || 0) + 1;
  return m;
}

/** Current debt, read from the gate outputs (static / crap / tidy / duplication / arch json). */
export function snapshot(cfg) {
  const o = (f) => readJson(cfg.out(f), null);
  const th = cfg.thresholds;
  const stat = o('static.json'), crap = o('crap.json'), tidy = o('tidy.json'), dup = o('duplication.json'), arch = o('arch.json'), cpp = o('cppcheck.json');
  const functions = {};
  const merge = (key, vals) => {
    const cur = functions[key] || {};
    for (const [k, v] of Object.entries(vals)) if (v != null) cur[k] = Math.max(cur[k] ?? -Infinity, v);
    functions[key] = cur;
  };
  for (const f of stat?.functions || []) merge(fnKey(f), { complexity: f.complexity, lines: f.lines, nesting: f.nesting, params: f.params });
  for (const f of crap?.functions || []) merge(fnKey(f), { crap: f.crap, complexity: f.complexity });
  // keep only functions that exceed some threshold: everything else must simply stay within the thresholds
  for (const [k, v] of Object.entries(functions)) if (!FN_METRICS.some((m) => v[m] != null && v[m] > th[LIMIT[m]])) delete functions[k];
  const coverage = {};
  for (const f of crap?.files || []) if (f.percent < th.lineCoverageMin) coverage[f.file] = Math.round(f.percent * 10000) / 10000;
  return {
    version: 1,
    createdAt: new Date().toISOString(),
    commit: readJson(cfg.out('gate.json'), {}).commit || null,
    functions,
    coverage,
    findings: multiset([
      ...(stat?.warnings || []).filter((w) => !w.accepted).map((w) => msgKey('warnings', w)),
      ...(tidy?.findings || []).filter((f) => !f.accepted).map((f) => msgKey('tidy', f)),
      ...(cpp?.findings || []).filter((f) => !f.accepted).map((f) => msgKey('cppcheck', f)),
    ]),
    clones: multiset((dup?.clones || []).filter((c) => !c.accepted).map(cloneKey)),
    duplicationRatio: dup?.summary?.ratio ?? 0,
    unanalyzed: [...(stat?.files || []).filter((f) => f.status !== 'ok' && f.status !== 'accepted').map((f) => f.file), ...(stat?.failed || []).map((f) => f.file)].filter((v, i, a) => a.indexOf(v) === i).sort(),
    arch: multiset((arch?.violations || []).map(archKey)),
  };
}

export function debtCount(b) {
  if (!b) return 0;
  const sum = (m) => Object.values(m || {}).reduce((n, v) => n + v, 0);
  return Object.keys(b.functions || {}).length + Object.keys(b.coverage || {}).length + sum(b.findings) + sum(b.clones) + (b.unanalyzed || []).length + sum(b.arch);
}

/** Tighten: every entry may only go down (or disappear); nothing is ever added. */
export function tighten(old, cur, th) {
  const out = { ...old, tightenedAt: new Date().toISOString(), commit: cur.commit || old.commit };
  out.functions = {};
  for (const [k, ov] of Object.entries(old.functions || {})) {
    const cv = cur.functions[k];
    if (!cv) continue;                         // fixed (now within thresholds), renamed or deleted
    const nv = {};
    for (const m of FN_METRICS) if (ov[m] != null && cv[m] != null) nv[m] = Math.min(ov[m], cv[m]);
    if (FN_METRICS.some((m) => nv[m] != null && nv[m] > th[LIMIT[m]])) out.functions[k] = nv;
  }
  out.coverage = {};
  for (const [f, p] of Object.entries(old.coverage || {})) if (f in cur.coverage) out.coverage[f] = Math.max(p, cur.coverage[f]);
  const minMap = (a, b) => Object.fromEntries(Object.entries(a || {}).map(([k, n]) => [k, Math.min(n, b?.[k] || 0)]).filter(([, n]) => n > 0));
  out.findings = minMap(old.findings, cur.findings);
  out.clones = minMap(old.clones, cur.clones);
  out.arch = minMap(old.arch, cur.arch);
  out.duplicationRatio = Math.min(old.duplicationRatio ?? 0, cur.duplicationRatio ?? 0);
  out.unanalyzed = (old.unanalyzed || []).filter((f) => cur.unanalyzed.includes(f));
  return out;
}

/** What got looser from `prev` to `next` (a human must approve these). */
export function loosened(prev, next) {
  const out = [];
  for (const [k, nv] of Object.entries(next.functions || {})) {
    const pv = prev.functions?.[k];
    if (!pv) out.push(`新增遗留函数 ${k}`);
    else for (const m of FN_METRICS) if (nv[m] != null && (pv[m] == null || nv[m] > pv[m])) out.push(`${k} 的${LABEL[m]}基线 ${pv[m] ?? '-'} → ${nv[m]}`);
  }
  for (const [f, p] of Object.entries(next.coverage || {})) if (!(f in (prev.coverage || {})) || p < prev.coverage[f]) out.push(`${f} 的覆盖率基线 → ${(p * 100).toFixed(1)}%`);
  for (const kind of ['findings', 'clones', 'arch']) for (const [k, n] of Object.entries(next[kind] || {})) if (n > (prev[kind]?.[k] || 0)) out.push(`新增遗留${kind === 'findings' ? '问题' : kind === 'clones' ? '重复代码' : '架构违规'} ${k.split('|').slice(0, 3).join(' ')}`);
  for (const f of next.unanalyzed || []) if (!(prev.unanalyzed || []).includes(f)) out.push(`新增未分析文件 ${f}`);
  if ((next.duplicationRatio ?? 0) > (prev.duplicationRatio ?? 0) + 1e-9) out.push(`重复率基线 ${prev.duplicationRatio} → ${next.duplicationRatio}`);
  return out;
}

/** Executed-line hits of the last test run, for either adapter. */
function lineHitsOf(cfg) {
  const cov = readJson(cfg.out('coverage.lines.json'), null);
  if (cov?.source) return new Map(Object.entries(cov.files).map(([f, m]) => [f, new Map(Object.entries(m).map(([l, h]) => [+l, h]))]));
  return null;
}

function consume(budget, key) {
  if ((budget[key] || 0) > 0) { budget[key]--; return true; }
  return false;
}

/**
 * Re-judge the quality gates against the baseline. Tolerated debt is marked `legacy` in the gate
 * outputs (so `next` and the evidence pack skip it); gates[*].pass is recomputed.
 */
export async function applyRatchet(cfg, gates, { lineHits } = {}) {
  const base = readJson(baselineFile(cfg), null);
  if (!base) {
    for (const k of ['scope', 'complexity', 'warnings', 'tidy', 'cppcheck', 'duplication', 'crap', 'coverage', 'arch']) if (gates[k]) gates[k] = { ...gates[k], pass: false, error: `ratchet 已开启但没有基线 ${path.basename(baselineFile(cfg))}：由人确认后运行 \`gauntlet baseline\`` };
    return null;
  }
  const th = cfg.thresholds;
  const o = (f) => readJson(cfg.out(f), null);
  const summary = { baseline: debtCount(base), legacy: 0, worse: 0 };

  // functions (static.json + crap.json): tolerated while no metric exceeds max(baseline, threshold)
  const judgeFn = (f, metrics) => {
    if (!f.violations?.length) return;
    const b = base.functions?.[fnKey(f)];
    if (!b) return;                                    // new function: must meet the thresholds
    const worse = metrics.filter((m) => f[m] != null && f[m] > Math.max(b[m] ?? -Infinity, th[LIMIT[m]]));
    if (worse.length) { f.violations = worse.map((m) => `${LABEL[m]} ${f[m]} 比基线 ${b[m] ?? '（无）'} 更差`); summary.worse++; return; }
    f.legacy = b;
    f.legacyViolations = f.violations;
    f.violations = [];
    summary.legacy++;
  };
  const stat = o('static.json');
  if (stat) {
    stat.functions.forEach((f) => judgeFn(f, ['complexity', 'lines', 'nesting', 'params']));
    const budget = { ...(base.findings || {}) };
    for (const w of stat.warnings) if (!w.accepted && consume(budget, msgKey('warnings', w))) { w.legacy = true; summary.legacy++; }
    const unanalyzed = new Set(base.unanalyzed || []);
    for (const f of stat.files) if (f.status !== 'ok' && f.status !== 'accepted' && unanalyzed.has(f.file)) { f.legacy = true; summary.legacy++; }
    const newFailed = stat.failed.filter((f) => !unanalyzed.has(f.file));
    const offenders = stat.functions.filter((f) => f.violations.length);
    const liveWarnings = stat.warnings.filter((w) => !w.accepted && !w.legacy);
    stat.summary.offenders = offenders.length;
    stat.summary.warnings = liveWarnings.length;
    stat.gates.scope.pass = !newFailed.length && stat.files.every((f) => f.status === 'ok' || f.status === 'accepted' || f.legacy);
    stat.gates.complexity.pass = offenders.length === 0;
    stat.gates.warnings.pass = liveWarnings.length <= th.warningsMax;
    stat.ratchet = true;
    writeJson(cfg.out('static.json'), stat);
    if (gates.scope) gates.scope = { ...gates.scope, pass: stat.gates.scope.pass };
    if (gates.complexity) gates.complexity = { ...gates.complexity, pass: stat.gates.complexity.pass, offenders: offenders.length };
    if (gates.warnings) gates.warnings = { ...gates.warnings, pass: stat.gates.warnings.pass, warnings: liveWarnings.length };
  }

  const tidy = o('tidy.json');
  if (tidy?.ran) {
    const budget = { ...(base.findings || {}) };
    for (const f of tidy.findings) if (!f.accepted && consume(budget, msgKey('tidy', f))) { f.legacy = true; summary.legacy++; }
    const live = tidy.findings.filter((f) => !f.accepted && !f.legacy);
    tidy.summary.findings = live.length;
    tidy.pass = !tidy.error && live.length <= th.tidyMax;
    writeJson(cfg.out('tidy.json'), tidy);
    if (gates.tidy) gates.tidy = { ...gates.tidy, pass: tidy.pass, findings: live.length };
  }

  const cpp = o('cppcheck.json');
  if (cpp?.ran) {
    const budget = { ...(base.findings || {}) };
    for (const f of cpp.findings) if (!f.accepted && consume(budget, msgKey('cppcheck', f))) { f.legacy = true; summary.legacy++; }
    const live = cpp.findings.filter((f) => !f.accepted && !f.legacy);
    cpp.summary.findings = live.length;
    cpp.pass = live.length <= th.cppcheckMax;
    writeJson(cfg.out('cppcheck.json'), cpp);
    if (gates.cppcheck) gates.cppcheck = { ...gates.cppcheck, pass: cpp.pass, findings: live.length };
  }

  const dup = o('duplication.json');
  if (dup) {
    const budget = { ...(base.clones || {}) };
    for (const c of dup.clones) if (!c.accepted && consume(budget, cloneKey(c))) { c.legacy = true; summary.legacy++; }
    const fresh = dup.clones.filter((c) => !c.accepted && !c.legacy);
    dup.pass = !fresh.length && dup.summary.ratio <= Math.max(th.duplicationMax, base.duplicationRatio ?? 0) + 1e-9;
    writeJson(cfg.out('duplication.json'), dup);
    if (gates.duplication) gates.duplication = { ...gates.duplication, pass: dup.pass, newClones: fresh.length };
  }

  const crap = o('crap.json');
  if (crap) {
    crap.functions.forEach((f) => judgeFn(f, ['crap', 'complexity']));
    crap.summary.offenders = crap.functions.filter((f) => f.violations.length).length;
    // coverage: no file below its baseline, and changed lines covered to lineCoverageMin
    const regressions = crap.files.filter((f) => f.percent + 1e-4 < Math.min(th.lineCoverageMin, base.coverage?.[f.file] ?? th.lineCoverageMin))
      .map((f) => ({ file: f.file, percent: f.percent, baseline: base.coverage?.[f.file] ?? null }));
    const changed = gitChangedLines(cfg, cfg.base);
    const hits = lineHits ? lineHits() : lineHitsOf(cfg);
    let changedLines = 0, coveredLines = 0;
    const uncovered = [];
    if (changed && hits) {
      const prod = new Set(crap.files.map((f) => f.file));
      for (const [file, lines] of changed.map) {
        if (!prod.has(file)) continue;
        const h = hits.get(file);
        const all = lines === 'all' ? (h ? [...h.keys()] : []) : [...lines];
        // a production file the tests never loaded: every changed code line counts as uncovered
        if (!h) {
          const n = lines === 'all' ? crap.files.find((f) => f.file === file)?.lines || 0 : lines.size;
          changedLines += n;
          if (n) uncovered.push({ file, lines: lines === 'all' ? ['整个新文件'] : ranges([...lines]), note: '这个文件没有被任何测试加载' });
          continue;
        }
        const miss = [];
        for (const l of all) { if (!h.has(l)) continue; changedLines++; if (h.get(l) > 0) coveredLines++; else miss.push(l); }
        if (miss.length) uncovered.push({ file, lines: ranges(miss) });
      }
    }
    const diffCoverage = changedLines ? coveredLines / changedLines : 1;
    // without a base there are no "changed lines": that must not read as "everything covered"
    const noBase = !changed ? '找不到分支基线：在 gauntlet.config.json 设 "base"，或在 GAUNTLET.md 写 base: <默认分支>' : null;
    const covPass = !noBase && !regressions.length && diffCoverage + 1e-9 >= th.lineCoverageMin;
    crap.ratchet = { coverage: { pass: covPass, ...(noBase ? { error: noBase } : {}), base: changed?.ref || null, baseName: changed?.base?.name || null, changedLines, coveredLines, diffCoverage: Math.round(diffCoverage * 1000) / 1000, regressions, uncovered } };
    writeJson(cfg.out('crap.json'), crap);
    if (gates.crap) gates.crap = { ...gates.crap, pass: crap.summary.offenders === 0, offenders: crap.summary.offenders };
    if (gates.coverage) gates.coverage = { ...gates.coverage, pass: covPass, ...(noBase ? { error: noBase } : {}), diffCoverage: crap.ratchet.coverage.diffCoverage, changedLines, regressions: regressions.length };
  }

  const arch = o('arch.json');
  if (arch && !arch.skipped) {
    const budget = { ...(base.arch || {}) };
    for (const v of arch.violations) if (consume(budget, archKey(v))) { v.legacy = true; summary.legacy++; }
    const live = arch.violations.filter((v) => !v.legacy);
    arch.pass = live.length === 0;
    writeJson(cfg.out('arch.json'), arch);
    if (gates.arch) gates.arch = { ...gates.arch, pass: arch.pass, violations: live.length };
  }
  writeJson(cfg.out('ratchet.json'), summary);
  return summary;
}

function ranges(lines) {
  const s = [...lines].sort((a, b) => a - b);
  const out = [];
  for (let i = 0; i < s.length; i++) {
    let j = i;
    while (j + 1 < s.length && s[j + 1] === s[j] + 1) j++;
    out.push(i === j ? `${s[i]}` : `${s[i]}-${s[j]}`);
    i = j;
  }
  return out;
}

/** The baseline as it was at the git base of this branch (null when it did not exist there). */
export function baselineAtBase(cfg, ref) {
  if (!ref) return undefined;
  const top = run('git', ['rev-parse', '--show-toplevel'], { cwd: cfg.root, quiet: true, allowFail: true }).stdout.trim();
  if (!top) return undefined;
  const rel = path.relative(top, baselineFile(cfg)).split(path.sep).join('/');
  const r = run('git', ['show', `${ref}:${rel}`], { cwd: cfg.root, quiet: true, allowFail: true });
  if (r.code !== 0) return null;
  try { return JSON.parse(r.stdout); } catch { return null; }
}

export function printRatchet(s) {
  if (!s) return;
  console.log(`\nRATCHET  基线遗留 ${s.baseline} 项；本次容忍（未变差）${s.legacy} 项，比基线变差 ${s.worse} 个函数`);
}

export function writeBaseline(cfg, b) {
  writeJson(baselineFile(cfg), b);
  return fs.existsSync(baselineFile(cfg));
}
