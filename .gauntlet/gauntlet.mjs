#!/usr/bin/env node
// Gauntlet: Uncle-Bob-style quality gates for agent-written code, any language and toolchain.
// Usage: node .gauntlet/gauntlet.mjs <command> [options]   (run from anywhere inside the project)
import fs from 'node:fs';
import path from 'node:path';
import { loadConfig, findRoot, writeJson, run, which, KIT_DIR, ensureDir, readJson } from './lib/util.mjs';
import { generate } from './lib/gen.mjs';
import { configure, build, testWithCoverage } from './lib/build.mjs';
import { computeCrap, printCrap, lineHits } from './lib/crap.mjs';
import { ratchetOn, applyRatchet, printRatchet, snapshot, tighten, debtCount, baselineFile, writeBaseline } from './lib/ratchet.mjs';
import { runMutation, recoverMutations } from './lib/mutate.mjs';
import { checkArchitecture, printArch } from './lib/arch.mjs';
import { runStatic, printStatic } from './lib/static.mjs';
import { runDuplication, printDuplication } from './lib/dup.mjs';
import { runTidy, printTidy } from './lib/tidy.mjs';
import { runCompare, printCompare, equivalenceGate } from './lib/compare.mjs';
import { qaGate, constraintsGate } from './lib/checks.mjs';
import { runDiagram } from './lib/diagram.mjs';
import { buildEvidence } from './lib/evidence.mjs';
import { recordDemo } from './lib/demo.mjs';
import { decide } from './lib/next.mjs';
import { survey, printSurvey } from './lib/survey.mjs';
import { runCppcheck, printCppcheck } from './lib/cppcheck.mjs';
import { runSanitize, runSanitizeCommand, printSanitize } from './lib/sanitize.mjs';
import { GherkinError } from './lib/gherkin.mjs';
import * as generic from './lib/adapter-commands.mjs';
import { leakMark, leakCheck, printLeak, splitList } from './lib/leak.mjs';

const HELP = `gauntlet — 质量闸门（任何语言、任何工具链：项目自己的命令 + 标准报告）

  init [--workdir tmp] [--adapter cmake-clang]
                            生成 gauntlet.config.json、features/ 等骨架（在 commands 里填项目自己的构建 / 测试 / 检查命令）
                            --workdir：构建目录、输出目录、生成文件全部放进该目录（如 tmp）
                            --adapter cmake-clang：可选的深度分析，只适用于已经用 CMake 构建、clang 能编译的项目
  doctor                    检查工具链
  survey                    第 0 阶段摸底：语言、构建、测试框架、CI 命令、质量工具、入口、项目档案是否过期，并起草适配器配置
  spec [--allow-undefined]  解析 features/*.feature，列出未实现的步骤
  gen                       （cmake-clang）生成验收测试代码
  build [--reconfigure]     构建
  test                      构建 + 跑全部测试 + 收集覆盖率（+ 核对每个场景都有通过的验收测试）
  static                    分析全部产品代码：圈复杂度/长度/嵌套/参数、告警、分析范围
  tidy                      静态检查
  cppcheck                  （cmake-clang）第二个静态分析器 cppcheck，装了就运行
  sanitize                  运行时检查：AddressSanitizer + UndefinedBehaviorSanitizer，CUDA 测试用 compute-sanitizer
                            （commands 适配器：运行项目的 commands.sanitize 并读取 sanitizer 报告）
  dup                       重复代码检测（复制粘贴）
  crap                      函数的 CRAP / 覆盖率（需先 test）
  arch                      检查模块依赖方向和环
  mutate [--changed] [--base <ref>] [--files a,b]
                            变异测试（默认全部产品代码、不限数量）
  compare <expected> <actual> --text [--tol x] [--atol y]
                            文本结果（日志、CSV、JSON、表格）逐行比较：文字一致（忽略空白），数字按容差比较
  compare <expected> <actual> [--dtype f32] [--header N] [--frame N] [--tol x | --exact]
          [--name label] [--record qa/equivalence.json]
                            二进制结果逐帧比较（与原程序的黄金样本对比）
  diagram <candidate.json> [--type architecture] [--name slug]
                            用 Archify 校验并渲染架构图（节点与连线必须引用真实源码）
  next --profile <p> [--no-run] [--reset]
                            跑闸门并给出按优先级排序的待修清单；退出码 0=DONE 1=继续 3=停滞（自动循环用）
                            没有轮数上限：指标达标才 DONE；离阈值的距离连续 loop.stallRounds 轮不缩小才停滞
  baseline [--reset | --tighten]
                            棘轮模式（ratchet.enabled）：记录现有代码的质量欠账 gauntlet-baseline.json（由人确认）；
                            --tighten 只收紧：把基线降到当前值、删掉已还清的欠账（agent 可以运行）
  gate --profile <specifier|coder|cleaner|hardener|full|quality>
                            quality：不需要任何场景，只量现有代码的质量（摸底、棘轮基线用）
                            按阶段运行对应闸门，写 gate.json，失败则退出码 1
  evidence [--title <t>] [--tutorial docs/x.md]
                            生成人类审阅证据包 evidence/{index.html,comment.md}
  demo <script.json>        真实执行命令并录制成可回放的终端演示
  leak-check --mark         记下开始时间（运行程序之前）
  leak-check [--roots a,b] [--ignore a,b] [--depth N]
                            列出开始之后在临时目录 / 家目录（或 --roots）里被改写的文件；为空才 PASS（跨平台，代替 find -newer）
`;

// Every stage re-runs everything before it: quality never depends on an earlier stage's word.
const QUALITY = ['static', 'tidy', 'cppcheck', 'duplication', 'crap', 'arch'];
const PROFILES = {
  specifier: ['spec-allow-undefined'],
  coder: ['spec', 'build', 'tests'],
  cleaner: ['spec', 'build', 'tests', ...QUALITY],
  hardener: ['spec', 'build', 'tests', ...QUALITY, 'coverage', 'sanitize', 'mutation'],
  full: ['spec', 'build', 'tests', ...QUALITY, 'coverage', 'sanitize', 'mutation', 'qa', 'constraints', 'equivalence'],
  // measure code quality without any spec (stage 0 survey, ratchet baseline): no scenarios needed yet
  quality: ['gen', 'build', 'tests', ...QUALITY, 'coverage'],
};

function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const k = a.slice(2);
      const next = argv[i + 1];
      if (next !== undefined && !next.startsWith('--')) { args[k] = next; i++; } else args[k] = true;
    } else args._.push(a);
  }
  return args;
}

function spec(cfg, { allowUndefined }) {
  const r = generate(cfg);
  const { binding } = r;
  console.log(`spec: ${r.ir.features.length} feature(s), ${r.scenarioCount} scenario(s), ${binding.total} step(s), ${r.defs.length} step definition(s)`);
  for (const u of binding.undefinedSteps) console.log(`  ❓ undefined: ${u.text}\n       at ${u.at}\n       suggest: ${u.suggestion} { ... }`);
  for (const a of binding.ambiguous) console.log(`  ⚠ ambiguous: ${a.text} at ${a.at}\n       ${a.matches.join('\n       ')}`);
  const pass = r.scenarioCount > 0 && binding.ambiguous.length === 0 && (allowUndefined || binding.undefinedSteps.length === 0);
  return { pass, scenarios: r.scenarioCount, undefinedSteps: binding.undefinedSteps.length, ambiguous: binding.ambiguous.length };
}

/** Each gate step, implemented by the project's adapter. Both write the same gauntlet-out/*.json files. */
function adapterOps(cfg) {
  if (cfg.adapter === 'commands') {
    return {
      spec: () => generic.specGeneric(cfg),
      build: () => generic.buildGeneric(cfg),
      tests: () => generic.testsGeneric(cfg),
      static: () => generic.staticGeneric(cfg),
      tidy: () => generic.tidyGeneric(cfg),
      crap: () => generic.crapGeneric(cfg),
      arch: () => generic.archGeneric(cfg),
      mutate: (o) => generic.mutationGeneric(cfg, o),
      cppcheck: () => null,   // needs a compilation database: in this adapter cppcheck is just a lint entry
      sanitize: () => runSanitizeCommand(cfg, generic.shell, generic.cmdSpec(cfg.commands?.sanitize)),
    };
  }
  if (cfg.adapter !== 'cmake-clang') throw new Error(`unknown adapter "${cfg.adapter}" (use commands | cmake-clang)`);
  return {
    spec: (o) => spec(cfg, o),
    build: (o = {}) => { configure(cfg, { force: !!o.reconfigure }); build(cfg); },
    tests: () => ({ tests: testWithCoverage(cfg) }),
    static: () => runStatic(cfg),
    tidy: () => runTidy(cfg),
    crap: () => computeCrap(cfg),
    arch: () => checkArchitecture(cfg),
    mutate: (o) => runMutation(cfg, o),
    cppcheck: () => runCppcheck(cfg, { quiet: true }),
    sanitize: () => runSanitize(cfg),
  };
}

function acceptanceGate(a) {
  if (a.error) return { pass: false, error: a.error };
  return { pass: a.pass, scenarios: a.scenarios, passed: a.passed, failed: a.failed.length, missing: a.missing.length, failedScenarios: a.failed.slice(0, 30), missingScenarios: a.missing.slice(0, 30) };
}

function printAcceptance(g) {
  if (g.error) { console.log(`ACCEPTANCE: ${g.error}`); return; }
  console.log(`\nACCEPTANCE  scenarios=${g.scenarios} passed=${g.passed} failed=${g.failed} missing=${g.missing}`);
  for (const n of g.missingScenarios) console.log(`  ❓ 没有对应的测试：${n}`);
  for (const n of g.failedScenarios) console.log(`  ✗ 测试失败：${n}`);
  console.log(g.pass ? 'ACCEPTANCE gate: PASS' : 'ACCEPTANCE gate: FAIL');
}

function gitInfo(cfg) {
  const r = run('git', ['rev-parse', '--short', 'HEAD'], { cwd: cfg.root, quiet: true, allowFail: true });
  // Only the gauntlet project itself counts (it may live in a subdirectory of a bigger repo).
  const dirty = run('git', ['status', '--porcelain', '--', '.'], { cwd: cfg.root, quiet: true, allowFail: true });
  return r.code === 0 ? `${r.stdout.trim()}${dirty.stdout.trim() ? '+dirty' : ''}` : null;
}

async function gate(cfg, args) {
  const profile = args.profile || 'full';
  const steps = PROFILES[profile];
  if (!steps) throw new Error(`unknown profile "${profile}" (use ${Object.keys(PROFILES).join('|')})`);
  const startedAt = new Date().toISOString();
  const gates = {};
  const prev = readJson(cfg.out('gate.json'), {});
  let ratchet;
  const save = () => writeJson(cfg.out('gate.json'), { profile, title: args.title || prev.title, startedAt, finishedAt: new Date().toISOString(), commit: gitInfo(cfg), pass: Object.values(gates).every((g) => g.pass), ...(ratchet ? { ratchet } : {}), gates });
  const useRatchet = ratchetOn(cfg) && !args['no-ratchet'];
  let crapRaw = null;
  let current = null;
  const ops = adapterOps(cfg);
  try {
    for (const s of steps) {
      current = s.startsWith('spec') ? 'spec' : s;
      if (s === 'gen') { if (cfg.adapter !== 'commands') generate(cfg); }
      else if (s === 'spec' || s === 'spec-allow-undefined') gates.spec = await ops.spec({ allowUndefined: s === 'spec-allow-undefined' });
      else if (s === 'build') { await ops.build({ reconfigure: !!args.reconfigure }); gates.build = { pass: true }; }
      else if (s === 'tests') {
        const { tests: t, acceptance } = await ops.tests();
        gates.tests = { pass: t.code === 0 && t.failed === 0 && t.total > 0, total: t.total, failed: t.failed };
        if (acceptance && !steps.includes('gen')) gates.acceptance = acceptanceGate(acceptance);   // quality profile: no scenarios yet
      } else if (s === 'static') {
        const r = await ops.static();
        printStatic(r);
        gates.scope = { pass: r.gates.scope.pass, scope: Math.round(r.summary.scope * 1000) / 1000, failedUnits: r.summary.failedUnits, files: r.summary.productionFiles };
        gates.complexity = { pass: r.gates.complexity.pass, functions: r.summary.functions, offenders: r.summary.offenders, maxComplexity: r.summary.maxComplexity };
        gates.warnings = { pass: r.gates.warnings.pass, warnings: r.summary.warnings };
      } else if (s === 'tidy') { const r = await ops.tidy(); printTidy(r); gates.tidy = { pass: r.pass, ran: r.ran, findings: r.summary.findings, ...(r.error ? { error: r.error } : {}) }; }
      else if (s === 'cppcheck') {
        const r = await ops.cppcheck();
        if (r) { printCppcheck(r); gates.cppcheck = { pass: r.pass, ran: r.ran, findings: r.summary.findings, ...(r.ran ? {} : { reason: r.reason }) }; }
      } else if (s === 'sanitize') {
        if (gates.tests && !gates.tests.pass) { gates.sanitize = { pass: false, error: 'skipped: tests are red' }; continue; }
        const r = await ops.sanitize();
        if (r) { printSanitize(r); gates.sanitize = { pass: r.pass, ran: r.ran, findings: r.findings.length, ...(r.notRun.length ? { notRun: r.notRun } : {}) }; }
      } else if (s === 'duplication') { const r = runDuplication(cfg); printDuplication(r); gates.duplication = { pass: r.pass, ratio: r.summary.ratio, clones: r.summary.clones }; }
      else if (s === 'crap' || s === 'coverage') {
        if (!crapRaw) { crapRaw = await ops.crap(); printCrap(crapRaw); }
        const c = crapRaw;
        if (s === 'crap') gates.crap = { pass: c.functions.every((f) => !f.violations.length), maxCrap: c.summary.maxCrap, offenders: c.summary.offenders, instrumentedFunctions: c.summary.functions };
        else gates.coverage = { pass: c.summary.lineCoverage >= cfg.thresholds.lineCoverageMin, lineCoverage: c.summary.lineCoverage };
      } else if (s === 'arch') { const a = await ops.arch(); printArch(a); gates.arch = { pass: a.pass, violations: a.violations.length, edges: a.edges.length, ...(a.skipped ? { skipped: a.skipped } : {}) }; }
      else if (s === 'mutation') {
        if (gates.tests && !gates.tests.pass) { gates.mutation = { pass: false, error: 'skipped: tests are red' }; continue; }
        const m = await ops.mutate({ base: args.base, changed: !!args.changed || (useRatchet && cfg.ratchet.mutation !== 'all') });
        gates.mutation = { pass: m.pass, score: m.summary.score, total: m.summary.total, survived: m.summary.survived, noCoverage: m.summary.noCoverage, ...(m.summary.unsupported ? { unsupported: m.summary.unsupported } : {}) };
      } else if (s === 'qa') gates.qa = qaGate(cfg);
      else if (s === 'constraints') { const c = constraintsGate(cfg); gates.constraints = { pass: c.pass, total: c.total, met: c.met, error: c.error }; }
      else if (s === 'equivalence') gates.equivalence = equivalenceGate(cfg, { required: !!cfg.requireEquivalence });
      if (s.startsWith('spec') && !gates.spec.pass) break; // nothing else is meaningful
    }
  } catch (e) {
    console.error(`\n✗ ${e.message}`);
    gates[e instanceof GherkinError ? 'spec' : current || 'error'] = { pass: false, error: String(e.message || e) };
  }
  if (useRatchet && QUALITY.some((q) => steps.includes(q))) {
    try {
      ratchet = await applyRatchet(cfg, gates, { lineHits: cfg.adapter === 'commands' ? undefined : () => lineHits(cfg) });
      printRatchet(ratchet);
    } catch (e) { gates.ratchet = { pass: false, error: String(e.message || e) }; }
  }
  save();
  console.log(`\n══ GATE (${profile}${ratchet ? ' · 棘轮模式' : ''}) ══`);
  for (const [k, v] of Object.entries(gates)) console.log(`  ${v.pass ? '✅' : '❌'} ${k.padEnd(12)} ${JSON.stringify({ ...v, pass: undefined })}`);
  const ok = Object.values(gates).length > 0 && Object.values(gates).every((g) => g.pass);
  console.log(ok ? `\nGATE ${profile}: PASS` : `\nGATE ${profile}: FAIL`);
  return ok ? 0 : 1;
}

/** gauntlet baseline: record (or only tighten) the quality debt of an existing code base. */
async function baselineCmd(cfg, args) {
  const file = baselineFile(cfg);
  const rel = path.relative(cfg.root, file);
  if (args.tighten) {
    const old = readJson(file, null);
    if (!old) throw new Error(`没有 ${rel}：先由人确认后运行 \`baseline\``);
    const last = readJson(cfg.out('gate.json'), null);
    const need = ['scope', 'complexity', 'tidy', 'duplication', 'crap', 'coverage', 'arch'];   // cppcheck: only where it runs
    if (!last || need.some((g) => !last.gates?.[g]) || need.some((g) => last.gates[g].error)) {
      throw new Error('--tighten 需要一次包含全部质量闸门的完整运行（gate --profile hardener / full / quality）作为依据，先运行它');
    }
    const next = tighten(old, snapshot(cfg), cfg.thresholds);
    writeBaseline(cfg, next);
    console.log(`baseline --tighten: 遗留欠账 ${debtCount(old)} → ${debtCount(next)} 项（只收紧，不放松）\n  ${rel}`);
    return 0;
  }
  if (fs.existsSync(file) && !args.reset) throw new Error(`${rel} 已存在。重新记录会放松规则，只能由人决定：确认后加 --reset；agent 只能用 --tighten`);
  await gate(cfg, { profile: 'quality', 'no-ratchet': true });
  const b = snapshot(cfg);
  writeBaseline(cfg, b);
  const n = (m) => Object.values(m).reduce((a, v) => a + v, 0);
  console.log(`\nbaseline: 记录了 ${debtCount(b)} 项遗留欠账 -> ${rel}
  超标函数 ${Object.keys(b.functions).length} 个，覆盖率不足的文件 ${Object.keys(b.coverage).length} 个，告警/静态检查 ${n(b.findings)} 条，
  重复代码 ${n(b.clones)} 处（重复率 ${(b.duplicationRatio * 100).toFixed(1)}%），未被分析的文件 ${b.unanalyzed.length} 个，架构违规 ${n(b.arch)} 条
这是规则文件：提交前必须由人确认（证据包会把它列为"需要你确认"）。在 gauntlet.config.json 设 "ratchet": { "enabled": true } 生效。`);
  return 0;
}

function init(root, workdir, adapter) {
  const tpl = path.join(KIT_DIR, 'templates');
  if (adapter && adapter !== true && adapter !== 'commands' && adapter !== 'cmake-clang') throw new Error(`unknown adapter "${adapter}" (use commands | cmake-clang)`);
  const generic = adapter !== 'cmake-clang';
  const copy = (from, to, transform = (s) => s) => {
    const dst = path.join(root, to);
    if (fs.existsSync(dst)) { console.log(`  keep   ${to}`); return; }
    ensureDir(path.dirname(dst));
    fs.writeFileSync(dst, transform(fs.readFileSync(path.join(tpl, from), 'utf8')));
    console.log(`  create ${to}`);
  };
  const wd = typeof workdir === 'string' ? workdir.replace(/\/+$/, '') : null;
  copy(generic ? 'gauntlet.config.commands.json' : 'gauntlet.config.json', 'gauntlet.config.json', (s) => {
    if (!wd) return s;
    const j = JSON.parse(s);
    Object.assign(j, { buildDir: `${wd}/build-gauntlet`, outDir: `${wd}/gauntlet-out`, generatedDir: `${wd}/acceptance-generated` });
    return `${JSON.stringify(j, null, 2)}\n`;
  });
  if (!generic) copy('architecture.json', 'architecture.json');   // the template's paths.architecture default
  ensureDir(path.join(root, 'features'));
  if (!generic) ensureDir(path.join(root, 'acceptance', 'steps'));
  const gi = path.join(root, '.gitignore');
  const want = wd ? [`${wd}/`, 'gauntlet.local.json'] : generic ? ['gauntlet-out/', 'gauntlet.local.json'] : ['build-gauntlet*/', 'gauntlet-out/', 'gauntlet.local.json', 'acceptance/generated/'];
  const have = fs.existsSync(gi) ? fs.readFileSync(gi, 'utf8') : '';
  const missing = want.filter((w) => !have.split(/\r?\n/).includes(w));
  if (missing.length) { fs.appendFileSync(gi, `${have && !have.endsWith('\n') ? '\n' : ''}# gauntlet\n${missing.join('\n')}\n`); console.log(`  update .gitignore (+${missing.join(' ')})`); }
  if (generic) {
    console.log(`\n下一步：编辑 gauntlet.config.json
  sources / exclude   本项目的产品代码（任何语言）和要排除的测试、生成代码
  commands.build      构建命令（解释型语言可删掉）
  commands.test       跑全部测试的命令；必须写出 JUnit XML（junit）和 LCOV 或 Cobertura 覆盖率（lcov / cobertura）
  commands.lint       可选：输出 SARIF 的检查器（eslint、ruff、semgrep、golangci-lint、cppcheck ……）
  commands.arch       可选：架构依赖检查命令（dependency-cruiser、import-linter、ArchUnit ……），退出码 0 = 无违规
  commands.mutation   可选：外部变异测试工具（Stryker、mull、mutmut ……）及其报告；不填用内置变异引擎
验收场景：features/*.feature 里每个场景都要有一个名字包含场景名的测试。
命令里的 {out} 会替换成输出目录。改完运行 node .gauntlet/gauntlet.mjs doctor 检查。`);
    return;
  }
  const gen = wd ? `${wd}/acceptance-generated` : 'acceptance/generated';
  console.log(`\n在顶层 CMakeLists.txt 中加入：\n  include(.gauntlet/cmake/Gauntlet.cmake)\n  gauntlet_add_acceptance(LINK <你的库 target>${wd ? ` GENERATED_DIR \${CMAKE_SOURCE_DIR}/${gen}` : ''})\n并把单元测试注册到 CTest（add_test() 或所用测试框架的发现命令）。`);
}

function doctor(cfg) {
  if (cfg.adapter === 'commands') return doctorCommands(cfg);
  const required = ['cmake', 'ctest', 'ninja', 'clang', 'clang++', 'llvm-profdata', 'llvm-cov', 'git'];
  const optional = ['llvm-cxxfilt', 'clang-tidy', 'nvcc'];
  let ok = true;
  for (const t of required) { const p = which(t); console.log(`${p ? '✅' : '❌'} ${t.padEnd(14)} ${p || 'NOT FOUND'}`); if (!p) ok = false; }
  for (const t of optional) { const p = which(t); console.log(`${p ? '✅' : '➖'} ${t.padEnd(14)} ${p || 'not found (optional)'}`); }
  console.log(`✅ node           ${process.version}（需要 18.17+）`);
  if (!ok) console.log('\n缺少工具：安装 LLVM/clang (含 llvm-profdata/llvm-cov)、CMake ≥ 3.21、Ninja、git。');
  return ok ? 0 : 1;
}

/** commands adapter: every configured command's program must exist; reports must be declared. */
function doctorCommands(cfg) {
  const c = cfg.commands || {};
  let ok = true;
  const check = (label, spec, required) => {
    const s = generic.cmdSpec(spec);
    if (!s?.run) { console.log(`${required ? '❌' : '➖'} ${label.padEnd(14)} ${required ? 'NOT CONFIGURED' : '未配置（可选）'}`); if (required) ok = false; return; }
    const exe = String(s.run).trim().split(/\s+/)[0].replace(/^["']|["']$/g, '');
    const p = which(exe);
    console.log(`${p ? '✅' : '❌'} ${label.padEnd(14)} ${s.run}${p ? '' : `   (${exe} NOT FOUND)`}`);
    if (!p) ok = false;
  };
  check('build', c.build, false);
  check('test', c.test, true);
  const t = generic.cmdSpec(c.test) || {};
  if (!t.junit) { console.log('❌ test.junit     没有声明 JUnit 报告路径'); ok = false; }
  if (!t.lcov && !t.cobertura) { console.log('❌ test.coverage  没有声明 lcov / cobertura 覆盖率报告（CRAP 需要它）'); ok = false; }
  const lint = Array.isArray(c.lint) ? c.lint : c.lint ? [c.lint] : [];
  if (!lint.length) check('lint', null, false);
  lint.forEach((l, i) => check(`lint[${i}]`, l, true));
  check('arch', c.arch, false);
  if (c.mutation) check('mutation', c.mutation, true); else console.log('✅ mutation       内置变异引擎（clike / python）');
  const liz = which(cfg.complexity?.lizard || 'lizard');
  console.log(`${liz ? '✅' : '➖'} lizard         ${liz || 'not found：使用内置复杂度分析（C 系语言 + Python）'}`);
  for (const t2 of ['git']) { const p = which(t2); console.log(`${p ? '✅' : '❌'} ${t2.padEnd(14)} ${p || 'NOT FOUND'}`); if (!p) ok = false; }
  console.log(`✅ node           ${process.version}（需要 18.17+）`);
  return ok ? 0 : 1;
}

// fs.readdirSync(..., { recursive: true }) and friends: Node 18.17 / 20.1 or newer
const NODE_OK = (() => { const [a, b] = process.versions.node.split('.').map(Number); return a > 20 || (a === 20 && b >= 1) || (a === 19 ? false : a === 18 && b >= 17); })();

async function main() {
  if (!NODE_OK) { console.error(`gauntlet: 需要 Node.js 18.17+（或 20.1+），当前是 ${process.version}`); return 2; }
  const args = parseArgs(process.argv.slice(2));
  const cmd = args._[0];
  if (!cmd || cmd === 'help' || args.help) { console.log(HELP); return 0; }
  const root = findRoot();
  if (cmd === 'init') { init(root, args.workdir, args.adapter); return 0; }
  if (cmd === 'doctor' && !fs.existsSync(path.join(root, 'gauntlet.config.json'))) {
    console.log(`✅ node           ${process.version}（需要 18.17+）\n${which('git') ? '✅' : '❌'} git\n还没有 gauntlet.config.json：先运行 survey 看清项目，再 init。`);
    return 1;
  }
  const cfg = loadConfig(root);
  recoverMutations(cfg);   // a mutation run that crashed must never leave mutated source behind
  if (cmd === 'doctor') return doctor(cfg);
  ensureDir(cfg.out());
  const isGeneric = cfg.adapter === 'commands';
  const ops = adapterOps(cfg);
  switch (cmd) {
    case 'spec': return (await ops.spec({ allowUndefined: !!args['allow-undefined'] })).pass ? 0 : 1;
    case 'gen': if (isGeneric) { console.log('commands 适配器不生成代码：场景按名称对应到测试'); return 0; } generate(cfg); console.log(`generated ${cfg.generatedDir}/`); return 0;
    case 'build': if (!isGeneric) generate(cfg); await ops.build({ reconfigure: !!args.reconfigure }); return 0;
    case 'test': {
      if (!isGeneric) generate(cfg);
      await ops.build({});
      const { tests: t, acceptance } = await ops.tests();
      console.log(`tests: ${t.total - t.failed}/${t.total} passed`);
      if (acceptance) printAcceptance(acceptanceGate(acceptance));
      return t.failed || t.code || (acceptance && !acceptance.pass) ? 1 : 0;
    }
    case 'static': { const r = await ops.static(); printStatic(r); return Object.values(r.gates).every((g) => g.pass) ? 0 : 1; }
    case 'tidy': { const r = await ops.tidy(); printTidy(r); return r.pass ? 0 : 1; }
    case 'cppcheck': { const r = await ops.cppcheck(); if (!r) { console.log('cppcheck：只在 cmake-clang 下内置；commands 适配器里把它配成 commands.lint 的一条'); return 0; } printCppcheck(r); return r.pass ? 0 : 1; }
    case 'sanitize': { const r = await ops.sanitize(); printSanitize(r); return !r || r.pass ? 0 : 1; }
    case 'dup': { const r = runDuplication(cfg); printDuplication(r); return r.pass ? 0 : 1; }
    case 'crap': { if (isGeneric && !fs.existsSync(cfg.out('static.json'))) await ops.static(); const r = await ops.crap(); printCrap(r); return r.pass ? 0 : 1; }
    case 'arch': { const r = await ops.arch(); printArch(r); return r.pass ? 0 : 1; }
    case 'mutate': {
      const r = await ops.mutate({ base: args.base, changed: !!args.changed, files: typeof args.files === 'string' ? args.files.split(',') : null });
      return r.pass ? 0 : 1;
    }
    case 'compare': {
      if (!args._[2]) throw new Error('usage: compare <expected> <actual> [--text [--atol y]] [--dtype f32] [--header N] [--frame N] [--tol x|--exact] [--name s] [--record qa/equivalence.json]');
      const e = runCompare(cfg, path.resolve(args._[1]), path.resolve(args._[2]), { text: !!args.text, atol: args.atol, dtype: args.dtype, header: args.header, frame: args.frame, tol: args.tol, exact: !!args.exact, name: typeof args.name === 'string' ? args.name : undefined, record: typeof args.record === 'string' ? args.record : undefined });
      printCompare(e);
      return e.status === 'pass' ? 0 : 1;
    }
    case 'diagram': {
      if (!args._[1]) throw new Error('usage: diagram <candidate.json> [--type architecture] [--name slug]');
      const r = runDiagram(cfg, args._[1], { type: typeof args.type === 'string' ? args.type : undefined, name: typeof args.name === 'string' ? args.name : undefined });
      console.log(`diagram: ${r.status.toUpperCase()}  ${r.html}${r.browserCheck ? `  (browser-check: ${r.browserCheck})` : ''}`);
      if (r.status !== 'pass') console.log(`  failed at ${r.stage}:\n${r.output}`);
      return r.status === 'pass' ? 0 : 1;
    }
    case 'gate': return gate(cfg, args);
    case 'next': {
      const profile = typeof args.profile === 'string' ? args.profile : 'full';
      if (!args['no-run']) await gate(cfg, { ...args, profile });
      const r = decide(cfg, profile, { reset: !!args.reset });
      console.log(`\n${r.text}`);
      console.log(`NEXT ${profile}: ${r.verdict}`);
      return r.verdict === 'DONE' ? 0 : r.verdict === 'STALLED' ? 3 : 1;
    }
    case 'baseline': return baselineCmd(cfg, args);
    case 'survey': { printSurvey(survey(cfg)); return 0; }
    case 'evidence': {
      const r = buildEvidence(cfg, { title: typeof args.title === 'string' ? args.title : undefined, tutorial: typeof args.tutorial === 'string' ? args.tutorial : undefined });
      console.log(`evidence: ${r.verdict}\n  ${r.index}\n  ${r.comment}`);
      return 0;
    }
    case 'demo': {
      if (!args._[1]) throw new Error('usage: demo <script.json>');
      const r = recordDemo(cfg, path.resolve(args._[1]));
      console.log(`demo: ${r.file}${r.failed.length ? `\n  ⚠ ${r.failed.length} command(s) exited with an unexpected code` : ''}`);
      return r.failed.length ? 1 : 0;
    }
    case 'leak-check': {
      if (args.mark) { leakMark(cfg); console.log(`leak-check: marked ${new Date().toISOString()}`); return 0; }
      const r = leakCheck(cfg, { roots: splitList(args.roots), ignore: splitList(args.ignore), depth: args.depth });
      printLeak(r);
      return r.pass ? 0 : 1;
    }
    default: console.log(HELP); return 2;
  }
}

main().then((code) => process.exit(code ?? 0), (e) => { console.error(`gauntlet: ${e.message}`); process.exit(1); });
