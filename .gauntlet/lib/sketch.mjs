// Dependency sketch: a module diagram the evidence pack can always show, with no external tool.
// When the project has architecture rules (arch.json from the cmake-clang adapter) those modules and
// edges are used as they are; otherwise modules are the product directories and edges are inferred from
// `#include "..."`, relative JS/TS imports, Python imports and Go imports that resolve to product files.
// It is a sketch: it is labelled as such in the evidence pack and never replaces an Archify diagram.
import fs from 'node:fs';
import path from 'node:path';
import { listSources, relToRoot, readJson, writeJson } from './util.mjs';

const MAX_MODULES = 14;

/** Module of a file: the first directory level that actually separates the product code. */
function moduleNamer(files) {
  const firstDirs = new Set(files.map((f) => (f.includes('/') ? f.split('/')[0] : '.')));
  const depth = firstDirs.size === 1 && [...firstDirs][0] !== '.' ? 2 : 1;
  return (f) => {
    const parts = f.split('/');
    return parts.length > depth ? parts.slice(0, depth).join('/') : parts.length > 1 ? parts.slice(0, parts.length - 1).join('/') : '.';
  };
}

function resolver(cfg, files) {
  const set = new Set(files);
  const bySuffix = (spec) => files.filter((f) => f === spec || f.endsWith(`/${spec}`));
  const goMod = (() => { try { return (fs.readFileSync(cfg.abs('go.mod'), 'utf8').match(/^module\s+(\S+)/m) || [])[1] || null; } catch { return null; } })();
  let fmods = null;
  const fortranModules = () => {
    if (fmods) return fmods;
    fmods = new Map();
    for (const f of files) {
      if (!/\.(f|for|f77|f90|f95|f03|f08)$/i.test(f)) continue;
      let src = '';
      try { src = fs.readFileSync(cfg.abs(f), 'utf8'); } catch { continue; }
      for (const m of src.matchAll(/^[ \t]*module[ \t]+(?!procedure\b)(\w+)[ \t]*$/gim)) fmods.set(m[1].toLowerCase(), [f]);
    }
    return fmods;
  };
  const JS_EXT = ['', '.js', '.mjs', '.cjs', '.ts', '.tsx', '.jsx', '.mts', '.cts', '/index.js', '/index.ts', '/index.mjs'];
  return (from, lang, spec) => {
    const dir = path.posix.dirname(from);
    if (lang === 'c') {
      const local = path.posix.normalize(path.posix.join(dir, spec));
      if (set.has(local)) return [local];
      const m = bySuffix(spec);
      return m.length === 1 ? m : [];
    }
    if (lang === 'js') {
      if (!spec.startsWith('.')) return [];
      const base = path.posix.normalize(path.posix.join(dir, spec));
      for (const e of JS_EXT) if (set.has(base + e)) return [base + e];
      const ts = base.replace(/\.js$/, '.ts');
      return set.has(ts) ? [ts] : [];
    }
    if (lang === 'py') {
      let mod = spec;
      let baseDir = '';
      const dots = (mod.match(/^\.+/) || [''])[0].length;
      if (dots) { baseDir = dir.split('/').slice(0, Math.max(0, dir.split('/').length - (dots - 1))).join('/'); mod = mod.slice(dots); }
      const rel = mod.replace(/\./g, '/');
      const cands = dots ? [`${baseDir ? `${baseDir}/` : ''}${rel}`] : [rel, ...[...new Set(files.map((f) => f.split('/')[0]))].map((t) => `${t}/${rel}`)];
      for (const c of cands) for (const e of ['.py', '/__init__.py']) if (set.has(c + e)) return [c + e];
      return [];
    }
    if (lang === 'fortran') return fortranModules().get(spec) || [];
    if (lang === 'go') {
      if (!goMod || !spec.startsWith(`${goMod}/`)) return [];
      const d = spec.slice(goMod.length + 1);
      return files.filter((f) => path.posix.dirname(f) === d).slice(0, 1);
    }
    return [];
  };
}

function importsOf(file, src) {
  const out = [];
  const push = (lang, spec, idx) => out.push({ lang, spec, line: src.slice(0, idx).split('\n').length });
  if (/\.(c|cc|cpp|cxx|cu|cuh|h|hh|hpp|hxx|inl|ipp|m|mm)$/i.test(file)) {
    for (const m of src.matchAll(/^[ \t]*#[ \t]*include[ \t]*"([^"]+)"/gm)) push('c', m[1], m.index);
  } else if (/\.(m?[jt]sx?|c[jt]s|mts|cts)$/i.test(file)) {
    for (const m of src.matchAll(/(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\s*\(\s*)['"]([^'"]+)['"]/g)) push('js', m[1], m.index);
  } else if (/\.pyw?$/i.test(file)) {
    for (const m of src.matchAll(/^[ \t]*from[ \t]+([.\w]+)[ \t]+import\b/gm)) push('py', m[1], m.index);
    for (const m of src.matchAll(/^[ \t]*import[ \t]+([\w.]+(?:[ \t]*,[ \t]*[\w.]+)*)/gm)) for (const s of m[1].split(',')) push('py', s.trim(), m.index);
  } else if (/\.(f|for|f77|f90|f95|f03|f08)$/i.test(file)) {
    for (const m of src.matchAll(/^[ \t]*use[ \t]*(?:,[^:]*::)?[ \t]*(\w+)/gim)) push('fortran', m[1].toLowerCase(), m.index);
  } else if (/\.go$/i.test(file)) {
    for (const m of src.matchAll(/import\s*\(([\s\S]*?)\)|import\s+(?:\w+\s+)?"([^"]+)"/g)) {
      if (m[2]) push('go', m[2], m.index);
      else for (const q of m[1].matchAll(/"([^"]+)"/g)) push('go', q[1], m.index);
    }
  }
  return out;
}

export function buildSketch(cfg) {
  const arch = readJson(cfg.out('arch.json'), null);
  if (arch && !arch.skipped && arch.modules?.length) {
    const s = { source: 'architecture', modules: arch.modules, edges: arch.edges, violations: arch.violations || [] };
    writeJson(cfg.out('sketch.json'), s);
    return s;
  }
  const files = listSources(cfg).map((abs) => relToRoot(cfg, abs));
  if (!files.length) return null;
  const modOf = moduleNamer(files);
  const resolve = resolver(cfg, files);
  const counts = new Map();
  const edges = new Map();
  for (const f of files) {
    const m = modOf(f);
    counts.set(m, (counts.get(m) || 0) + 1);
    let src;
    try { src = fs.readFileSync(cfg.abs(f), 'utf8'); } catch { continue; }
    for (const imp of importsOf(f, src)) {
      for (const target of resolve(f, imp.lang, imp.spec)) {
        const t = modOf(target);
        if (t === m) continue;
        const key = `${m}→${t}`;
        const e = edges.get(key) || { from: m, to: t, allowed: true, refs: [] };
        if (e.refs.length < 5) e.refs.push({ file: f, line: imp.line });
        edges.set(key, e);
      }
    }
  }
  // keep the largest modules readable; the rest collapse into one box
  let names = [...counts.keys()].sort((a, b) => counts.get(b) - counts.get(a));
  const other = '（其他）';
  const keep = new Set(names.slice(0, MAX_MODULES - 1));
  const rename = (n) => (names.length <= MAX_MODULES || keep.has(n) ? n : other);
  const modules = new Map();
  for (const [n, c] of counts) { const r = rename(n); modules.set(r, (modules.get(r) || 0) + c); }
  const merged = new Map();
  for (const e of edges.values()) {
    const from = rename(e.from), to = rename(e.to);
    if (from === to) continue;
    const key = `${from}→${to}`;
    const cur = merged.get(key) || { from, to, allowed: true, refs: [] };
    cur.refs.push(...e.refs.slice(0, 5 - cur.refs.length));
    merged.set(key, cur);
  }
  const s = {
    source: 'imports',
    modules: [...modules].map(([name, n]) => ({ name, files: n, mayDependOn: [] })),
    edges: [...merged.values()],
    violations: [],
  };
  writeJson(cfg.out('sketch.json'), s);
  return s;
}

export const sketchMermaid = (s) => ['flowchart LR', ...s.modules.map((m, i) => `  m${i}["${m.name.replace(/"/g, "'")}<br/>${m.files} 个文件"]`),
  ...s.edges.map((e) => `  m${s.modules.findIndex((m) => m.name === e.from)} --> m${s.modules.findIndex((m) => m.name === e.to)}`)].join('\n');

