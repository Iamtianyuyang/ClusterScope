// `gauntlet next`: turn a gate result into a prioritized punch list and decide, mechanically, whether
// the agent must keep looping. There is NO fixed iteration budget: the loop ends when the metrics reach
// their thresholds (CRAP, complexity, coverage, mutation score, ...), or when they stop moving.
//   exit 0  DONE     every gate of the profile passes -> finish the stage
//   exit 1  CONTINUE still failing, but the distance to the thresholds shrank (or this is the first round)
//   exit 3  STALLED  the distance did not shrink for `loop.stallRounds` rounds in a row -> stop and ask
//                    (stallRounds 0 = never stall; only the metrics end the loop)
// distance = number of open items + how far each measured value is beyond its threshold, so pushing a
// function from CRAP 182 to CRAP 40 counts as progress even though it is still one open item.
// History lives in <outDir>/loop-<profile>.json (a DONE or --reset starts a new loop); the list in next.md.
import fs from 'node:fs';
import { readJson, writeJson, pathOf } from './util.mjs';
import { loadAcceptance } from './gen.mjs';
import { constraintsGate } from './checks.mjs';

const GATE_ORDER = ['spec', 'build', 'tests', 'acceptance', 'scope', 'warnings', 'complexity', 'tidy', 'cppcheck', 'duplication', 'crap', 'arch', 'coverage', 'sanitize', 'mutation', 'qa', 'constraints', 'equivalence'];

const HOW = {
  spec: '在 acceptance/steps/ 用 GT_STEP 实现该步骤（措辞要与场景完全一致）',
  build: '修复编译/链接错误，先让构建通过',
  tests: '找出失败原因并修复产品代码（不要改测试的期望）',
  acceptance: '为缺少的场景写一个名字包含场景名的验收测试（大纲场景每行 Examples 一个），并让它通过',
  scope: '让该文件被分析到：不支持的语言装 lizard；CMake 项目让 clang 能解析它：接入构建、修编译参数（static.extraArgs/cudaPath 写进 gauntlet.local.json）；确属死代码就删除；都不行 NEED-HUMAN',
  warnings: '修掉告警本身，不要用 pragma 或删 -W 选项压掉',
  complexity: '提取函数 / 卫语句 / 表驱动 / 参数对象，把指标降到阈值内',
  tidy: '按静态检查（clang-tidy / lint）的提示修复；确认是误报才写 quality-accepted.json 并给出理由',
  cppcheck: '按 cppcheck 的提示修复（未初始化、越界、资源泄漏……）；确认是误报才写 quality-accepted.json（kind "cppcheck"）并给出理由',
  sanitize: '运行时内存 / 未定义行为错误：按报告里的位置修复产品代码，并补一个能复现它的测试；不能关掉 sanitizer 或跳过测试',
  duplication: '把两处相同的逻辑合并成一个共享函数/模块，后端只保留真正不同的部分',
  crap: '降低复杂度，或为该函数补测试',
  arch: '按依赖方向重构（依赖倒置）；需要改 architecture.json 时 NEED-HUMAN',
  coverage: '为未执行的代码补测试；确属死代码就删除',
  mutation: '补一个能区分原代码和变异代码的断言（边界值、另一半条件、返回值）',
  qa: '修复产品使其行为符合 QA 期望，或证明 QA 期望写错并 NEED-HUMAN',
  constraints: '补一条真实执行的 QA 检查证实该约束；做不到就 NEED-HUMAN，不能宣布超出范围',
  equivalence: '找出新旧程序输出不一致的原因并修复；不能放宽容差',
};

/** Collect concrete punch-list items for every failing gate, most fundamental gate first. */
export function punchList(cfg) {
  const gate = readJson(cfg.out('gate.json'), null);
  if (!gate) return { gate: null, items: [] };
  const failing = GATE_ORDER.filter((g) => gate.gates[g] && !gate.gates[g].pass)
    .concat(Object.keys(gate.gates).filter((g) => !GATE_ORDER.includes(g) && !gate.gates[g].pass));
  const items = [];
  // excess: how far beyond the threshold (relative), so partial improvements register as progress
  const add = (g, where, what, excess = 0) => items.push({ gate: g, where, what, how: HOW[g] || '', excess: Math.max(0, excess) });
  const th = cfg.thresholds;
  const over = (v, max) => (v != null && max ? Math.max(0, v / max - 1) : 0);
  const o = (f) => readJson(cfg.out(f), null);
  for (const g of failing) {
    const info = gate.gates[g];
    if (info.error) { add(g, '-', info.error); continue; }
    switch (g) {
      case 'spec': {
        if (cfg.adapter === 'commands') { add(g, cfg.features, 'features/*.feature 里没有任何场景'); break; }
        try {
          const { binding } = loadAcceptance(cfg);
          binding.undefinedSteps.forEach((u) => add(g, u.at, `未定义步骤：${u.text}  建议 ${u.suggestion}`));
          binding.ambiguous.forEach((a) => add(g, a.at, `步骤匹配多个定义：${a.text}`));
        } catch (e) { add(g, '-', e.message); }
        break;
      }
      case 'tests': (o('tests.json')?.cases || []).filter((c) => c.status === 'failed').forEach((c) => add(g, c.title || c.name, '测试失败（看测试输出里的失败原因）')); break;
      case 'acceptance':
        (info.missingScenarios || []).forEach((n) => add(g, n, '没有名字包含该场景名的测试'));
        (info.failedScenarios || []).forEach((n) => add(g, n, '对应的验收测试失败'));
        break;
      case 'scope': {
        const s = o('static.json');
        s?.failed.forEach((f) => add(g, f.file, `clang 解析失败：${f.errors[0] || ''}`));
        s?.files.filter((f) => !f.legacy && (f.status === 'not-built' || f.status === 'not-included')).forEach((f) => add(g, f.file, f.status === 'not-built' ? '没有参与构建（compile_commands.json 里没有它）' : '没有被任何产品编译单元包含'));
        s?.files.filter((f) => !f.legacy && f.status === 'unsupported').forEach((f) => add(g, f.file, '内置分析器不认识这种语言（装 lizard，或 NEED-HUMAN）'));
        break;
      }
      case 'warnings': o('static.json')?.warnings.filter((w) => !w.accepted && !w.legacy).forEach((w) => add(g, `${w.file}:${w.line}`, `${w.message}${w.flag ? ` [${w.flag}]` : ''}`)); break;
      case 'complexity': o('static.json')?.functions.filter((f) => f.violations.length).forEach((f) => add(g, `${f.file}:${f.line}`, `${f.name}：${f.violations.join('；')}`,
        over(f.complexity, th.complexityMax) + over(f.lines, th.functionLinesMax) + over(f.nesting, th.nestingMax) + over(f.params, th.paramsMax))); break;
      case 'tidy': o('tidy.json')?.findings.filter((f) => !f.accepted && !f.legacy).forEach((f) => add(g, `${f.file}:${f.line}`, `${f.message} [${f.check}]`)); break;
      case 'cppcheck': o('cppcheck.json')?.findings.filter((f) => !f.accepted && !f.legacy).forEach((f) => add(g, `${f.file}:${f.line}`, `${f.message} [${f.severity}/${f.check}]`)); break;
      case 'sanitize': {
        const r = o('sanitize.json');
        r?.findings.forEach((f) => add(g, `${f.file}:${f.line}`, `[${f.tool}] ${f.kind}${f.test ? `（测试 ${f.test}）` : ''}`));
        if (r && !r.findings.length) r.notRun.forEach((n) => add(g, '-', `没有运行：${n}`));
        break;
      }
      case 'duplication': o('duplication.json')?.clones.filter((c) => !c.accepted && !c.legacy).forEach((c) => add(g, `${c.a.file}:${c.a.start}-${c.a.end}`, `${c.lines} 行与 ${c.b.file}:${c.b.start}-${c.b.end} 完全相同`)); break;
      case 'crap': o('crap.json')?.functions.filter((f) => f.violations.length).forEach((f) => add(g, `${f.file}:${f.line}`, `${f.name}：${f.violations.join('；')}（覆盖率 ${(f.coverage * 100).toFixed(0)}%）`,
        over(f.crap, th.crapMax) + over(f.complexity, th.complexityMax))); break;
      case 'coverage': if (o('crap.json')?.ratchet) {
        const r = o('crap.json').ratchet.coverage;
        r.regressions.forEach((f) => add(g, f.file, `覆盖率 ${(f.percent * 100).toFixed(1)}% 低于基线 ${f.baseline == null ? '（阈值）' : `${(f.baseline * 100).toFixed(1)}%`}`, 1));
        if (r.diffCoverage < th.lineCoverageMin) r.uncovered.forEach((u) => add(g, u.file, `改动行没有被测试执行：${u.lines.join(', ')}${u.note ? `（${u.note}）` : ''}（改动行覆盖率 ${(r.diffCoverage * 100).toFixed(1)}% < ${th.lineCoverageMin * 100}%）`, (th.lineCoverageMin - r.diffCoverage) / th.lineCoverageMin));
        break;
      } else o('crap.json')?.files.filter((f) => f.percent < th.lineCoverageMin).forEach((f) => add(g, f.file, `行覆盖率 ${(f.percent * 100).toFixed(1)}%（${f.covered}/${f.lines}）`,
        (th.lineCoverageMin - f.percent) / th.lineCoverageMin)); break;
      case 'arch': o('arch.json')?.violations.filter((v) => !v.legacy).forEach((v) => add(g, v.refs[0] ? `${v.refs[0].file}:${v.refs[0].line}` : '-', v.message)); break;
      case 'mutation': o('mutation.json')?.unsupported?.forEach((f) => add(g, f, '内置变异引擎不支持这种语言：配置 commands.mutation（外部变异工具，输出 mutation-testing-report-schema），做不到就 NEED-HUMAN'));
        o('mutation.json')?.survivors.forEach((s) => add(g, `${s.file}:${s.line}`, `${s.status === 'NO_COVERAGE' ? '未覆盖' : '存活'} ${s.op}  | ${String(s.source || '').trim()}`)); break;
      case 'qa': (readJson(pathOf(cfg, 'qa', 'qa-report.json'), {}).checks || []).filter((c) => c.status !== 'pass').forEach((c) => add(g, c.id || c.title, `${c.title}：期望 ${c.expected}，实际 ${c.actual}`)); break;
      case 'constraints': {
        const c = constraintsGate(cfg);
        if (c.error) add(g, `${cfg.paths.qa}/constraints.json`, c.error);
        c.items.filter((i) => i.state !== 'met').forEach((i) => add(g, i.id, `${i.state === 'violated' ? '未满足' : '未证实'}：${i.text}`));
        break;
      }
      case 'equivalence': {
        const doc = readJson(pathOf(cfg, 'qa', 'equivalence.json'), null);
        if (!doc) add(g, `${cfg.paths.qa}/equivalence.json`, '要求与原程序对比，但还没有任何对比记录');
        else doc.entries.filter((e) => e.status !== 'pass').forEach((e) => add(g, e.name, e.error || `最大相对 L2 ${e.worstRelL2} > ${e.tol}${e.nanMismatch ? `，NaN 位置不一致 ${e.nanMismatch} 处` : ''}`));
        break;
      }
      default: add(g, '-', JSON.stringify(info));
    }
    if (!items.some((it) => it.gate === g)) add(g, '-', `闸门未通过：${JSON.stringify({ ...info, pass: undefined })}`);
  }
  return { gate, items, failing };
}

export function distanceOf(items) {
  return Math.round((items.length + items.reduce((n, it) => n + (it.excess || 0), 0)) * 1000) / 1000;
}

export function decide(cfg, profile, { reset = false } = {}) {
  const { gate, items, failing } = punchList(cfg);
  const histFile = cfg.out(`loop-${profile}.json`);
  let hist = readJson(histFile, { rounds: [] });
  // a finished loop (DONE) or an explicit --reset starts a new one: a rework round must not inherit the old history
  if (reset || hist.rounds[hist.rounds.length - 1]?.verdict === 'DONE') hist = { rounds: [] };
  const stallRounds = cfg.loop?.stallRounds ?? 3;
  const distance = distanceOf(items);
  const prev = hist.rounds.slice();
  // progress = closer to the thresholds than every earlier round of this loop (fewer items / gates also count)
  const best = (k) => Math.min(Infinity, ...prev.map((r) => (k === 'failing' ? r.failing.length : r[k] ?? Infinity)));
  const improved = distance < best('distance') - 1e-9 || items.length < best('remaining') || (failing || []).length < best('failing');
  let streak = 0;   // rounds in a row, ending with this one, that did not improve
  if (!improved) { streak = 1; for (let k = prev.length - 1; k >= 0 && !prev[k].improved; k--) streak++; }
  let verdict;
  if (gate && gate.pass) verdict = 'DONE';
  else verdict = stallRounds && streak >= stallRounds ? 'STALLED' : 'CONTINUE';
  hist.rounds.push({ at: new Date().toISOString(), commit: gate?.commit, remaining: items.length, distance, failing: failing || [], improved, verdict });
  writeJson(histFile, hist);
  const last = prev[prev.length - 1];
  const lines = [
    `# gauntlet next — ${profile} — 第 ${hist.rounds.length} 轮`,
    '',
    `结论：**${verdict}**　剩余 ${items.length} 项，离阈值的距离 ${distance}${last ? `（上一轮 ${last.remaining} 项 / ${last.distance ?? '-'}）` : ''}　失败的闸门：${(failing || []).join(', ') || '无'}`,
    '',
  ];
  if (verdict === 'CONTINUE') lines.push('下一步：修第 1 项（同一闸门的同类问题可以一起修），然后重新运行本命令。没有轮数上限：只要指标还在向阈值靠近就继续。', '');
  if (verdict === 'STALLED') lines.push(`已经连续 ${stallRounds} 轮离阈值的距离没有缩小：停止，按收尾协议以 FAIL / NEED-HUMAN 结束，在结果里贴出本清单。`, '');
  let shown = 0;
  for (const g of failing || []) {
    const its = items.filter((i) => i.gate === g);
    lines.push(`## ${g}（${its.length} 项）— ${HOW[g] || ''}`);
    for (const it of its.slice(0, 25)) { shown++; lines.push(`${shown}. \`${it.where}\` ${it.what}`); }
    if (its.length > 25) lines.push(`… 还有 ${its.length - 25} 项（完整数据见输出目录的 *.json）`);
    lines.push('');
  }
  fs.writeFileSync(cfg.out('next.md'), lines.join('\n'));
  return { verdict, items, failing, distance, rounds: hist.rounds.length, text: lines.join('\n') };
}
