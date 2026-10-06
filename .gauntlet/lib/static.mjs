// Static analysis of EVERY production translation unit with clang (no execution needed, works for CUDA):
//   per function: cyclomatic complexity, length, nesting depth, parameter count, CUDA kind
//   per TU:       compiler warnings (-Wall -Wextra by default) and parse failures
//   per file:     whether static analysis saw it at all (feeds the scope gate)
// The AST is streamed from `clang -fsyntax-only -Xclang -ast-dump` and parsed line by line.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { listSources, makeSourceMatcher, relToRoot, writeJson, readJson, isHeader, pathOf } from './util.mjs';
import { loadCompileDb, clangArgsFor } from './compdb.mjs';
import { mask } from './lang.mjs';

const FUNCTION_KINDS = new Set(['FunctionDecl', 'CXXMethodDecl', 'CXXConstructorDecl', 'CXXDestructorDecl', 'CXXConversionDecl']);
const DECISION_KINDS = new Set(['IfStmt', 'ForStmt', 'WhileStmt', 'DoStmt', 'CXXForRangeStmt', 'CaseStmt', 'CXXCatchStmt', 'ConditionalOperator', 'BinaryConditionalOperator']);
const NEST_KINDS = new Set(['IfStmt', 'ForStmt', 'WhileStmt', 'DoStmt', 'CXXForRangeStmt', 'SwitchStmt']);
const SCOPE_KINDS = new Set(['NamespaceDecl', 'CXXRecordDecl', 'ClassTemplateDecl']);
const CUDA_ATTRS = { CUDAGlobalAttr: 'kernel', CUDADeviceAttr: 'device', CUDAHostAttr: 'host' };

// Location tokens in clang's text dump. Pseudo files (<command line>, <built-in>, <scratch space>) are
// real state changes too: a following "line:N" refers to them.
const LOC_RE = /(<<invalid sloc>>|<invalid sloc>)|(<[^<>]+>):(\d+):(\d+)|line:(\d+):(\d+)|col:(\d+)|(?:Spelling=)?((?:[A-Za-z]:)?[^\s<>:,'"=]+):(\d+):(\d+)/g;

/** Find the source range "<...>" that follows the node address, honouring nested <...>. */
function rangeOf(rest) {
  const start = rest.indexOf('<');
  if (start < 0) return null;
  let depth = 0;
  for (let i = start; i < rest.length; i++) {
    if (rest[i] === '<') depth++;
    else if (rest[i] === '>' && --depth === 0) return { start, end: i };
  }
  return null;
}

/**
 * Streaming parser for clang's textual AST dump. Feed it lines; it calls onFunction for each function
 * definition and tracks every file mentioned in a source location.
 */
export function createAstParser({ cwd, onFunction, onFile }) {
  const st = { file: null, line: 0 };
  const scopes = []; // {depth, name}
  let fn = null;
  const resolve = (f) => path.resolve(cwd, f);
  // returns the location the token denotes, or null for invalid locations
  const take = (m) => {
    if (m[1]) return null;
    if (m[2]) { st.file = m[2]; st.line = +m[3]; st.real = false; }
    else if (m[5]) st.line = +m[5];
    else if (m[7]) { /* column only */ }
    else if (m[8]) { st.file = resolve(m[8]); st.line = +m[9]; st.real = true; onFile?.(st.file); }
    return st.file ? { file: st.file, line: st.line, real: st.real } : null;
  };
  const finish = () => {
    if (fn && fn.hasBody) onFunction(fn);
    fn = null;
  };
  return {
    line(text) {
      let depth;
      let body;
      if (text.startsWith('TranslationUnitDecl')) { depth = 0; body = text; }
      else {
        const m = /^([| `]*)[|`]-/.exec(text);
        if (!m) return;
        depth = m[1].length / 2 + 1;
        body = text.slice(m[0].length);
      }
      const kind = (/^[A-Za-z_]\w*/.exec(body) || [''])[0];
      // update location state; remember begin/end of this node's own range
      const r = rangeOf(body);
      let begin = null;
      let end = null;
      let after = body;
      if (r) {
        const inner = body.slice(r.start + 1, r.end);
        let top = 0;
        let partStart = 0;
        const parts = [];
        for (let i = 0; i < inner.length; i++) {
          if (inner[i] === '<') top++;
          else if (inner[i] === '>') top--;
          else if (inner[i] === ',' && top === 0) { parts.push(inner.slice(partStart, i)); partStart = i + 1; }
        }
        parts.push(inner.slice(partStart));
        for (const mm of body.slice(0, r.start).matchAll(LOC_RE)) take(mm);
        parts.forEach((p, idx) => {
          let first = true;
          for (const mm of p.matchAll(LOC_RE)) {
            const loc = take(mm);
            if (first) { if (idx === 0) begin = loc; if (idx === parts.length - 1) end = loc; first = false; }
          }
        });
        after = body.slice(r.end + 1);
      }
      let nodeLoc = null;
      for (const mm of after.matchAll(LOC_RE)) { const loc = take(mm); if (!nodeLoc) nodeLoc = loc; }
      // A range that starts inside a macro from the command line (e.g. __global__) starts in a pseudo
      // file: use the declaration's own location, which is in the real source file.
      if (begin && !begin.real && nodeLoc?.real) begin = nodeLoc;

      if (fn && depth <= fn.depth) finish();
      while (scopes.length && scopes[scopes.length - 1].depth >= depth) scopes.pop();

      if (fn) {
        if (depth === fn.depth + 1) {
          if (kind === 'CompoundStmt' || kind === 'CXXTryStmt') fn.hasBody = true;
          else if (kind === 'ParmVarDecl') fn.params++;
          else if (CUDA_ATTRS[kind]) fn.cuda = fn.cuda === 'host' || !fn.cuda ? CUDA_ATTRS[kind] : fn.cuda;
        }
        if (DECISION_KINDS.has(kind)) fn.decisions++;
        else if (kind === 'BinaryOperator' && /'(&&|\|\|)'\s*$/.test(body)) fn.decisions++;
        while (fn.nest.length && fn.nest[fn.nest.length - 1].depth >= depth) fn.nest.pop();
        if (NEST_KINDS.has(kind)) {
          const parent = fn.nest[fn.nest.length - 1];
          const elseIf = kind === 'IfStmt' && parent && parent.kind === 'IfStmt' && parent.depth === depth - 1;
          const level = parent ? parent.level + (elseIf ? 0 : 1) : 1;
          fn.nest.push({ depth, kind, level });
          fn.maxNest = Math.max(fn.maxNest, level);
        }
        return;
      }
      if (SCOPE_KINDS.has(kind)) {
        const nm = /\s(?:struct|class|union)?\s*([A-Za-z_]\w*)(?:\s+definition)?\s*$/.exec(after.replace(/'[^']*'/g, ''));
        const ns = /\s([A-Za-z_]\w*)\s*$/.exec(after.replace(/\s+(nested|inline)\b/g, ''));
        scopes.push({ depth, name: kind === 'NamespaceDecl' ? (ns && !/^(line|col)$/.test(ns[1]) ? ns[1] : '(anonymous)') : nm ? nm[1] : '' });
        return;
      }
      if (FUNCTION_KINDS.has(kind) && begin?.real && !/\simplicit\s/.test(` ${after} `)) {
        const nameM = /(?:^|\s)(operator\S+|~?[A-Za-z_]\w*)\s+'/.exec(after);
        const qual = scopes.map((s) => s.name).filter(Boolean);
        fn = {
          depth, kind, name: [...qual, nameM ? nameM[1] : '?'].join('::'),
          file: begin.file, line: begin.line, endLine: end?.file === begin.file ? end.line : begin.line,
          params: 0, decisions: 0, maxNest: 0, nest: [], hasBody: false, cuda: null,
        };
      }
    },
    end() { finish(); },
  };
}

const DIAG_RE = /^((?:[A-Za-z]:)?[^:\n]+):(\d+):(\d+): (warning|error|fatal error): (.*?)(?: \[([^\]]+)\])?$/;

function analyzeTu(cfg, entry, isSrc, sink) {
  const { args } = clangArgsFor(entry, cfg);
  const clangArgs = ['-fsyntax-only', '-fno-color-diagnostics', ...cfg.static.warnings, ...args, '-Xclang', '-ast-dump', entry.file];
  return new Promise((resolve) => {
    const child = spawn(cfg.llvm.clang, clangArgs, { cwd: entry.directory, windowsHide: true });
    const parser = createAstParser({
      cwd: entry.directory,
      onFile: (f) => sink.seen.add(f),
      onFunction: (f) => {
        const rel = relToRoot(cfg, f.file);
        if (!isSrc(rel)) return;
        const key = `${rel}:${f.line}:${f.name}`;
        const prev = sink.functions.get(key);
        const rec = { name: f.name, file: rel, line: f.line, endLine: f.endLine, lines: f.endLine - f.line + 1, complexity: 1 + f.decisions, nesting: f.maxNest, params: f.params, cuda: f.cuda };
        if (!prev || rec.complexity > prev.complexity) sink.functions.set(key, rec);
      },
    });
    readline.createInterface({ input: child.stdout, crlfDelay: Infinity }).on('line', (l) => parser.line(l));
    let stderr = '';
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (e) => resolve({ file: entry.file, ok: false, errors: [`cannot run ${cfg.llvm.clang}: ${e.message}`] }));
    child.on('close', (code) => {
      parser.end();
      const errors = [];
      for (const line of stderr.split(/\r?\n/)) {
        const m = DIAG_RE.exec(line);
        if (!m) {
          if (/(^|\s)(fatal )?error:/.test(line)) errors.push(line.trim()); // driver errors have no location
          continue;
        }
        const file = relToRoot(cfg, path.resolve(entry.directory, m[1]));
        if (m[4] === 'warning') {
          if (!isSrc(file)) continue;
          const key = `${file}:${m[2]}:${m[3]}:${m[5]}`;
          if (!sink.warnings.has(key)) sink.warnings.set(key, { file, line: +m[2], col: +m[3], message: m[5], flag: m[6] || '' });
        } else errors.push(`${file}:${m[2]}: ${m[5]}`);
      }
      if (code !== 0 && !errors.length) errors.push(`clang exited with code ${code}: ${stderr.trim().split(/\r?\n/).slice(-3).join(' | ')}`);
      resolve({ file: entry.file, ok: code === 0 && errors.length === 0, errors: errors.slice(0, 8) });
    });
  });
}

async function pool(items, limit, fn) {
  const out = [];
  let i = 0;
  const workers = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (i < items.length) { const k = i++; out[k] = await fn(items[k], k); }
  });
  await Promise.all(workers);
  return out;
}

/** A line that carries code after comments/strings/preprocessor are blanked (lone braces do not count). */
export const isCodeLine = (maskedLine) => !!maskedLine && !!maskedLine.trim() && !/^\s*[{}();]*\s*$/.test(maskedLine);

export function codeLines(src, lang = 'clike') {
  return mask(lang, src).split('\n').filter(isCodeLine).length;
}

/** Load human-approved exceptions: [{kind: 'warning'|'tidy'|'unbuilt'|'function', file, match?, check?, reason}] */
export function loadAccepted(cfg) {
  return readJson(pathOf(cfg, 'qualityAccepted'), []);
}

export async function runStatic(cfg, { quiet = false } = {}) {
  const isSrc = makeSourceMatcher(cfg);
  const db = loadCompileDb(cfg);
  const prodFiles = listSources(cfg);
  const tus = db.filter((e) => isSrc(relToRoot(cfg, e.file)));
  const sink = { seen: new Set(), functions: new Map(), warnings: new Map() };
  const limit = Math.max(1, Math.min(8, Math.floor(os.cpus().length / 2)));
  if (!quiet) console.log(`static: ${tus.length} production translation unit(s), ${prodFiles.length} production file(s), ${limit} parallel clang job(s)`);
  const results = await pool(tus, limit, async (e, k) => {
    const r = await analyzeTu(cfg, e, isSrc, sink);
    if (!quiet) process.stdout.write(`  [${k + 1}/${tus.length}] ${r.ok ? 'ok    ' : 'FAILED'} ${relToRoot(cfg, e.file)}\n`);
    return r;
  });
  const failed = results.filter((r) => !r.ok).map((r) => ({ file: relToRoot(cfg, r.file), errors: r.errors }));
  const failedSet = new Set(failed.map((f) => f.file));
  const seenRel = new Set([...sink.seen].map((f) => relToRoot(cfg, f).toLowerCase()));
  const tuSet = new Set(tus.map((e) => relToRoot(cfg, e.file)));
  const accepted = loadAccepted(cfg);

  const files = prodFiles.map((abs) => {
    const rel = relToRoot(cfg, abs);
    const src = fs.readFileSync(abs, 'utf8');
    let status;
    if (failedSet.has(rel)) status = 'failed';
    else if (tuSet.has(rel) || (isHeader(rel) && seenRel.has(rel.toLowerCase()))) status = 'ok';
    else status = isHeader(rel) ? 'not-included' : 'not-built';
    const acc = status !== 'ok' && accepted.find((a) => a.kind === 'unbuilt' && a.file === rel);
    return { file: rel, lines: codeLines(src), status: acc ? 'accepted' : status, reason: acc ? acc.reason : undefined };
  });

  const th = cfg.thresholds;
  const functions = [...sink.functions.values()].map((f) => {
    const v = [];
    if (f.complexity > th.complexityMax) v.push(`圈复杂度 ${f.complexity} > ${th.complexityMax}`);
    if (f.lines > th.functionLinesMax) v.push(`长度 ${f.lines} 行 > ${th.functionLinesMax}`);
    if (f.nesting > th.nestingMax) v.push(`嵌套 ${f.nesting} 层 > ${th.nestingMax}`);
    if (f.params > th.paramsMax) v.push(`参数 ${f.params} 个 > ${th.paramsMax}`);
    const acc = v.length && accepted.find((a) => a.kind === 'function' && a.file === f.file && (!a.match || f.name.includes(a.match)));
    return { ...f, violations: acc ? [] : v, accepted: acc ? acc.reason : undefined };
  }).sort((a, b) => b.complexity - a.complexity || b.lines - a.lines);

  const warnings = [...sink.warnings.values()].map((w) => {
    const acc = accepted.find((a) => a.kind === 'warning' && a.file === w.file && (!a.match || w.message.includes(a.match) || w.flag === a.match));
    return { ...w, accepted: acc ? acc.reason : undefined };
  }).sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);

  const totalLines = files.reduce((n, f) => n + f.lines, 0);
  const okLines = files.filter((f) => f.status === 'ok' || f.status === 'accepted').reduce((n, f) => n + f.lines, 0);
  const offenders = functions.filter((f) => f.violations.length);
  const liveWarnings = warnings.filter((w) => !w.accepted);
  const result = {
    thresholds: th,
    summary: {
      productionFiles: files.length,
      translationUnits: tus.length,
      failedUnits: failed.length,
      functions: functions.length,
      cudaFunctions: functions.filter((f) => f.cuda === 'kernel' || f.cuda === 'device').length,
      maxComplexity: Math.max(0, ...functions.map((f) => f.complexity)),
      maxLines: Math.max(0, ...functions.map((f) => f.lines)),
      maxNesting: Math.max(0, ...functions.map((f) => f.nesting)),
      maxParams: Math.max(0, ...functions.map((f) => f.params)),
      offenders: offenders.length,
      warnings: liveWarnings.length,
      codeLines: totalLines,
      scope: totalLines ? okLines / totalLines : 1,
    },
    gates: {
      scope: { pass: failed.length === 0 && (totalLines ? okLines / totalLines : 1) >= th.staticScopeMin },
      complexity: { pass: offenders.length === 0 },
      warnings: { pass: liveWarnings.length <= th.warningsMax },
    },
    files,
    failed,
    functions,
    warnings,
  };
  writeJson(cfg.out('static.json'), result);
  return result;
}

export function printStatic(r) {
  const s = r.summary;
  console.log(`\nSTATIC  files=${s.productionFiles} TUs=${s.translationUnits} failed=${s.failedUnits} functions=${s.functions}${s.cudaFunctions ? ` (CUDA ${s.cudaFunctions})` : ''}  scope=${(s.scope * 100).toFixed(1)}%`);
  console.log(`        maxCC=${s.maxComplexity} maxLines=${s.maxLines} maxNesting=${s.maxNesting} maxParams=${s.maxParams} offenders=${s.offenders} warnings=${s.warnings}`);
  for (const f of r.failed.slice(0, 10)) console.log(`  ✗ parse failed: ${f.file}\n      ${f.errors.slice(0, 3).join('\n      ')}`);
  for (const f of r.files.filter((x) => x.status === 'not-built' || x.status === 'not-included').slice(0, 15)) console.log(`  ✗ ${f.status === 'not-built' ? '未参与构建' : '未被产品代码包含'}: ${f.file}`);
  for (const f of r.functions.filter((x) => x.violations.length).slice(0, 15)) console.log(`  ✗ ${f.file}:${f.line} ${f.name}  ${f.violations.join('; ')}`);
  for (const w of r.warnings.filter((x) => !x.accepted).slice(0, 15)) console.log(`  ⚠ ${w.file}:${w.line}:${w.col} ${w.message}${w.flag ? ` [${w.flag}]` : ''}`);
  for (const [k, g] of Object.entries(r.gates)) console.log(`${k.toUpperCase()} gate: ${g.pass ? 'PASS' : 'FAIL'}`);
}
