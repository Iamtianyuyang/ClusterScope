// Evidence package for HUMAN review. Agents produce code faster than humans can read it,
// so the human reviews *evidence*, not diffs:
//   <outDir>/evidence/index.html   one-page "sheet" report (lettered panels, themes, offline, single file)
//   <outDir>/evidence/comment.md   ready-to-post Multica comment (inline html dashboard + mermaid)
// Inputs from <outDir>: gate.json, tests.json, acceptance/*.json, static.json, tidy.json, duplication.json,
// crap.json, mutation.json, arch.json, evidence/demos/*.html, evidence/diagrams/*;
// plus COMMITTED stage hand-offs: qa/qa-report.json, qa/constraints.json, qa/equivalence.json, qa/media/*,
// docs/*.md (tutorial via --tutorial).
//
// Layout ideas: answer-me-with-html (blueprint sheet, lettered panels, conclusion first, limits bars,
// theme/mode toggles, embedded source data) and html-anything's data-report template (KPI block,
// fixed-size charts, sticky zebra tables, insights list, collapsible methodology, no invented data).
import fs from 'node:fs';
import path from 'node:path';
import { readJson, ensureDir, run, gitChangedLines, relToRoot, pathOf } from './util.mjs';
import { irOf } from './gen.mjs';
import { parseFeatureDir } from './gherkin.mjs';
import { constraintsGate } from './checks.mjs';
import { listDiagrams } from './diagram.mjs';
import { buildSketch, sketchMermaid } from './sketch.mjs';
import { ratchetOn, baselineFile, baselineAtBase, loosened, debtCount } from './ratchet.mjs';
import { themeCss, BASE_CSS, PAGE_JS } from './report/theme.mjs';
import { esc, pct, panel, status, callout, kv, limits, timeline, table, markdown, barChart, crapScatter, archGraph } from './report/components.mjs';

export { markdown };

const readJsonSafe = (dir, rel) => { try { return readJson(path.resolve(dir, rel), null); } catch { return null; } };

// Files that define the rules of the game. Agents must not change them without a human noticing.
const governanceFiles = (cfg) => ['gauntlet.config.json', cfg.paths.architecture, cfg.paths.mutationAccepted, cfg.paths.qualityAccepted, `${cfg.paths.qa}/constraints.json`].map((p) => relToRoot(cfg, cfg.abs(p)));

export function collect(cfg, opts = {}) {
  const o = (f) => cfg.out(f);
  const gate = readJson(o('gate.json'), { gates: {} });
  const accDir = o('acceptance');
  const accResults = new Map();
  if (fs.existsSync(accDir)) for (const f of fs.readdirSync(accDir).filter((x) => x.endsWith('.json'))) {
    const r = readJson(path.join(accDir, f)); accResults.set(r.id, r);
  }
  let features = [];
  try { features = irOf(cfg, parseFeatureDir(cfg.abs(cfg.features))).features; } catch { features = []; }
  const evDir = o('evidence');
  const media = [pathOf(cfg, 'qa', 'media'), path.join(evDir, 'media')]
    .filter((dir) => fs.existsSync(dir))
    .flatMap((dir) => fs.readdirSync(dir).sort().map((f) => path.join(dir, f)));
  const demos = fs.existsSync(path.join(evDir, 'demos')) ? fs.readdirSync(path.join(evDir, 'demos')).filter((f) => f.endsWith('.html')).sort() : [];
  const tutorialFile = opts.tutorial ? path.resolve(cfg.root, opts.tutorial) : path.join(evDir, 'tutorial.md');
  const changes = changesByModule(cfg);
  return {
    gate,
    tests: readJson(o('tests.json'), null),
    static: readJson(o('static.json'), null),
    tidy: readJson(o('tidy.json'), null),
    cppcheck: readJson(o('cppcheck.json'), null),
    sanitize: readJson(o('sanitize.json'), null),
    dup: readJson(o('duplication.json'), null),
    crap: readJson(o('crap.json'), null),
    mutation: readJson(o('mutation.json'), null),
    arch: readJson(o('arch.json'), null),
    qa: readJson(pathOf(cfg, 'qa', 'qa-report.json'), null),
    constraints: fs.existsSync(pathOf(cfg, 'qa', 'constraints.json')) ? constraintsGate(cfg) : null,
    equivalence: readJson(pathOf(cfg, 'qa', 'equivalence.json'), null),
    diagrams: listDiagrams(cfg),
    // no Archify diagram (offline, not installed, not drawn): fall back to a dependency sketch
    sketch: listDiagrams(cfg).length ? null : buildSketch(cfg),
    features,
    accResults,
    tutorial: fs.existsSync(tutorialFile) ? fs.readFileSync(tutorialFile, 'utf8') : null,
    tutorialName: path.basename(tutorialFile),
    tutorialBase: path.dirname(tutorialFile),
    media,
    demos,
    evDir,
    changes,
    ratchet: ratchetOn(cfg) ? ratchetModel(cfg, changes) : null,
    configDiff: changes?.governance.some((f) => f.file === 'gauntlet.config.json')
      ? run('git', ['diff', changes.base, '--', 'gauntlet.config.json'], { cwd: cfg.root, quiet: true, allowFail: true }).stdout : '',
  };
}

/** Ratchet: baseline size, and whether this branch created or loosened it (only tightening is free). */
function ratchetModel(cfg, changes) {
  const now = readJson(baselineFile(cfg), null);
  const rel = relToRoot(cfg, baselineFile(cfg));
  const before = changes ? baselineAtBase(cfg, changes.base) : undefined;
  const touched = !!changes?.files.some((f) => f.file === rel);
  return {
    file: rel,
    debt: debtCount(now),
    debtBefore: before ? debtCount(before) : null,
    created: touched && before === null,
    loosened: touched && before && now ? loosened(before, now) : [],
  };
}

function changesByModule(cfg) {
  const ch = gitChangedLines(cfg, cfg.base);
  if (!ch) return null;
  const stat = run('git', ['diff', '--numstat', ch.ref, '--', '.'], { cwd: cfg.root, quiet: true, allowFail: true }).stdout;
  const top = run('git', ['rev-parse', '--show-toplevel'], { cwd: cfg.root, quiet: true, allowFail: true }).stdout.trim();
  const rows = stat.split(/\r?\n/).filter(Boolean).map((l) => {
    const [a, d, f] = l.split('\t');
    return { file: relToRoot(cfg, path.join(top, f)), added: +a || 0, deleted: +d || 0 };
  });
  for (const [f, v] of ch.map) if (v === 'all' && !rows.some((r) => r.file === f)) {
    const abs = path.resolve(cfg.root, f);
    const n = fs.existsSync(abs) ? fs.readFileSync(abs, 'utf8').split('\n').length : 0;
    rows.push({ file: f, added: n, deleted: 0, untracked: true });
  }
  // the vendored kit (.gauntlet/) is tooling, not rules: leave it out so real rule changes stand out
  const files = rows.filter((r) => !r.file.startsWith('..') && !r.file.startsWith(cfg.outDir) && !r.file.startsWith('.gauntlet/'));
  if (!files.length) return null;
  const rules = governanceFiles(cfg);
  const governance = files.filter((f) => rules.includes(f.file));
  // gauntlet-baseline.json is judged separately (ratchetModel): created / loosened -> review, tightened -> fine
  return { base: ch.ref, files, governance };
}

// ---------- model: everything the page and the comment say, computed once ----------

function scopeModel(d) {
  if (!d.static) return null;
  const runtime = new Map((d.crap?.files || []).map((f) => [f.file, f]));
  const mutated = new Map();
  for (const r of d.mutation?.results || []) if (r.status !== 'COMPILE_ERROR') mutated.set(r.file, (mutated.get(r.file) || 0) + 1);
  const withFunctions = new Set(d.static.functions.map((f) => f.file));
  const rows = d.static.files.map((f) => ({ ...f, runtime: runtime.get(f.file) || null, mutants: mutated.get(f.file) || 0, hasFunctions: withFunctions.has(f.file) }));
  const total = rows.reduce((n, f) => n + f.lines, 0) || 1;
  const sum = (list, pred) => list.filter(pred).reduce((n, f) => n + f.lines, 0);
  // runtime / mutation shares only make sense for files that define functions (not declaration-only headers)
  const code = rows.filter((f) => f.hasFunctions);
  const codeTotal = code.reduce((n, f) => n + f.lines, 0) || 1;
  return {
    rows,
    total,
    staticShare: sum(rows, (f) => f.status === 'ok' || f.status === 'accepted') / total,
    runtimeShare: sum(code, (f) => f.runtime) / codeTotal,
    mutatedShare: sum(code, (f) => f.mutants > 0) / codeTotal,
    missing: rows.filter((f) => f.status !== 'ok' && f.status !== 'accepted'),
    accepted: rows.filter((f) => f.status === 'accepted'),
  };
}

function model(cfg, d, title) {
  const g = d.gate.gates || {};
  const cases = d.tests?.cases || [];
  const acc = cases.filter((c) => c.name.startsWith('acc.'));
  const unit = cases.filter((c) => !c.name.startsWith('acc.'));
  const scenarios = d.features.flatMap((f) => f.scenarios.map((s) => ({ ...s, feature: f.name, file: f.file, result: d.accResults.get(s.id) })));
  const scenOk = scenarios.filter((s) => s.result?.status === 'passed').length;
  const th = cfg.thresholds;
  const qaChecks = d.qa?.checks || [];
  const ruleFiles = d.changes?.governance || [];
  const scope = scopeModel(d);
  const st = d.static?.summary;

  const gates = [];
  const add = (key, title, ok, detail) => gates.push({ key, title, ok, detail });
  if (g.spec) add('spec', '验收规格', g.spec.pass, `${g.spec.scenarios} 个场景，未定义步骤 ${g.spec.undefinedSteps}`);
  if (d.tests) {
    add('acceptance', '验收测试', acc.length > 0 && acc.every((c) => c.status === 'passed'), `${acc.filter((c) => c.status === 'passed').length}/${acc.length} 通过`);
    add('unit', '单元测试', unit.every((c) => c.status === 'passed'), `${unit.filter((c) => c.status === 'passed').length}/${unit.length} 通过`);
  }
  add('scope', '测量范围', !!d.static && d.static.gates.scope.pass, d.static ? `静态分析覆盖 ${pct(st.scope)} 的产品代码，解析失败 ${st.failedUnits} 个` : '没有运行静态分析');
  if (d.static) {
    add('complexity', '函数质量', d.static.gates.complexity.pass, `${st.functions} 个函数，超标 ${st.offenders} 个`);
    add('warnings', '编译告警', d.static.gates.warnings.pass, `${st.warnings} 条`);
  }
  if (d.tidy) add('tidy', d.tidy.label || 'clang-tidy', d.tidy.pass, d.tidy.ran ? `${d.tidy.summary.findings} 条问题` : `未运行：${d.tidy.reason}`);
  if (d.cppcheck?.ran) add('cppcheck', 'cppcheck', d.cppcheck.pass, `${d.cppcheck.summary.findings} 条问题`);
  if (d.sanitize) add('sanitize', '运行时检查（sanitizer）', d.sanitize.pass, d.sanitize.ran ? `${d.sanitize.findings.length} 处错误${d.sanitize.notRun?.length ? `，部分未运行` : ''}` : '没有运行');
  if (d.dup) add('duplication', '重复代码', d.dup.pass, `${pct(d.dup.summary.ratio)}，${d.dup.summary.clones} 处`);
  if (d.crap) {
    const legacyCrap = d.crap.functions.filter((f) => f.legacy).length;
    add('crap', 'CRAP', d.crap.functions.every((f) => !f.violations.length), `最大 CRAP ${d.crap.summary.maxCrap}${legacyCrap ? `（${legacyCrap} 个遗留函数未变差）` : ''}`);
    const rc = d.crap.ratchet?.coverage;
    if (rc) add('coverage', '改动行覆盖率（棘轮）', rc.pass, `改动的 ${rc.changedLines} 行可执行代码覆盖 ${pct(rc.diffCoverage)}${rc.regressions.length ? `，${rc.regressions.length} 个文件低于基线` : ''}`);
    else add('coverage', '行覆盖率', d.crap.summary.lineCoverage >= th.lineCoverageMin, pct(d.crap.summary.lineCoverage));
  }
  if (d.mutation) add('mutation', '变异测试', d.mutation.pass, `得分 ${pct(d.mutation.summary.score)}，${d.mutation.summary.total} 个变异体，存活 ${d.mutation.summary.survived + d.mutation.summary.noCoverage}${d.mutation.summary.unsupported ? `，${d.mutation.summary.unsupported} 个文件没法变异` : ''}`);
  if (d.arch && !d.arch.skipped) add('arch', '架构边界', d.arch.pass, `${d.arch.edges.length} 条依赖，违规 ${d.arch.violations.length}`);
  if (d.qa) add('qa', 'QA 端到端', d.qa.verdict === 'pass' && qaChecks.every((c) => c.status === 'pass'), `${qaChecks.filter((c) => c.status === 'pass').length}/${qaChecks.length} 通过`);
  add('constraints', '需求约束', !!d.constraints?.pass, d.constraints ? `${d.constraints.met}/${d.constraints.total} 条已证实` : '没有 qa/constraints.json');
  if (d.equivalence || cfg.requireEquivalence) {
    const ents = d.equivalence?.entries || [];
    add('equivalence', '与原程序等价', ents.length > 0 && ents.every((e) => e.status === 'pass'), d.equivalence ? `${ents.filter((e) => e.status === 'pass').length}/${ents.length} 项对比通过` : '要求对比，但没有 qa/equivalence.json');
  }
  if (d.diagrams.length) add('diagram', '架构图', d.diagrams.every((x) => x.status === 'pass'), `${d.diagrams.length} 张，${d.diagrams.filter((x) => x.status === 'pass').length} 张通过校验`);
  else add('diagram', '架构图', !!d.sketch, d.sketch ? `没有 Archify 架构图，用自动依赖草图代替（${d.sketch.modules.length} 个模块）` : '没有架构图，也无法生成依赖草图');

  const failing = gates.filter((x) => !x.ok);
  // pass but with things only a human can sign off: rule changes, accepted exceptions, skipped tools
  const reviewReasons = [];
  if (ruleFiles.length) reviewReasons.push(`改动了规则文件 ${ruleFiles.map((f) => f.file).join('、')}`);
  if (d.mutation?.accepted?.length) reviewReasons.push(`${d.mutation.accepted.length} 个变异体被接受为等价`);
  const qAcc = (d.static?.functions || []).filter((f) => f.accepted).length + (d.static?.warnings || []).filter((w) => w.accepted).length + (d.tidy?.findings || []).filter((f) => f.accepted).length + (scope?.accepted.length || 0);
  if (qAcc) reviewReasons.push(`${qAcc} 条质量问题被列为例外（${cfg.paths.qualityAccepted}）`);
  if (d.ratchet?.created) reviewReasons.push(`新建了棘轮基线 ${d.ratchet.file}（${d.ratchet.debt} 项遗留欠账被容忍）`);
  if (d.ratchet?.loosened.length) reviewReasons.push(`棘轮基线被放松 ${d.ratchet.loosened.length} 处：${d.ratchet.loosened.slice(0, 3).join('；')}${d.ratchet.loosened.length > 3 ? ' ……' : ''}`);
  if (!d.diagrams.length && d.sketch) reviewReasons.push('没有 Archify 架构图：只有按 import / #include 推断的依赖草图，未经源码证据校验');
  if (d.sanitize?.notRun?.length) reviewReasons.push(`sanitizer 没有运行：${d.sanitize.notRun.join('；')}`);
  if (d.tidy && !d.tidy.ran) reviewReasons.push(`${d.tidy.label || 'clang-tidy'} 没有运行`);
  if (cfg.adapter === 'commands' && d.arch?.skipped) reviewReasons.push('没有配置架构依赖检查（commands.arch）');
  const verdict = failing.length ? 'fail' : reviewReasons.length ? 'review' : 'pass';

  const stOf = (...keys) => {
    const hit = gates.filter((x) => keys.includes(x.key));
    return !hit.length ? 'na' : hit.every((x) => x.ok) ? 'ok' : 'bad';
  };
  const stages = [
    { title: '摸底', role: 'Surveyor', state: fs.existsSync(pathOf(cfg, 'profile')) ? 'ok' : 'na', text: `${cfg.adapter === 'commands' ? 'commands' : 'cmake-clang'} 适配器${cfg.ratchet?.enabled ? '，棘轮模式' : ''}` },
    { title: '规格', role: 'Specifier', state: stOf('spec'), text: `${scenarios.length} 个 Gherkin 场景` },
    { title: '编码', role: 'Coder', state: stOf('acceptance', 'unit'), text: d.tests ? `${d.tests.total - d.tests.failed}/${d.tests.total} 个测试通过` : '未运行' },
    { title: '清理', role: 'Cleaner', state: stOf('scope', 'complexity', 'warnings', 'tidy', 'cppcheck', 'duplication', 'crap', 'arch'), text: st ? `最大圈复杂度 ${st.maxComplexity}，重复 ${d.dup ? pct(d.dup.summary.ratio) : '–'}` : '未运行' },
    { title: '加固', role: 'Hardener', state: stOf('coverage', 'sanitize', 'mutation'), text: d.mutation ? `变异得分 ${pct(d.mutation.summary.score)}` : '未运行' },
    { title: 'QA', role: 'QA', state: stOf('qa', 'constraints', 'equivalence'), text: d.qa ? `${qaChecks.filter((c) => c.status === 'pass').length}/${qaChecks.length} 项检查通过` : '未运行' },
    { title: '证据包', role: 'Reporter', state: stOf('diagram') === 'bad' ? 'bad' : verdict === 'fail' ? 'bad' : 'ok', text: '本页 + 架构图' },
  ];

  // Insights: facts only, derived from the data above (html-anything: never invent numbers).
  const insights = [];
  if (scope) insights.push(`📏 产品代码 ${scope.rows.length} 个文件、${scope.total} 行：静态分析 ${pct(scope.staticShare)}，有覆盖率数据 ${pct(scope.runtimeShare)}，被变异测试触及 ${pct(scope.mutatedShare)}。`);
  if (st) insights.push(`🔬 ${st.functions} 个函数${st.cudaFunctions ? `（其中 CUDA ${st.cudaFunctions} 个）` : ''}：最大圈复杂度 ${st.maxComplexity}，最长 ${st.maxLines} 行，最深嵌套 ${st.maxNesting} 层；编译告警 ${st.warnings} 条。`);
  if (d.ratchet) {
    const legacy = readJsonSafe(d.evDir, '../ratchet.json');
    insights.push(`🪜 棘轮模式：基线遗留欠账 ${d.ratchet.debt} 项${d.ratchet.debtBefore != null && d.ratchet.debtBefore !== d.ratchet.debt ? `（本分支开始时 ${d.ratchet.debtBefore} 项）` : ''}；新代码和改动行按阈值严格要求${legacy ? `，本次容忍未变差的遗留问题 ${legacy.legacy} 项` : ''}。`);
  }
  if (d.dup) insights.push(`📋 重复代码占 ${pct(d.dup.summary.ratio)}（${d.dup.summary.clones} 处，其中跨目录 ${d.dup.summary.crossDirectory} 处）。`);
  if (d.mutation) {
    const mm = d.mutation.summary;
    insights.push(`🧬 共 ${mm.total} 个变异体：杀死 ${mm.killed}，存活 ${mm.survived}，未覆盖 ${mm.noCoverage}，人工接受 ${mm.accepted || 0}。`);
  }
  if (d.equivalence?.entries?.length) {
    const worst = Math.max(...d.equivalence.entries.map((e) => e.worstRelL2 || 0));
    insights.push(`⚖️ 与原程序对比 ${d.equivalence.entries.length} 项，最大相对 L2 误差 ${worst.toExponential(2)}，逐位一致 ${d.equivalence.entries.filter((e) => e.bitIdentical).length} 项。`);
  }
  if (d.changes) {
    const a = d.changes.files.reduce((n, f) => n + f.added, 0), r = d.changes.files.reduce((n, f) => n + f.deleted, 0);
    insights.push(`📦 本分支改动 ${d.changes.files.length} 个文件，+${a} / −${r} 行${ruleFiles.length ? `，其中 ${ruleFiles.length} 个是规则文件` : ''}。`);
  }

  return { title, verdict, failing, reviewReasons, gates, stages, insights, scenarios, scenOk, acc, unit, th, qaChecks, ruleFiles, scope };
}

// fill a row when the next panel does not fit (answer-me-with-html's fillRows): no holes in the grid
function fillRows(spans, cols = 3) {
  let used = 0;
  return spans.map((span, i) => {
    if (used + span > cols) used = 0;
    used += span;
    const next = spans[i + 1];
    const fill = next === undefined || used + next > cols ? cols - used : 0;
    used = fill || used === cols ? 0 : used;
    return span + fill;
  });
}

const VERDICT = {
  pass: { cls: 'is-ok', head: '✓ 全部闸门通过', kv: '全部闸门通过', tone: 'ok', emoji: '✅' },
  review: { cls: 'is-warn', head: '! 闸门通过，有事项待你确认', kv: '通过，待你确认', tone: 'warn', emoji: '⚠️' },
  fail: { cls: 'is-err', head: '✗ 有闸门未通过', kv: '未通过', tone: 'err', emoji: '❌' },
};

// ---------- page ----------

export function buildEvidence(cfg, { title, tutorial } = {}) {
  const d = collect(cfg, { tutorial });
  ensureDir(d.evDir);
  const featureTitle = title || d.gate.title || d.features.map((f) => f.name).join(' / ') || 'Gauntlet';
  const m = model(cfg, d, featureTitle);
  const V = VERDICT[m.verdict];
  const when = (d.gate.finishedAt || new Date().toISOString()).replace('T', ' ').replace(/\.\d+Z$/, ' UTC');

  // Inline media as data URIs so the single html file survives being attached / uploaded.
  let budget = 8 * 1024 * 1024;
  const mime = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp', '.svg': 'image/svg+xml', '.mp4': 'video/mp4', '.webm': 'video/webm' };
  const resolveMedia = (rel, baseDir = d.evDir) => {
    const abs = path.resolve(baseDir, rel);
    if (!fs.existsSync(abs)) return rel;
    const size = fs.statSync(abs).size;
    const ext = path.extname(abs).toLowerCase();
    if (!mime[ext] || size > budget) return rel;
    budget -= size;
    return `data:${mime[ext]};base64,${fs.readFileSync(abs).toString('base64')}`;
  };

  // Which panels exist decides their letters; the review route refers to panels by letter.
  const specs = [
    { key: 'verdict', title: '结论', span: 2 },
    { key: 'route', title: '审阅路线', span: 1 },
    { key: 'scope', title: '测量范围', span: 1, when: !!m.scope },
    { key: 'pipeline', title: '流水线', span: 2 },
    { key: 'gates', title: '闸门', span: 1 },
    { key: 'diagram', title: '架构图', span: 2 },
    { key: 'quality', title: '代码质量', span: 2, when: !!d.static },
    { key: 'dup', title: '重复代码', span: 1, when: !!d.dup },
    { key: 'constraints', title: '需求约束', span: 1 },
    { key: 'equivalence', title: '与原程序等价性', span: 2, when: !!d.equivalence || !!cfg.requireEquivalence },
    { key: 'scenarios', title: '验收场景', span: 2, when: m.scenarios.length > 0 },
    { key: 'qa', title: 'QA 端到端', span: 1, when: !!d.qa },
    { key: 'demo', title: '演示', span: 2, when: d.demos.length + d.media.length > 0 },
    { key: 'tutorial', title: '使用教程', span: 1, when: !!d.tutorial },
    { key: 'risk', title: 'CRAP 风险图', span: 2, when: !!d.crap?.functions.length },
    { key: 'functions', title: '函数风险（CRAP）', span: 1, when: !!d.crap?.functions.length },
    { key: 'mutation', title: '变异测试', span: 2, when: !!d.mutation },
    { key: 'arch', title: '架构边界', span: 1, when: !!d.arch && !d.arch.skipped },
    { key: 'changes', title: '改动范围', span: 3, when: !!d.changes },
  ].filter((p) => p.when !== false);
  const spans = fillRows(specs.map((p) => p.span));
  specs.forEach((p, i) => { p.id = i < 26 ? String.fromCharCode(65 + i) : `A${String.fromCharCode(65 + i - 26)}`; p.span = spans[i]; });
  const L = Object.fromEntries(specs.map((p) => [p.key, p.id]));
  const ref = (key, label) => (L[key] ? `${L[key]} 面板「${label}」` : `「${label}」`);

  const route = [];
  if (m.ruleFiles.length) route.push(`先看 ${ref('changes', '改动范围')}：本次改了规则文件 ${m.ruleFiles.map((f) => f.file).join('、')}。这些文件决定闸门量什么、量多严，必须由你确认。`);
  if (m.scope) route.push(`看 ${ref('scope', '测量范围')}：确认所有产品代码都被量到了。${m.scope.missing.length ? `现在有 ${m.scope.missing.length} 个文件没被分析。` : ''}`);
  route.push(d.diagrams.length ? `看 ${ref('diagram', '架构图')}：代码现在长什么样。每个方块和箭头都引用了真实源码。` : `看 ${ref('diagram', '架构图')}：自动生成的模块依赖草图（按 import / #include 推断，没有逐条源码证据）。`);
  if (L.quality) route.push(`看 ${ref('quality', '代码质量')} 和 ${ref('dup', '重复代码')}：最复杂、最长的函数，以及复制粘贴的代码。`);
  route.push(`读 ${ref('scenarios', '验收场景')} 和 ${ref('constraints', '需求约束')}：确认需求和你提的每一条要求都被证实了。`);
  if (L.equivalence) route.push(`看 ${ref('equivalence', '与原程序等价性')}：新程序的输出和原程序逐帧对比的误差。`);
  if (L.demo) route.push(`看 ${ref('demo', '演示')}：真实执行的录像。`);
  if (d.mutation?.accepted?.length) route.push(`核对 ${ref('mutation', '变异测试')} 里 ${d.mutation.accepted.length} 个“人工接受”的变异体。`);
  route.push(m.verdict === 'fail' ? '有闸门未通过：不要合并。Leader 会按失败的闸门安排返工。'
    : m.verdict === 'review' ? `闸门都通过了，但有事项需要你确认（${m.reviewReasons.join('；')}）。确认无误回复「通过」，否则写出面板字母和意见。`
      : '全部闸门为绿：在 issue 里回复「通过」。有问题就写出面板字母和具体意见，Leader 会安排返工。');

  const body = {};
  // verdict
  body.verdict = () => {
    const sc = m.scope;
    const st = d.static?.summary;
    const cells = [
      { k: '结论', v: m.failing.length ? `${m.failing.length} 个闸门未通过` : V.kv, tone: m.verdict === 'pass' ? 'ok' : m.verdict === 'fail' ? 'err' : undefined, wide: true },
      { k: '静态分析覆盖', v: sc ? pct(sc.staticShare) : '–', small: `下限 ${pct(m.th.staticScopeMin)}`, tone: sc && sc.staticShare >= m.th.staticScopeMin ? 'ok' : 'err' },
      { k: '验收场景', v: `${m.scenOk}/${m.scenarios.length}`, tone: m.scenOk === m.scenarios.length && m.scenarios.length ? 'ok' : 'err' },
      { k: '需求约束', v: d.constraints ? `${d.constraints.met}/${d.constraints.total}` : '–', tone: d.constraints?.pass ? 'ok' : 'err' },
      { k: '最大圈复杂度', v: st ? st.maxComplexity : '–', small: `上限 ${m.th.complexityMax}` },
      { k: '重复代码', v: d.dup ? pct(d.dup.summary.ratio) : '–', small: `上限 ${pct(m.th.duplicationMax)}` },
      { k: '变异得分', v: d.mutation ? pct(d.mutation.summary.score) : '–', small: d.mutation ? `${d.mutation.summary.total} 个变异体` : '' },
    ];
    return `${kv(cells, 3)}
${m.failing.length ? callout('err', '未通过的闸门', `<ul class="g-insights">${m.failing.map((x) => `<li>${status('no', x.title)} <span class="g-muted">${esc(x.detail)}</span></li>`).join('')}</ul>`) : ''}
${m.verdict === 'review' ? callout('warn', '需要你确认', `<ul class="g-insights">${m.reviewReasons.map((x) => `<li>${esc(x)}</li>`).join('')}</ul>`) : ''}
<ul class="g-insights">${m.insights.map((x) => `<li>${esc(x)}</li>`).join('')}</ul>`;
  };
  // review route
  body.route = () => `${m.ruleFiles.length ? callout('warn', '规则文件被改动', `<p class="g-muted">${m.ruleFiles.map((f) => `<code class="g-code-inline">${esc(f.file)}</code>`).join(' ')}</p>`) : ''}
<div class="g-md"><ol>${route.map((x) => `<li>${esc(x)}</li>`).join('')}</ol></div>
<p class="g-muted">你不需要读代码。下面每个数字都由机器生成，可用 <code class="g-code-inline">node .gauntlet/gauntlet.mjs gate --profile full</code> 复现。</p>`;
  // measurement scope: what was actually measured, file by file
  body.scope = () => {
    const sc = m.scope;
    const stateOf = (f) => (f.status === 'ok' ? status('ok', '已分析') : f.status === 'accepted' ? status('warn', '例外') : f.status === 'failed' ? status('no', '解析失败') : f.status === 'not-built' ? status('no', '未参与构建') : f.status === 'unsupported' ? status('no', '分析器不支持') : status('no', '未被包含'));
    return `${limits([
      { label: '静态分析（全部函数的复杂度/告警）', value: sc.staticShare, limit: m.th.staticScopeMin, kind: 'min', scale: 1, fmt: (x) => pct(x) },
      { label: '有覆盖率数据（覆盖率 / CRAP）', value: sc.runtimeShare, info: true, scale: 1, fmt: (x) => pct(x), note: `按含函数定义的文件计。${d.static?.summary.cudaFunctions ? 'nvcc 编译的 CUDA 设备代码无法插桩，' : '拿不到覆盖率的代码'}由静态分析 + 变异测试补位` },
      { label: '被变异测试触及', value: sc.mutatedShare, info: true, scale: 1, fmt: (x) => pct(x) },
    ])}
${sc.missing.length ? callout('err', `${sc.missing.length} 个产品代码文件没有被量到`, `<p class="g-muted">${sc.missing.slice(0, 12).map((f) => `<code class="g-code-inline">${esc(f.file)}</code>`).join(' ')}${sc.missing.length > 12 ? ' …' : ''}</p>`) : callout('ok', '全部产品代码都被静态分析', `<p class="g-muted">${sc.rows.length} 个文件、${sc.total} 行代码。</p>`)}
${d.static.failed.length ? d.static.failed.slice(0, 5).map((f) => callout('err', `解析失败：${f.file}`, `<pre class="g-err">${esc(f.errors.join('\n'))}</pre>`)).join('') : ''}
<details class="g-method"><summary>逐文件明细（${sc.rows.length} 个文件）</summary>${table([
    { h: '文件', cell: (f) => `<span class="g-mono">${esc(f.file)}</span>${f.reason ? `<div class="g-muted">例外理由：${esc(f.reason)}</div>` : ''}` },
    { h: '行', cell: (f) => esc(f.lines), cls: 'g-nw' },
    { h: '静态', cell: stateOf, cls: 'g-nw' },
    { h: '插桩', cell: (f) => (f.runtime ? status('ok', pct(f.runtime.percent, 0)) : status('na')), cls: 'g-nw' },
    { h: '变异体', cell: (f) => esc(f.mutants), cls: 'g-nw' },
  ], sc.rows, { maxHeight: 420 })}</details>`;
  };
  // pipeline
  body.pipeline = () => timeline(m.stages);
  // gates as limits
  body.gates = () => {
    const items = [];
    const st = d.static?.summary;
    if (m.scenarios.length) items.push({ label: '验收场景通过', value: m.scenOk, limit: m.scenarios.length, kind: 'min', scale: m.scenarios.length, fmt: (x) => `${x}/${m.scenarios.length}` });
    if (st) {
      items.push({ label: '最大圈复杂度（全部函数）', value: st.maxComplexity, limit: m.th.complexityMax, kind: 'max' });
      items.push({ label: '最长函数（行）', value: st.maxLines, limit: m.th.functionLinesMax, kind: 'max' });
      items.push({ label: '最深嵌套', value: st.maxNesting, limit: m.th.nestingMax, kind: 'max' });
      items.push({ label: '编译告警', value: st.warnings, limit: m.th.warningsMax, kind: 'max', scale: Math.max(3, st.warnings) });
    }
    if (d.tidy?.ran) items.push({ label: `${d.tidy.label || 'clang-tidy'} 问题`, value: d.tidy.summary.findings, limit: m.th.tidyMax, kind: 'max', scale: Math.max(3, d.tidy.summary.findings) });
    if (d.dup) items.push({ label: '重复代码', value: d.dup.summary.ratio, limit: m.th.duplicationMax, kind: 'max', scale: Math.max(0.1, d.dup.summary.ratio * 1.2), fmt: (x) => pct(x) });
    if (d.crap) {
      items.push({ label: '最大 CRAP', value: d.crap.summary.maxCrap, limit: m.th.crapMax, kind: 'max', note: 'CRAP = CC² × (1 − 覆盖率)³ + CC' });
      items.push({ label: '行覆盖率', value: d.crap.summary.lineCoverage, limit: m.th.lineCoverageMin, kind: 'min', scale: 1, fmt: (x) => pct(x) });
    }
    if (d.mutation) items.push({ label: '变异得分', value: d.mutation.summary.score, limit: d.mutation.threshold, kind: 'min', scale: 1, fmt: (x) => pct(x) });
    if (d.arch && !d.arch.skipped) items.push({ label: '架构违规', value: d.arch.violations.length, limit: 0, kind: 'max', scale: Math.max(3, d.arch.violations.length) });
    return `${limits(items)}
<details class="g-method"><summary>这些指标怎么算</summary><div class="g-md" style="margin-top:8px"><ul>
${cfg.adapter === 'commands' ? `<li><b>测量范围</b>：sources 匹配到的全部产品代码（测试、生成代码、构建目录除外）都必须被分析到；分析器不认识的文件让闸门失败。</li>
<li><b>圈复杂度 CC</b>：1 + 分支条件数（if / for / while / case / catch / && / || / ?:；Python 另含 elif / except / and / or）。来自 ${esc(d.static?.engine === 'lizard' ? 'lizard' : '内置多语言分析器')}，覆盖每个函数，不依赖测试是否执行到。</li>
<li><b>告警 / 静态检查</b>：commands.lint 里配置的检查器（${esc(d.tidy?.checks || '未配置')}）输出的 SARIF。覆盖率来自测试命令输出的 ${esc(d.crap?.coverageSource || 'LCOV / Cobertura')} 报告。</li>` : `<li><b>测量范围</b>：仓库里所有 C/C++/CUDA 文件（测试、生成代码、构建目录除外）都必须被 clang 解析；没参与构建、解析失败的文件让闸门失败。</li>
<li><b>圈复杂度 CC</b>：1 + 分支条件数（if / for / while / case / && / || / ?:）。来自 clang AST，覆盖每个函数，包括 CUDA kernel。</li>
<li><b>编译告警 / clang-tidy</b>：-Wall -Wextra，以及 bugprone、performance、clang 静态分析器的检查。</li>`}
<li><b>重复代码</b>：连续 ${esc(d.dup?.minTokens ?? cfg.duplication.minTokens)} 个以上完全相同的词法单元（忽略注释、字符串）。</li>
<li><b>CRAP</b>：CC² × (1 − 覆盖率)³ + CC，对有覆盖率数据的函数计算。人类标准 ≤ 4，agent 放宽到 ≤ ${esc(m.th.crapMax)}。</li>
<li><b>变异得分</b>：每次只改一个符号再构建、跑全部测试，测试失败即“杀死”。得分 = 杀死 /（杀死 + 存活 + 未覆盖）。</li></ul></div></details>`;
  };
  // architecture diagram(s) from Archify
  body.diagram = () => (d.diagrams.length ? d.diagrams.map((x) => `<div class="g-demo"><div class="g-demo-title">${x.status === 'pass' ? status('ok') : status('no')}${esc(x.candidate)} <span class="g-muted">Archify ${esc(x.type)} · 源码证据已校验${x.browserCheck ? ` · 浏览器检查：${esc(x.browserCheck)}` : ''}</span></div>${x.status === 'pass' && x.htmlAbs ? `<iframe class="g-diagram" title="${esc(x.candidate)}" loading="lazy" srcdoc="${esc(fs.readFileSync(x.htmlAbs, 'utf8'))}"></iframe>` : `<pre class="g-err">${esc(`失败阶段：${x.stage}\n${x.output || ''}`)}</pre>`}</div>`).join('')
    : d.sketch ? `${callout('warn', '没有 Archify 架构图：下面是自动生成的依赖草图', `<p class="g-muted">${d.sketch.source === 'architecture' ? '模块来自架构规则，依赖来自 #include。' : '模块是产品代码目录，依赖按 import / #include 推断。'}没有逐条源码证据，只用来快速看清结构。要源码可追溯的架构图，请让 Reporter 用 <code class="g-code-inline">diagram</code> 生成（离线环境把 Archify 放到本机，在 <code class="g-code-inline">gauntlet.local.json</code> 里设 <code class="g-code-inline">tools.archify.dir</code>）。</p>`)}${archGraph(d.sketch)}`
    : callout('err', '没有架构图', '<p class="g-muted">没有 Archify 架构图，也找不到可以推断依赖的产品代码。</p>'));
  // static code quality
  body.quality = () => {
    const s = d.static;
    const fns = s.functions;
    const cols = [
      { h: '', cell: (f) => (f.violations.length ? status('no') : f.accepted ? status('warn') : f.legacy ? status('warn', '遗留') : status('ok')), cls: 'g-nw' },
      { h: '函数', cell: (f) => `${esc(f.name)}${f.cuda && f.cuda !== 'host' ? ` <span class="g-status--warn">CUDA ${esc(f.cuda)}</span>` : ''}<div class="g-muted g-mono">${esc(f.file)}:${f.line}</div>${f.violations.length ? `<div class="g-status--no">${esc(f.violations.join('；'))}</div>` : ''}${f.accepted ? `<div class="g-muted">例外：${esc(f.accepted)}</div>` : ''}${f.legacy ? `<div class="g-muted">基线遗留，未变差：${esc((f.legacyViolations || []).join('；'))}</div>` : ''}` },
      { h: 'CC', cell: (f) => esc(f.complexity), cls: 'g-nw' },
      { h: '行', cell: (f) => esc(f.lines), cls: 'g-nw' },
      { h: '嵌套', cell: (f) => esc(f.nesting), cls: 'g-nw' },
      { h: '参数', cell: (f) => esc(f.params), cls: 'g-nw' },
    ];
    const worst = [...fns].sort((a, b) => b.violations.length - a.violations.length || b.complexity - a.complexity || b.lines - a.lines).slice(0, 40);
    const warnRows = s.warnings.filter((w) => !w.accepted && !w.legacy);
    const tidyRows = (d.tidy?.findings || []).filter((f) => !f.accepted && !f.legacy);
    const cppRows = (d.cppcheck?.findings || []).filter((f) => !f.accepted && !f.legacy);
    const sanRows = d.sanitize?.findings || [];
    const loc = { h: '位置', cell: (f) => `<span class="g-mono">${esc(f.file)}:${f.line}</span>`, cls: 'g-nw' };
    const extra = `${d.cppcheck?.ran ? (cppRows.length ? `${callout('err', `${cppRows.length} 条 cppcheck 问题`, '')}${table([loc, { h: '问题', cell: (f) => `${esc(f.message)} <span class="g-muted">${esc(f.severity)}/${esc(f.check)}</span>` }], cppRows, { maxHeight: 300 })}` : callout('ok', 'cppcheck 没有发现问题', `<p class="g-muted g-mono">--enable=${esc(d.cppcheck.enable)}</p>`)) : ''}
${d.sanitize ? (sanRows.length ? `${callout('err', `sanitizer 发现 ${sanRows.length} 处运行时错误`, '<p class="g-muted">内存越界、释放后使用、未定义行为、GPU 访存或竞争——测试通过也可能藏着它们。</p>')}${table([loc, { h: '错误', cell: (f) => `${esc(f.kind)} <span class="g-muted">${esc(f.tool)}${f.test ? ` · ${esc(f.test)}` : ''}</span>` }], sanRows, { maxHeight: 300 })}` : d.sanitize.ran ? callout('ok', 'sanitizer 没有发现运行时错误', `<p class="g-muted">${esc([d.sanitize.host.ran ? 'AddressSanitizer + UndefinedBehaviorSanitizer' : '', d.sanitize.cuda.ran ? `compute-sanitizer ${d.sanitize.cuda.tools.join(' / ')}` : ''].filter(Boolean).join('；'))}</p>`) : '') : ''}
${d.sanitize?.notRun?.length ? callout('warn', 'sanitizer 没有运行', `<p class="g-muted">${esc(d.sanitize.notRun.join('；'))}</p>`) : ''}`;
    return `${barChart([...fns].sort((a, b) => b.complexity - a.complexity).slice(0, 12).map((f) => ({ name: f.name, value: f.complexity })), { threshold: m.th.complexityMax })}
<p class="g-muted">圈复杂度最高的 12 个函数（全部 ${fns.length} 个函数都参与评判，不依赖测试是否执行到）。</p>
${table(cols, worst, { maxHeight: 520 })}
${warnRows.length ? `${callout('err', `${warnRows.length} 条编译告警`, '')}${table([{ h: '位置', cell: (w) => `<span class="g-mono">${esc(w.file)}:${w.line}</span>`, cls: 'g-nw' }, { h: '告警', cell: (w) => `${esc(w.message)} <span class="g-muted">${esc(w.flag)}</span>` }], warnRows, { maxHeight: 300 })}` : callout('ok', '没有编译告警', `<p class="g-muted">${esc(cfg.adapter === 'commands' ? 'commands.lint 中 gate = "warnings" 的检查器' : cfg.static.warnings.join(' '))}</p>`)}
${d.tidy ? (d.tidy.ran ? (tidyRows.length ? `${callout('err', `${tidyRows.length} 条 ${d.tidy.label || 'clang-tidy'} 问题`, '')}${table([{ h: '位置', cell: (f) => `<span class="g-mono">${esc(f.file)}:${f.line}</span>`, cls: 'g-nw' }, { h: '问题', cell: (f) => `${esc(f.message)} <span class="g-muted">${esc(f.check)}</span>` }], tidyRows, { maxHeight: 300 })}` : callout('ok', `${d.tidy.label || 'clang-tidy'} 没有发现问题`, `<p class="g-muted g-mono">${esc(d.tidy.checks)}</p>`)) : callout('warn', `${d.tidy.label || 'clang-tidy'} 没有运行`, `<p class="g-muted">${esc(d.tidy.reason)}</p>`)) : ''}
${extra}`;
  };
  // duplication
  body.dup = () => {
    const s = d.dup.summary;
    return `${limits([{ label: '重复代码占比', value: s.ratio, limit: d.dup.threshold, kind: 'max', scale: Math.max(0.1, s.ratio * 1.2), fmt: (x) => pct(x), note: `${s.duplicatedLines}/${s.codeLines} 行，${s.clones} 处，跨目录 ${s.crossDirectory} 处` }])}
${d.dup.clones.length ? table([
      { h: '行', cell: (c) => esc(c.lines), cls: 'g-nw' },
      { h: '这里', cell: (c) => `<span class="g-mono">${esc(c.a.file)}:${c.a.start}-${c.a.end}</span>` },
      { h: '和这里一样', cell: (c) => `<span class="g-mono">${esc(c.b.file)}:${c.b.start}-${c.b.end}</span>${c.accepted ? `<div class="g-muted">例外：${esc(c.accepted)}</div>` : ''}` },
    ], d.dup.clones, { maxHeight: 520 }) : callout('ok', '没有复制粘贴的代码', '')}`;
  };
  // constraints
  body.constraints = () => (d.constraints ? `${table([
    { h: '', cell: (c) => status(c.state === 'met' ? 'ok' : c.state === 'violated' ? 'no' : 'warn', c.state === 'met' ? '已证实' : c.state === 'violated' ? '未满足' : '未验证'), cls: 'g-nw' },
    { h: '约束', cell: (c) => `<b>${esc(c.id)}</b> ${esc(c.text)}${c.verify ? `<div class="g-muted">验证方式：${esc(c.verify)}</div>` : ''}` },
    { h: '证据', cell: (c) => esc(c.proofs.join('、') || '–') },
  ], d.constraints.items)}
<p class="g-muted">来自你的需求原文，由规格阶段逐条列出。agent 不能自行宣布某条“超出范围”；未验证的约束会让闸门失败。</p>` : callout('err', '没有列出需求约束', '<p class="g-muted">规格阶段必须把需求里的每条明确要求（服务器、目录、工具、性能……）写进 qa/constraints.json。</p>'));
  // equivalence with the original program
  body.equivalence = () => {
    const ents = d.equivalence?.entries || [];
    if (!ents.length) return callout('err', '要求与原程序对比，但没有对比记录', '<p class="g-muted">QA 阶段用 <code class="g-code-inline">gauntlet compare &lt;原程序输出&gt; &lt;新程序输出&gt; --record qa/equivalence.json</code> 记录。</p>');
    const finite = ents.filter((e) => Number.isFinite(e.worstRelL2) && e.worstRelL2 > 0);
    return `${finite.length ? barChart(finite.slice(0, 16).map((e) => ({ name: e.name, value: Number(e.worstRelL2.toExponential(2)) })), { threshold: Math.max(...finite.map((e) => e.tol || 0)) || null }) : ''}
${table([
      { h: '', cell: (e) => status(e.status === 'pass' ? 'ok' : 'no'), cls: 'g-nw' },
      { h: '对比项', cell: (e) => `${esc(e.name)}<div class="g-muted g-mono">${esc(e.expected)}<br>${esc(e.actual)}</div>` },
      { h: '帧', cell: (e) => esc(e.frames ?? '–'), cls: 'g-nw' },
      { h: '最大相对 L2', cell: (e) => (e.error ? `<span class="g-status--no">${esc(e.error)}</span>` : esc(e.worstRelL2.toExponential(3))), cls: 'g-nw' },
      { h: '最大绝对误差', cell: (e) => (e.error ? '–' : esc(e.maxAbs.toExponential(3))), cls: 'g-nw' },
      { h: '逐位一致', cell: (e) => (e.bitIdentical ? status('ok') : status('na')), cls: 'g-nw' },
      { h: '容差', cell: (e) => esc(e.rule === 'bit-identical' ? '逐位' : e.tol), cls: 'g-nw' },
    ], ents, { maxHeight: 520 })}`;
  };
  // scenarios
  body.scenarios = () => {
    const byFeature = d.features.map((f) => {
      const rows = f.scenarios.map((s) => {
        const r = d.accResults.get(s.id);
        const state = r?.status === 'passed' ? 'ok' : r ? 'bad' : 'na';
        const steps = s.steps.map((stp, i) => {
          const rs = r?.steps?.[i];
          const cls = rs?.status === 'passed' || !rs ? '' : rs.status === 'skipped' ? 'is-skip' : 'is-bad';
          return `<li class="${cls}"><span class="g-kw">${esc(stp.keyword)}</span>${esc(stp.text)}${stp.docString != null ? `<pre class="g-code">${esc(stp.docString)}</pre>` : ''}${stp.table ? table(stp.table[0].map((h, ci) => ({ h, cell: (row) => esc(row[ci]) })), stp.table.slice(1)) : ''}${rs?.message ? `<pre class="g-err">${esc(rs.message)}</pre>` : ''}</li>`;
        }).join('');
        return `<details class="g-sc is-${state}"${state === 'bad' ? ' open' : ''}><summary>${status(state === 'ok' ? 'ok' : state === 'bad' ? 'no' : 'na')}<span class="g-sc-name">${esc(s.name)}</span><span class="g-sc-id">${esc(s.id)}${r ? ` · ${r.ms.toFixed(1)} ms` : ' · 未运行'}</span></summary><ol class="g-steps">${steps}</ol></details>`;
      }).join('');
      return `<div class="g-feature"><div class="g-feature-title">📘 ${esc(f.name)} <span class="g-muted g-mono">${esc(f.file)}</span></div>${f.description.length ? `<p class="g-muted">${esc(f.description.join(' '))}</p>` : ''}${rows}</div>`;
    }).join('');
    return `<div class="g-scroll">${byFeature}</div>`;
  };
  // QA
  body.qa = () => `${callout(d.qa.verdict === 'pass' ? 'ok' : 'err', d.qa.verdict === 'pass' ? 'QA 通过' : 'QA 未通过', d.qa.summary ? `<p class="g-muted">${esc(d.qa.summary)}</p>` : '')}
${table([
    { h: '', cell: (c) => status(c.status === 'pass' ? 'ok' : 'no') },
    { h: '检查项', cell: (c) => `${esc(c.title)}${c.constraint ? `<div class="g-muted">证实约束 ${esc([].concat(c.constraint).join('、'))}</div>` : ''}` },
    { h: '操作', cell: (c) => `<code class="g-code-inline">${esc(c.action)}</code>` },
    { h: '实际结果', cell: (c) => `${esc(c.actual)}${c.status === 'pass' ? '' : `<div class="g-muted">期望：${esc(c.expected)}</div>`}` },
  ], m.qaChecks, { maxHeight: 520 })}`;
  // demos + media
  body.demo = () => [
    ...d.demos.map((f) => `<div class="g-demo"><div class="g-demo-title">▶ ${esc(f.replace(/\.html$/, ''))}</div><iframe title="${esc(f)}" loading="lazy" srcdoc="${esc(fs.readFileSync(path.join(d.evDir, 'demos', f), 'utf8'))}"></iframe></div>`),
    ...d.media.map((abs) => {
      const f = path.basename(abs);
      if (/\.(mp4|webm)$/i.test(f)) return `<video class="g-video" controls src="${esc(resolveMedia(abs))}"></video>`;
      if (/\.(png|jpe?g|gif|webp|svg)$/i.test(f)) return `<figure class="g-fig"><img src="${esc(resolveMedia(abs))}" alt="${esc(f)}"><figcaption>${esc(f)}</figcaption></figure>`;
      return '';
    }),
  ].join('\n');
  // tutorial
  body.tutorial = () => `<div class="g-scroll">${markdown(d.tutorial, (src) => resolveMedia(src, d.tutorialBase))}</div>`;
  // CRAP risk (instrumented functions only)
  body.risk = () => `${crapScatter(d.crap.functions, m.th)}
<p class="g-muted">只包含有覆盖率数据的函数。每个点是一个函数，越靠左上越危险：复杂且没被测试。</p>
${barChart(d.crap.functions.slice(0, 10).map((f) => ({ name: f.name, value: f.crap })), { threshold: m.th.crapMax })}`;
  body.functions = () => table([
    { h: '', cell: (f) => status(f.violations.length ? 'no' : 'ok') },
    { h: '函数', cell: (f) => `${esc(f.name)}<div class="g-muted g-mono">${esc(f.file)}:${f.line}</div>` },
    { h: 'CC', cell: (f) => esc(f.complexity), cls: 'g-nw' },
    { h: '覆盖', cell: (f) => pct(f.coverage, 0), cls: 'g-nw' },
    { h: 'CRAP', cell: (f) => `<b>${esc(f.crap)}</b>`, cls: 'g-nw' },
  ], d.crap.functions, { maxHeight: 560 });
  // mutation
  body.mutation = () => {
    const mu = d.mutation, s = mu.summary;
    const lim = limits([{ label: '变异得分', value: s.score, limit: mu.threshold, kind: 'min', scale: 1, fmt: (x) => pct(x), note: `范围：${mu.scope}。共 ${s.total} 个：杀死 ${s.killed} · 存活 ${s.survived} · 未覆盖 ${s.noCoverage} · 编译失败（忽略）${s.compileErrors} · 人工接受 ${s.accepted || 0}` }]);
    const cols = [
      { h: '位置', cell: (r) => `<span class="g-mono">${esc(r.file)}:${r.line}</span>`, cls: 'g-nw' },
      { h: '变异', cell: (r) => esc(r.op), cls: 'g-nw' },
      { h: '源码', cell: (r) => `<code class="g-code-inline">${esc(r.source.trim())}</code>` },
    ];
    return `${lim}
${mu.survivors.length ? `${callout('err', `${mu.survivors.length} 个变异体存活`, '<p class="g-muted">改了这些代码行，测试仍然全部通过。说明这里的行为没有被断言锁住。</p>')}${table([{ h: '状态', cell: (r) => status(r.status === 'NO_COVERAGE' ? 'warn' : 'no', r.status === 'NO_COVERAGE' ? '未覆盖' : '存活'), cls: 'g-nw' }, ...cols], mu.survivors, { maxHeight: 520 })}` : callout('ok', '没有存活的变异体', '<p class="g-muted">每个被变异的代码行都至少让一个测试失败。</p>')}
${mu.unsupported?.length ? callout('err', `${mu.unsupported.length} 个文件没有被变异：内置引擎不支持它们的语言`, `<p class="g-muted g-mono">${mu.unsupported.slice(0, 20).map(esc).join('<br>')}</p><p class="g-muted">需要配置外部变异工具（commands.mutation），否则这些代码的测试有没有守住行为没人知道。</p>`) : ''}
${mu.accepted?.length ? `${callout('warn', `${mu.accepted.length} 个变异体被人工接受为“等价”——请核对理由`, '<p class="g-muted">agent 认为这些改动不改变可观察行为，所以不写测试。理由不成立就驳回。</p>')}${table([...cols, { h: '理由', cell: (r) => esc(r.reason) }], mu.accepted)}` : ''}`;
  };
  // module boundaries
  body.arch = () => (d.arch.tool ? `${d.arch.violations.length ? d.arch.violations.map((v) => callout('err', v.message, `<p class="g-muted g-mono">${v.refs.slice(0, 5).map((r) => `${esc(r.file)}:${r.line}`).join('<br>')}</p>`)).join('') : callout('ok', '没有越界依赖', `<p class="g-muted g-mono">${esc(d.arch.tool)}</p>`)}
<pre class="g-code">${esc(d.arch.output || '')}</pre>` : archBody());
  const archBody = () => `${archGraph(d.arch)}
${(d.arch.warnings || []).map((w) => callout('warn', w, '')).join('')}
${d.arch.violations.length ? d.arch.violations.map((v) => callout('err', v.message, `<p class="g-muted g-mono">${v.refs.slice(0, 5).map((r) => `${esc(r.file)}:${r.line}${r.include ? ` #include ${esc(r.include)}` : ''}`).join('<br>')}</p>`)).join('') : callout('ok', '没有越界依赖', '<p class="g-muted">依赖方向全部符合 architecture.json，模块之间没有环，项目内 include 全部解析。</p>')}
${table([{ h: '模块', cell: (x) => esc(x.name) }, { h: '文件', cell: (x) => esc(x.files) }, { h: '允许依赖', cell: (x) => esc(x.mayDependOn.join('、') || '（无）') }], d.arch.modules)}`;
  // changes (+ the config diff when the rules themselves changed)
  body.changes = () => `${d.configDiff ? `${callout('warn', 'gauntlet.config.json 的改动（决定量什么、量多严）', '')}<pre class="g-code">${esc(d.configDiff)}</pre>` : ''}
${table([
    { h: '', cell: (f) => (m.ruleFiles.includes(f) ? status('warn', '规则') : ''), cls: 'g-nw' },
    { h: '文件', cell: (f) => `<span class="g-mono">${esc(f.file)}</span>${f.untracked ? ' <span class="g-muted">（新文件）</span>' : ''}` },
    { h: '新增', cell: (f) => `<span class="g-status--ok">+${f.added}</span>`, cls: 'g-nw' },
    { h: '删除', cell: (f) => `<span class="g-status--no">−${f.deleted}</span>`, cls: 'g-nw' },
  ], d.changes.files, { maxHeight: 420 })}`;

  const ok = (key) => m.gates.find((x) => x.key === key)?.ok;
  const metaOf = {
    verdict: d.gate.profile ? `profile ${d.gate.profile}` : '',
    scope: m.scope ? `${m.scope.rows.length} 个文件` : '',
    pipeline: 'Specifier → Reporter',
    gates: `${m.gates.filter((x) => x.ok).length}/${m.gates.length} 通过`,
    diagram: d.diagrams.length ? `${d.diagrams.length} 张 · Archify` : d.sketch ? '依赖草图' : '',
    quality: d.static ? `${d.static.summary.functions} 个函数` : '',
    dup: d.dup ? pct(d.dup.summary.ratio) : '',
    constraints: d.constraints ? `${d.constraints.met}/${d.constraints.total}` : '',
    equivalence: d.equivalence ? `${d.equivalence.entries.length} 项` : '',
    scenarios: `${m.scenOk}/${m.scenarios.length} 通过`,
    qa: d.qa ? `${m.qaChecks.filter((c) => c.status === 'pass').length}/${m.qaChecks.length}` : '',
    demo: `${d.demos.length} 段录像 · ${d.media.length} 个文件`,
    tutorial: d.tutorialName,
    risk: `${d.crap?.functions.length || 0} 个函数`,
    functions: '按 CRAP 降序',
    mutation: d.mutation ? pct(d.mutation.summary.score) : '',
    arch: d.arch && !d.arch.skipped ? `${d.arch.modules.length} 个模块` : '',
    changes: d.changes ? `相对 ${d.changes.base.slice(0, 10)}` : '',
  };
  const toneOf = {
    verdict: m.verdict === 'fail' ? 'err' : m.verdict === 'review' ? 'warn' : '',
    route: m.ruleFiles.length ? 'warn' : '',
    scope: ok('scope') ? '' : 'err',
    diagram: ok('diagram') ? '' : 'err',
    quality: ok('complexity') && ok('warnings') && ok('tidy') !== false ? '' : 'err',
    dup: d.dup && !d.dup.pass ? 'err' : '',
    constraints: ok('constraints') ? '' : 'err',
    equivalence: ok('equivalence') === false ? 'err' : '',
    scenarios: m.scenOk < m.scenarios.length ? 'err' : '',
    qa: ok('qa') === false ? 'err' : '',
    mutation: d.mutation && !d.mutation.pass ? 'err' : d.mutation?.accepted?.length ? 'warn' : '',
    arch: d.arch && !d.arch.pass ? 'err' : '',
    changes: m.ruleFiles.length ? 'warn' : '',
  };
  const tools = { scenarios: ' <button class="g-btn" type="button" data-g="expand" style="margin-left:auto">展开 / 收起</button>' };
  const panels = specs.map((p) => panel({ id: p.id, title: p.title, span: p.span, meta: metaOf[p.key], tone: toneOf[p.key], tools: tools[p.key] || '' }, body[p.key]())).join('\n');

  const data = {
    title: featureTitle, verdict: m.verdict, reviewReasons: m.reviewReasons, commit: d.gate.commit, profile: d.gate.profile, finishedAt: d.gate.finishedAt,
    gates: m.gates, stages: m.stages, insights: m.insights,
    scope: m.scope && { staticShare: m.scope.staticShare, runtimeShare: m.scope.runtimeShare, mutatedShare: m.scope.mutatedShare, missing: m.scope.missing.map((f) => f.file) },
    staticSummary: d.static?.summary, worstFunctions: d.static?.functions.slice(0, 20),
    duplication: d.dup && { summary: d.dup.summary, clones: d.dup.clones.slice(0, 20) },
    scenarios: m.scenarios.map((s) => ({ id: s.id, name: s.name, status: s.result?.status || 'notrun' })),
    constraints: d.constraints?.items, equivalence: d.equivalence?.entries, qa: d.qa,
    mutation: d.mutation && { summary: d.mutation.summary, survivors: d.mutation.survivors, accepted: d.mutation.accepted },
    archViolations: d.arch?.violations, ruleFileChanges: m.ruleFiles.map((f) => f.file), diagrams: d.diagrams.map(({ htmlAbs, ...x }) => x),
  };
  const nums = Array.from({ length: 8 }, (_, i) => i + 1);
  const ruler = (side, labels) => `<div class="g-ruler g-ruler--${side}" aria-hidden="true">${labels.map((l) => `<span>${l}</span>`).join('')}</div>`;

  const html = `<!doctype html>
<html lang="zh-CN" data-theme="blueprint" data-mode="auto">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>证据包 · ${esc(featureTitle)}</title>
<style>${themeCss()}${BASE_CSS}</style></head>
<body>
<div class="g-toolbar"><button class="g-btn" type="button" data-g="theme" data-labels='{"blueprint":"主题：图纸","card":"主题：卡片"}'>主题</button><button class="g-btn" type="button" data-g="mode" data-labels='{"auto":"明暗：跟随系统","light":"明暗：浅色","dark":"明暗：深色"}'>明暗</button><button class="g-btn" type="button" data-g="copy">复制数据</button></div>
<main class="g-sheet">
<header class="g-head">
<div class="g-eyebrow">GAUNTLET 证据包 · ${esc(when)}</div>
<h1>${esc(featureTitle)}</h1>
<div class="g-verdict ${V.cls}">${m.failing.length ? `✗ ${m.failing.length} 个闸门未通过` : V.head}</div>
<div class="g-head-meta">${d.gate.commit ? `<span><b>commit</b>${esc(d.gate.commit)}</span>` : ''}${d.gate.profile ? `<span><b>profile</b>${esc(d.gate.profile)}</span>` : ''}${m.scope ? `<span><b>测量范围</b>${pct(m.scope.staticShare)}</span>` : ''}<span><b>场景</b>${m.scenOk}/${m.scenarios.length}</span>${d.changes ? `<span><b>base</b>${esc(d.changes.base.slice(0, 10))}</span>` : ''}</div>
<p class="g-intro">${esc(m.verdict === 'pass' ? `全部产品代码都被量到，所有质量闸门为绿。按 ${L.route} 面板的顺序审阅。` : m.verdict === 'review' ? `闸门都通过了，但有 ${m.reviewReasons.length} 件事需要你确认。按 ${L.route} 面板的顺序审阅。` : '有闸门未通过，这一版不能合并。红色面板说明了原因。')}</p>
</header>
<div class="g-frame">${ruler('top', nums)}${ruler('bottom', nums)}${ruler('left', ['A', 'B', 'C', 'D'])}${ruler('right', ['A', 'B', 'C', 'D'])}
<div class="g-grid">
${panels}
</div></div>
<p class="g-foot">由 .gauntlet/gauntlet.mjs evidence 生成 · 单文件、离线可用 · 面板字母可直接在 issue 评论里引用</p>
</main>
<script type="application/json" id="gauntlet-data">${JSON.stringify(data, null, 2).replace(/</g, '\\u003c')}</script>
<script>${PAGE_JS}</script>
</body></html>`;
  fs.writeFileSync(path.join(d.evDir, 'index.html'), html);

  const comment = buildComment(d, m, L, V);
  fs.writeFileSync(path.join(d.evDir, 'comment.md'), comment);
  return { verdict: m.verdict, allOk: m.verdict !== 'fail', rows: m.gates, index: path.join(d.evDir, 'index.html'), comment: path.join(d.evDir, 'comment.md') };
}

// ---------- Multica comment: a compact limits-style dashboard using Multica's theme variables ----------

function buildComment(d, m, L, V) {
  const bad = 'color-mix(in srgb, #dc2626 85%, var(--foreground))';
  const okC = 'var(--chart-2)';
  const row = (label, value, limit, kind, fmt = (x) => String(x), scale) => {
    const sc = scale ?? Math.max(value, limit, 1) * (kind === 'max' ? 1.25 : 1);
    const isBad = kind === 'max' ? value > limit : value < limit;
    const w = Math.min(100, (value / sc) * 100), mk = Math.min(100, (limit / sc) * 100);
    return `<div style="margin:8px 0"><div style="display:flex;justify-content:space-between;font-size:12px"><span>${isBad ? '✗' : '✓'} ${esc(label)}</span><span style="font-family:ui-monospace,monospace;color:${isBad ? bad : okC}">${esc(fmt(value))} · ${kind === 'max' ? '上限' : '下限'} ${esc(fmt(limit))}</span></div><div style="position:relative;height:10px;border:1px solid var(--border);border-radius:3px;background:var(--muted);margin-top:3px"><div style="position:absolute;left:0;top:0;bottom:0;width:${w.toFixed(1)}%;background:${isBad ? bad : okC};opacity:.35;border-right:2px solid ${isBad ? bad : okC}"></div><div style="position:absolute;top:-4px;bottom:-4px;left:${mk.toFixed(1)}%;border-left:2px dashed var(--muted-foreground)"></div></div></div>`;
  };
  const rows = [];
  const th = m.th;
  if (m.scope) rows.push(row('测量范围（静态分析）', m.scope.staticShare, th.staticScopeMin, 'min', (x) => pct(x), 1));
  if (m.scenarios.length) rows.push(row('验收场景', m.scenOk, m.scenarios.length, 'min', (x) => `${x}/${m.scenarios.length}`, m.scenarios.length));
  if (d.static) {
    rows.push(row('最大圈复杂度（全部函数）', d.static.summary.maxComplexity, th.complexityMax, 'max'));
    rows.push(row('编译告警', d.static.summary.warnings, th.warningsMax, 'max', String, Math.max(3, d.static.summary.warnings)));
  }
  if (d.dup) rows.push(row('重复代码', d.dup.summary.ratio, th.duplicationMax, 'max', (x) => pct(x), Math.max(0.1, d.dup.summary.ratio * 1.2)));
  if (d.mutation) rows.push(row('变异得分', d.mutation.summary.score, d.mutation.threshold, 'min', (x) => pct(x), 1));
  if (d.constraints) rows.push(row('需求约束', d.constraints.met, d.constraints.total, 'min', (x) => `${x}/${d.constraints.total}`, Math.max(1, d.constraints.total)));
  if (d.arch && !d.arch.skipped) rows.push(row('架构违规', d.arch.violations.length, 0, 'max', String, Math.max(3, d.arch.violations.length)));
  const stages = m.stages.map((s) => `<div style="flex:1;min-width:70px;text-align:center"><div style="width:22px;height:22px;margin:0 auto 4px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;border:1.5px solid ${s.state === 'bad' ? bad : s.state === 'ok' ? okC : 'var(--border)'};background:${s.state === 'ok' ? okC : s.state === 'bad' ? bad : 'transparent'};color:var(--background)">${s.state === 'ok' ? '✓' : s.state === 'bad' ? '✗' : ''}</div><div style="font-size:12px;font-weight:600">${esc(s.title)}</div><div style="font-size:11px;color:var(--muted-foreground)">${esc(s.text)}</div></div>`).join('');
  const html = `<style>body{background:var(--background);color:var(--foreground);font-family:var(--font-sans);margin:0}</style><div style="padding:6px 4px"><div style="display:flex;gap:4px;flex-wrap:wrap;padding-bottom:10px;border-bottom:1px solid var(--border)">${stages}</div>${rows.join('')}</div>`;

  const lines = [];
  lines.push(`## ${V.emoji} Gauntlet 证据包：${m.title}`, '');
  if (m.failing.length) lines.push(`> ❌ **未通过：** ${m.failing.map((x) => `${x.title}（${x.detail}）`).join('；')}`, '');
  if (m.reviewReasons.length) lines.push(`> ⚠️ **需要你确认：** ${m.reviewReasons.join('；')}`, '');
  lines.push('```html title="闸门仪表盘"', html, '```', '');
  lines.push(...m.insights.map((x) => `- ${x}`), '');
  if (m.scope?.missing.length) {
    lines.push(`### 📏 没被量到的产品代码 (${m.scope.missing.length})`);
    for (const f of m.scope.missing.slice(0, 15)) lines.push(`- \`${f.file}\` — ${f.status === 'not-built' ? '未参与构建' : f.status === 'failed' ? '解析失败' : f.status === 'unsupported' ? '分析器不支持这种语言' : '未被任何产品代码包含'}`);
    lines.push('');
  }
  if (d.constraints) {
    lines.push(`### 📌 需求约束 ${d.constraints.met}/${d.constraints.total}`);
    for (const c of d.constraints.items) lines.push(`- ${c.state === 'met' ? '✅' : c.state === 'violated' ? '❌' : '⚠️ 未验证'} **${c.id}** ${c.text}`);
    lines.push('');
  }
  if (d.equivalence?.entries?.length) {
    lines.push(`### ⚖️ 与原程序对比 ${d.equivalence.entries.filter((e) => e.status === 'pass').length}/${d.equivalence.entries.length}`);
    for (const e of d.equivalence.entries.slice(0, 20)) lines.push(`- ${e.status === 'pass' ? '✅' : '❌'} ${e.name} — ${e.error || `最大相对 L2 ${e.worstRelL2.toExponential(2)}${e.bitIdentical ? '（逐位一致）' : ''}`}`);
    lines.push('');
  }
  lines.push(`### 📘 验收场景 ${m.scenOk}/${m.scenarios.length}${L.scenarios ? `（附件 ${L.scenarios} 面板有每一步的结果）` : ''}`);
  for (const s of m.scenarios) lines.push(`- ${s.result?.status === 'passed' ? '✅' : s.result ? '❌' : '⚪'} ${s.name}`);
  if (d.dup?.clones?.length) {
    lines.push('', `### 📋 重复代码 ${pct(d.dup.summary.ratio)}`);
    for (const c of d.dup.clones.slice(0, 8)) lines.push(`- ${c.lines} 行：\`${c.a.file}:${c.a.start}-${c.a.end}\` = \`${c.b.file}:${c.b.start}-${c.b.end}\``);
  }
  if (d.mutation?.survivors?.length) {
    lines.push('', `### 🧬 存活变异体 (${d.mutation.survivors.length})`);
    for (const s of d.mutation.survivors.slice(0, 10)) lines.push(`- \`${s.file}:${s.line}\` ${s.op} — \`${s.source.trim()}\``);
  }
  if (d.mutation?.accepted?.length) {
    lines.push('', `### ⚠️ 需要你确认的等价变异体 (${d.mutation.accepted.length})`);
    for (const s of d.mutation.accepted) lines.push(`- \`${s.file}:${s.line}\` ${s.op} — 理由：${s.reason}`);
  }
  if (d.arch && !d.arch.skipped && d.arch.mermaid) lines.push('', '### 🏛️ 模块依赖', '', '```mermaid', d.arch.mermaid, '```');
  else if (d.sketch?.edges.length) lines.push('', '### 🏛️ 模块依赖（自动草图）', '', '```mermaid', sketchMermaid(d.sketch), '```');
  lines.push('', `> 完整报告见附件 \`index.html\`（含 ${L.diagram || ''} 面板架构图、逐文件测量范围、代码质量明细、演示录像）。架构图单独附件见 \`diagrams/*.html\`。评论时写面板字母即可定位。`);
  return lines.join('\n') + '\n';
}
