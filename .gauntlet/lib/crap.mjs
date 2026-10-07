// CRAP (Change Risk Anti-Patterns) per function, straight from llvm-cov export:
//   complexity = 1 + number of branch conditions clang instrumented in the function
//                (&&, ||, ?:, if/for/while conditions, each switch case except `default`)
//   coverage   = executed code regions / code regions
//   CRAP       = complexity^2 * (1 - coverage)^3 + complexity
import fs from 'node:fs';
import { run, makeSourceMatcher, relToRoot, writeJson } from './util.mjs';
import { loadCoverage } from './build.mjs';

const CODE_REGION = 0;

export function crapScore(cc, cov) {
  return cc * cc * Math.pow(1 - cov, 3) + cc;
}

const FILE_PREFIX = /^[^:?]*\.(?:c|cc|cpp|cxx|h|hh|hpp|hxx):/i;

/** Readable name for an MSVC-mangled symbol: "?run@cli@roman@@YAH..." -> "roman::cli::run". */
export function prettyMsvc(name) {
  if (!name.startsWith('?')) return name;
  let body = name.slice(1);
  let special = '';
  if (body.startsWith('?0')) { special = 'ctor'; body = body.slice(2); }
  else if (body.startsWith('?1')) { special = 'dtor'; body = body.slice(2); }
  else if (body.startsWith('?$')) {
    // function template: ??$name@<template args>@<scopes>@@...  ->  scope::name<…>
    const fname = body.slice(2, body.indexOf('@'));
    const after = body.indexOf('@@@');
    const scopes = after > 0 ? body.slice(after + 3, body.indexOf('@@', after + 3)).split('@').filter((p) => p && !p.startsWith('?')) : [];
    return fname ? [...scopes.reverse(), `${fname}<…>`].join('::') : name;
  } else if (body.startsWith('?')) return name; // operators: keep raw
  const end = body.indexOf('@@');
  if (end < 0) return name;
  const parts = body.slice(0, end).split('@').map((p) => (/^\?A0x[0-9a-f]+$/i.test(p) ? '(anonymous)' : p));
  if (parts.some((p) => p.startsWith('?') || p === '')) return name;
  const [fn, ...scopes] = parts;
  const cls = special ? fn : null;
  const leaf = special === 'ctor' ? cls : special === 'dtor' ? `~${cls}` : fn;
  return [...scopes.reverse(), ...(special ? [cls] : []), leaf].join('::');
}

/** "ns::(anonymous namespace)::f(int, std::pair<int,int>) const" -> "ns::(anonymous namespace)::f" */
export function stripParams(s) {
  let t = s.replace(/\s+(const|volatile|&&|&|noexcept)\s*$/g, '').trimEnd();
  if (!t.endsWith(')')) return t;
  let depth = 0;
  for (let i = t.length - 1; i >= 0; i--) {
    if (t[i] === ')') depth++;
    else if (t[i] === '(' && --depth === 0) return t.slice(0, i).trimEnd() || t;
  }
  return t;
}

function demangleAll(cfg, names) {
  const stripped = names.map((n) => n.replace(FILE_PREFIX, ''));
  const out = new Map(names.map((n, i) => [n, prettyMsvc(stripped[i])]));
  const itanium = stripped.map((n, i) => [n, i]).filter(([n]) => n.startsWith('_Z'));
  if (itanium.length) {
    try {
      const res = run(cfg.llvm.cxxfilt, [], { quiet: true, allowFail: true, input: itanium.map(([n]) => n).join('\n') });
      if (res.code === 0) {
        const lines = res.stdout.split(/\r?\n/);
        itanium.forEach(([, i], k) => lines[k] && out.set(names[i], stripParams(lines[k])));
      }
    } catch { /* demangling is cosmetic */ }
  }
  return out;
}

/** Drop `default:` labels: clang reports them as switch "branches" but McCabe does not count them. */
function makeDefaultFilter() {
  const cache = new Map();
  return (file, b) => {
    if (!cache.has(file)) { try { cache.set(file, fs.readFileSync(file, 'utf8').split(/\r?\n/)); } catch { cache.set(file, null); } }
    const lines = cache.get(file);
    const text = lines?.[b[0] - 1]?.slice(b[1] - 1) ?? '';
    return !/^default\b/.test(text);
  };
}

/** Merge duplicate function records (inline/template code appears once per binary/TU). */
function collectFunctions(cfg, cov) {
  const isSrc = makeSourceMatcher(cfg);
  const notDefault = makeDefaultFilter();
  const byKey = new Map();
  for (const data of cov.data) {
    for (const fn of data.functions) {
      const file = relToRoot(cfg, fn.filenames[0]);
      if (!isSrc(file)) continue;
      const regions = fn.regions.filter((r) => r[5] === 0 && r[7] === CODE_REGION);
      if (!regions.length) continue;
      const branches = (fn.branches || []).filter((b) => b[6] === 0 && notDefault(fn.filenames[0], b));
      const key = `${file}|${fn.name}`;
      const prev = byKey.get(key);
      if (!prev) {
        byKey.set(key, { name: fn.name, file, line: regions[0][0], endLine: Math.max(...regions.map((r) => r[2])), count: fn.count, regions: regions.map((r) => [...r]), branches });
      } else {
        prev.count += fn.count;
        if (prev.regions.length === regions.length) regions.forEach((r, i) => { prev.regions[i][4] += r[4]; });
        else if (fn.count > 0 && prev.count === fn.count) prev.regions = regions.map((r) => [...r]);
      }
    }
  }
  return [...byKey.values()];
}

export function computeCrap(cfg) {
  const cov = loadCoverage(cfg);
  const fns = collectFunctions(cfg, cov);
  const pretty = demangleAll(cfg, fns.map((f) => f.name));
  const th = cfg.thresholds;
  const functions = fns.map((f) => {
    const total = f.regions.length;
    const hit = f.regions.filter((r) => r[4] > 0).length;
    const coverage = total ? hit / total : 0;
    const complexity = 1 + f.branches.length;
    const crap = crapScore(complexity, coverage);
    const display = pretty.get(f.name) || f.name;
    return {
      name: display,
      mangled: f.name,
      file: f.file,
      line: f.line,
      endLine: f.endLine,
      calls: f.count,
      complexity,
      coverage: Math.round(coverage * 1000) / 1000,
      crap: Math.round(crap * 100) / 100,
      violations: [
        ...(crap > th.crapMax ? [`CRAP ${crap.toFixed(1)} > ${th.crapMax}`] : []),
        ...(complexity > th.complexityMax ? [`complexity ${complexity} > ${th.complexityMax}`] : []),
      ],
    };
  }).sort((a, b) => b.crap - a.crap);

  // File-level line coverage of measured production code.
  const isSrc = makeSourceMatcher(cfg);
  const files = [];
  for (const data of cov.data) for (const f of data.files) {
    const rel = relToRoot(cfg, f.filename);
    if (!isSrc(rel)) continue;
    files.push({ file: rel, lines: f.summary.lines.count, covered: f.summary.lines.covered, percent: f.summary.lines.count ? f.summary.lines.covered / f.summary.lines.count : 1 });
  }
  const lines = files.reduce((a, f) => a + f.lines, 0);
  const covered = files.reduce((a, f) => a + f.covered, 0);
  const lineCoverage = lines ? covered / lines : 1;

  const offenders = functions.filter((f) => f.violations.length);
  const result = {
    thresholds: th,
    summary: {
      functions: functions.length,
      maxCrap: functions[0]?.crap ?? 0,
      maxComplexity: Math.max(0, ...functions.map((f) => f.complexity)),
      offenders: offenders.length,
      lineCoverage: Math.round(lineCoverage * 1000) / 1000,
      linesTotal: lines,
      linesCovered: covered,
    },
    pass: offenders.length === 0 && lineCoverage >= th.lineCoverageMin,
    functions,
    files: files.sort((a, b) => a.percent - b.percent),
  };
  writeJson(cfg.out('crap.json'), result);
  return result;
}

/** Map<relFile, Map<line, count>> of executed line counts (for mutation NO_COVERAGE). */
export function lineHits(cfg) {
  const cov = loadCoverage(cfg);
  const map = new Map();
  for (const data of cov.data) for (const f of data.files) {
    const rel = relToRoot(cfg, f.filename);
    const lines = map.get(rel) || new Map();
    const segs = f.segments;
    for (let i = 0; i < segs.length; i++) {
      const [line, , count, hasCount, , isGap] = segs[i];
      if (!hasCount || isGap) continue;
      const next = segs[i + 1];
      const endLine = next ? (next[1] > 1 ? next[0] : next[0] - 1) : line;
      for (let l = line; l <= Math.max(line, endLine); l++) lines.set(l, Math.max(lines.get(l) || 0, count));
    }
    map.set(rel, lines);
  }
  return map;
}

export function printCrap(r) {
  const s = r.summary;
  console.log(`\nCRAP  functions=${s.functions}  maxCRAP=${s.maxCrap}  maxCC=${s.maxComplexity}  lineCoverage=${(s.lineCoverage * 100).toFixed(1)}%  offenders=${s.offenders}`);
  for (const f of r.functions.slice(0, 15)) {
    const flag = f.violations.length ? '  <-- ' + f.violations.join('; ') : '';
    console.log(`  ${String(f.crap).padStart(7)}  cc=${String(f.complexity).padStart(2)}  cov=${(f.coverage * 100).toFixed(0).padStart(3)}%  ${f.file}:${f.line}  ${f.name}${flag}`);
  }
  console.log(r.pass ? 'CRAP gate: PASS' : 'CRAP gate: FAIL');
}

