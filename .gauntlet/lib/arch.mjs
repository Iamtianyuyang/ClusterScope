// Architecture constitution check: module boundaries + allowed dependency directions,
// derived from #include "..." edges. architecture.json is HUMAN-owned; agents may not edit it.
//
// architecture.json:
// {
//   "modules": {
//     "domain": { "paths": ["src/domain/**"], "mayDependOn": [] },
//     "app":    { "paths": ["src/app/**"],    "mayDependOn": ["domain"] }
//   },
//   "includeRoots": ["src", "include"],
//   "forbidCycles": true,
//   "strict": true            // every production file must belong to a module
// }
import fs from 'node:fs';
import path from 'node:path';
import { listRepoFiles, makeSourceMatcher, relToRoot, globToRegExp, readJson, writeJson, CODE_EXT, pathOf } from './util.mjs';

export function checkArchitecture(cfg) {
  const spec = readJson(pathOf(cfg, 'architecture'), null);
  if (!spec) {
    const r = { pass: true, skipped: `no ${cfg.paths.architecture}`, modules: [], edges: [], violations: [], unmapped: [] };
    writeJson(cfg.out('arch.json'), r);
    return r;
  }
  const mods = Object.entries(spec.modules || {}).map(([name, m]) => ({ name, res: (m.paths || []).map(globToRegExp), may: new Set(m.mayDependOn || []), files: 0 }));
  const moduleOf = (rel) => mods.find((m) => m.res.some((r) => r.test(rel)));
  const roots = (spec.includeRoots || ['src', 'include']).map((r) => cfg.abs(r));
  const isSrc = makeSourceMatcher(cfg);
  // Module paths are scanned on their own: a narrow `sources` list must not hide code from this check.
  const files = listRepoFiles(cfg, (rel) => CODE_EXT.test(rel) && (isSrc(rel) || !!moduleOf(rel)));
  const known = new Set(files.map((f) => path.resolve(f).toLowerCase()));

  const edgeMap = new Map(); // "a->b" -> [{from,to,line}]
  const unmapped = [];
  const unresolved = [];
  let includes = 0;
  for (const abs of files) {
    const rel = relToRoot(cfg, abs);
    const from = moduleOf(rel);
    if (!from) { if (isSrc(rel)) unmapped.push(rel); continue; }
    from.files++;
    const lines = fs.readFileSync(abs, 'utf8').split(/\r?\n/);
    lines.forEach((text, i) => {
      const m = text.match(/^\s*#\s*include\s*(?:"([^"]+)"|<([^>]+)>)/);
      if (!m) return;
      const inc = m[1] || m[2];
      const cands = [...(m[1] ? [path.resolve(path.dirname(abs), inc)] : []), ...roots.map((r) => path.resolve(r, inc))];
      const hit = cands.find((c) => known.has(c.toLowerCase()) || fs.existsSync(c));
      if (!hit) {
        // "quoted" includes are project headers: not finding one means includeRoots is incomplete
        if (m[1]) unresolved.push({ file: rel, line: i + 1, include: inc });
        return; // <angle> includes that are not in the project are system / third-party headers
      }
      includes++;
      const to = moduleOf(relToRoot(cfg, hit));
      if (!to || to === from) return;
      const key = `${from.name}->${to.name}`;
      (edgeMap.get(key) || edgeMap.set(key, []).get(key)).push({ file: rel, line: i + 1, include: inc });
    });
  }

  const edges = [...edgeMap.entries()].map(([key, refs]) => {
    const [from, to] = key.split('->');
    const allowed = mods.find((m) => m.name === from).may.has(to);
    return { from, to, allowed, refs };
  });
  const violations = edges.filter((e) => !e.allowed).map((e) => ({ kind: 'forbidden-dependency', message: `${e.from} -> ${e.to} is not in ${e.from}.mayDependOn`, refs: e.refs }));

  if (spec.forbidCycles !== false) {
    const adj = new Map(mods.map((m) => [m.name, edges.filter((e) => e.from === m.name).map((e) => e.to)]));
    const state = new Map();
    const stack = [];
    const dfs = (n) => {
      state.set(n, 1); stack.push(n);
      for (const t of adj.get(n) || []) {
        if (state.get(t) === 1) violations.push({ kind: 'cycle', message: `dependency cycle: ${[...stack.slice(stack.indexOf(t)), t].join(' -> ')}`, refs: [] });
        else if (!state.get(t)) dfs(t);
      }
      stack.pop(); state.set(n, 2);
    };
    for (const m of mods) if (!state.get(m.name)) dfs(m.name);
  }
  if (spec.strict !== false && unmapped.length) violations.push({ kind: 'unmapped', message: `${unmapped.length} production file(s) belong to no module`, refs: unmapped.map((f) => ({ file: f, line: 1 })) });
  if (unresolved.length) violations.push({ kind: 'unresolved-include', message: `${unresolved.length} project #include(s) could not be resolved: add the directory to includeRoots, otherwise this check is blind to them`, refs: unresolved });
  for (const m of mods) if (!m.files) violations.push({ kind: 'empty-module', message: `module ${m.name} matches no file: its paths are wrong`, refs: [] });
  // A declared dependency that never shows up is a hint that the scan missed it (or the design changed).
  const warnings = [];
  for (const m of mods) for (const dep of m.may) if (!edges.some((e) => e.from === m.name && e.to === dep)) warnings.push(`${m.name} 允许依赖 ${dep}，但没有发现任何 #include`);

  const result = {
    pass: violations.length === 0,
    summary: { files: files.length, includesResolved: includes, unresolved: unresolved.length },
    modules: mods.map((m) => ({ name: m.name, files: m.files, mayDependOn: [...m.may] })),
    edges,
    violations,
    warnings,
    unmapped,
    mermaid: toMermaid(mods, edges),
  };
  writeJson(cfg.out('arch.json'), result);
  return result;
}

function toMermaid(mods, edges) {
  const id = (n) => n.replace(/[^A-Za-z0-9_]/g, '_');
  const out = ['flowchart LR'];
  for (const m of mods) out.push(`  ${id(m.name)}["${m.name}<br/>${m.files} files"]`);
  edges.forEach((e) => out.push(`  ${id(e.from)} -->${e.allowed ? '' : '|❌ forbidden|'} ${id(e.to)}`));
  edges.forEach((e, i) => { if (!e.allowed) out.push(`  linkStyle ${i} stroke:#d33,stroke-width:3px`); });
  return out.join('\n');
}

export function printArch(r) {
  if (r.skipped) { console.log(`ARCH: skipped (${r.skipped})`); return; }
  console.log(`\nARCH modules=${r.modules.length} edges=${r.edges.length} violations=${r.violations.length}`);
  for (const w of r.warnings || []) console.log(`  ⚠ ${w}`);
  for (const v of r.violations) {
    console.log(`  ✗ ${v.message}`);
    for (const ref of v.refs.slice(0, 5)) console.log(`      ${ref.file}:${ref.line}${ref.include ? `  #include "${ref.include}"` : ''}`);
  }
  console.log(r.pass ? 'ARCH gate: PASS' : 'ARCH gate: FAIL');
}
