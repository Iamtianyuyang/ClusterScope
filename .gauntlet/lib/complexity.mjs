// Per-function metrics for any language, without a compiler:
//   lizard (https://github.com/terryyin/lizard, ~30 languages) when it is installed, otherwise
//   a built-in analyzer for C-like languages (brace blocks) and Python (indentation).
// Every function: { name, file, line, endLine, complexity, lines, nesting, params }.
//   complexity = 1 + if/for/while/case/catch/&&/||/ternary (Python: if/elif/for/while/except/and/or/case)
//   nesting    = deepest block inside the body (null when the engine cannot tell)
// Nested functions are measured separately and excluded from their parent, like lizard does.
import { langOf, mask, isFortran } from './lang.mjs';
import { run, which } from './util.mjs';

const NOT_A_NAME = new Set([
  'if', 'for', 'foreach', 'while', 'switch', 'catch', 'with', 'using', 'lock', 'synchronized', 'fixed', 'when',
  'return', 'sizeof', 'typeof', 'alignof', 'decltype', 'new', 'delete', 'throw', 'await', 'match', 'case', 'do',
  'else', 'try', 'defer', 'go', 'select', 'yield', 'in', 'of', 'constexpr', 'noexcept', 'requires', 'static_assert',
]);
const ANON = new Set(['function', 'func', 'fn', 'fun', 'lambda']);
const TYPE_DECL = /\b(class|struct|interface|enum|record|object|impl|trait|namespace|union|extension|protocol)\b/;
const QUALIFIERS = /^[\s\w:<>,[\]&*.?|+'-]*$/;   // what may follow ")" before "{": const, throws X, -> T, : T, where T: C
const CLIKE_DECISION = /\b(if|for|foreach|while|case|catch)\b|&&|\|\||\s\?\s/g;
const PY_DECISION = /\b(if|elif|for|while|except|and|or|case)\b/g;
const PY_BLOCK = /^(if|elif|else|for|while|try|except|finally|with|match|case)\b/;

function lineIndex(src) {
  const starts = [0];
  for (let i = 0; i < src.length; i++) if (src[i] === '\n') starts.push(i + 1);
  return (pos) => { let lo = 0, hi = starts.length - 1; while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (starts[mid] <= pos) lo = mid; else hi = mid - 1; } return lo + 1; };
}

function matchingOpen(s, close) {
  let depth = 0;
  for (let k = close; k >= 0; k--) {
    if (s[k] === ')') depth++;
    else if (s[k] === '(' && --depth === 0) return k;
  }
  return -1;
}

function countParams(text) {
  const t = text.trim();
  if (!t || t === 'void') return 0;
  let depth = 0, n = 1;
  for (const c of t) {
    if ('([{<'.includes(c)) depth++;
    else if (')]}>'.includes(c)) depth--;
    else if (c === ',' && depth === 0) n++;
  }
  return n;
}

// a block that starts with one of these is control flow, also without parentheses (Go/Rust/Swift "if f(x) {")
const CONTROL_START = /^(?:\}\s*)?(?:else\b\s*)?(if|for|foreach|while|loop|match|switch|select|when|unless|guard|case|catch|return|elif|until)\b/;

/** Is the text in front of a "{" a function header? -> {name, params} | null */
export function parseHeader(header, { arrows = true } = {}) {
  let h = header.replace(/\s+$/, '');
  if (/=>$/.test(h)) {
    if (!arrows) return null;
    // arrow function: (a, b) => {   or   x => {
    const body = h.slice(0, -2).trimEnd();
    if (body.endsWith(')')) { const o = matchingOpen(body, body.length - 1); return { name: '(arrow)', params: o < 0 ? 0 : countParams(body.slice(o + 1, -1)) }; }
    return { name: '(arrow)', params: 1 };
  }
  if (CONTROL_START.test(h.trimStart())) return null;
  const close = h.lastIndexOf(')');
  if (close < 0 || !QUALIFIERS.test(h.slice(close + 1))) return null;
  let open = matchingOpen(h, close);
  if (open < 0) return null;
  let params = h.slice(open + 1, close);
  // Go "func f(a int) (int, error) {", C++ "f(int a) noexcept(true) {": step back over the trailing group
  for (let guard = 0; guard < 2; guard++) {
    const before = h.slice(0, open).trimEnd();
    if (before.endsWith(')') || /\bnoexcept$/.test(before)) {
      const c2 = before.endsWith(')') ? before.length - 1 : before.lastIndexOf('noexcept') - 1;
      const b2 = h.slice(0, c2 + 1).trimEnd();
      if (!b2.endsWith(')')) break;
      const o2 = matchingOpen(b2, b2.length - 1);
      if (o2 < 0) break;
      params = b2.slice(o2 + 1, -1);
      open = o2;
    } else break;
  }
  const before = h.slice(0, open);
  if (TYPE_DECL.test(before) || /[=;]\s*$/.test(before)) return null;
  if (/\]\s*$/.test(before)) return { name: '(lambda)', params: countParams(params) };
  const m = before.match(/([A-Za-z_$~][\w$]*(?:\s*(?:::|\.)\s*~?[A-Za-z_$][\w$]*)*)\s*(?:<[^()]*>)?\s*$/);
  if (!m) return null;
  const last = m[1].split(/::|\./).pop().trim();
  if (NOT_A_NAME.has(last)) return null;
  if (/\bnew\s+[\w.$:<>]*$/.test(before)) return null;  // anonymous class / constructor call
  return { name: ANON.has(m[1]) ? '(anonymous)' : m[1].replace(/\s+/g, ''), params: countParams(params) };
}

function clikeFunctions(src, file) {
  // "=> {" is a closure in JS/TS/C#/Scala/Dart, but a match arm in Rust and an array entry in PHP
  const arrows = !/\.(rs|php)$/i.test(file);
  const masked = mask('clike', src);
  const lineOf = lineIndex(src);
  const fns = [];
  const stack = [];
  let segStart = 0;
  for (let i = 0; i < masked.length; i++) {
    const c = masked[i];
    if (c === '{') {
      const fn = parseHeader(masked.slice(segStart, i), { arrows });
      if (fn) {
        // a header may span lines: the function starts on the line of its name / parameter list
        const head = masked.slice(segStart, i);
        const off = head.search(/\S/);
        fns.push({ ...fn, open: i, headPos: segStart + Math.max(0, off) });
        stack.push(fns.length - 1);
      } else stack.push(null);
      segStart = i + 1;
    } else if (c === '}') {
      const top = stack.pop();
      if (top != null) fns[top].close = i;
      segStart = i + 1;
    } else if (c === ';') segStart = i + 1;
  }
  const done = fns.filter((f) => f.close != null);
  return done.map((f) => {
    const inner = done.filter((g) => g !== f && g.open > f.open && g.close < f.close).map((g) => [g.headPos, g.close]);
    const own = (pos) => !inner.some(([a, b]) => pos >= a && pos <= b);
    let complexity = 1;
    for (const m of masked.slice(f.open, f.close).matchAll(CLIKE_DECISION)) if (own(f.open + m.index)) complexity++;
    let depth = 0, nesting = 0;
    for (let k = f.open + 1; k < f.close; k++) {
      if (!own(k)) continue;
      if (masked[k] === '{') nesting = Math.max(nesting, ++depth);
      else if (masked[k] === '}') depth--;
    }
    const line = lineOf(f.headPos), endLine = lineOf(f.close);
    return { name: f.name, line, endLine, complexity, lines: endLine - line + 1, nesting, params: f.params };
  });
}

function pythonFunctions(src) {
  const lines = mask('python', src).split('\n');
  const indentOf = (l) => l.match(/^[ \t]*/)[0].replace(/\t/g, '    ').length;
  const blank = (l) => !l || !l.trim();
  const fns = [];
  const scope = [];
  for (let n = 0; n < lines.length; n++) {
    const l = lines[n];
    if (blank(l)) continue;
    const ind = indentOf(l);
    while (scope.length && scope[scope.length - 1].indent >= ind) scope.pop();
    const cls = l.match(/^\s*class\s+([A-Za-z_]\w*)/);
    if (cls) { scope.push({ indent: ind, name: cls[1] }); continue; }
    const def = l.match(/^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(/);
    if (!def) continue;
    // signature: up to the ":" at bracket depth 0 (may span lines)
    let depth = 0, sigEnd = n, params = '', started = false, k = n, col = l.indexOf('(');
    outer: for (; k < lines.length; k++, col = 0) {
      const s = lines[k];
      for (let c = col; c < s.length; c++) {
        const ch = s[c];
        if ('([{'.includes(ch)) { depth++; if (depth === 1 && ch === '(' && !started) { started = true; continue; } }
        else if (')]}'.includes(ch)) { depth--; if (depth === 0 && started && ch === ')') { started = 'done'; continue; } }
        else if (ch === ':' && depth === 0 && started === 'done') { sigEnd = k; break outer; }
        if (started === true) params += ch;
      }
      if (started === true) params += ',';
    }
    let end = sigEnd;
    for (let j = sigEnd + 1; j < lines.length; j++) {
      if (blank(lines[j])) continue;
      if (indentOf(lines[j]) <= ind) break;
      end = j;
    }
    const plist = params.split(',').map((p) => p.trim()).filter(Boolean).filter((p) => p !== 'self' && p !== 'cls' && p !== '*' && p !== '/');
    const name = [...scope.map((s) => s.name), def[1]].join('.');
    fns.push({ name, start: n, sigEnd, end, indent: ind, params: plist.length });
    scope.push({ indent: ind, name: def[1] });
  }
  return fns.map((f) => {
    const inner = fns.filter((g) => g !== f && g.start > f.start && g.end <= f.end);
    const own = (j) => !inner.some((g) => j >= g.start && j <= g.end);
    let complexity = 1, nesting = 0;
    const bodyIndent = (() => { for (let j = f.sigEnd + 1; j <= f.end; j++) if (!blank(lines[j])) return indentOf(lines[j]); return f.indent + 4; })();
    const unit = Math.max(1, bodyIndent - f.indent);
    for (let j = f.start; j <= f.end; j++) {
      if (!own(j) || blank(lines[j])) continue;
      const text = j === f.start ? lines[j].slice(lines[j].indexOf('(')) : lines[j];
      complexity += (text.match(PY_DECISION) || []).length;
      if (j > f.sigEnd && PY_BLOCK.test(lines[j].trim())) nesting = Math.max(nesting, Math.floor((indentOf(lines[j]) - bodyIndent) / unit) + 1);
    }
    return { name: f.name, line: f.start + 1, endLine: f.end + 1, complexity, lines: f.end - f.start + 1, nesting, params: f.params };
  });
}

// Fortran: procedures are subroutine / function / program units closed by "end [subroutine|function|program]"
// (or a bare "end"); internal procedures after "contains" are measured separately.
const F_START = /^\s*(?:\d+\s+)?(?:(?:pure|impure|elemental|recursive|module|non_recursive)\s+)*(?:(?:integer|real|double\s+precision|complex|logical|character|type\s*\([^)]*\)|class\s*\([^)]*\))(?:\s*\([^)]*\)|\s*\*\s*\d+)?\s+)?(?:(?:pure|impure|elemental|recursive)\s+)*(subroutine|function)\s+(\w+)\s*(\([^)]*\))?|^\s*(program)\s+(\w+)/;
const F_END = /^\s*(?:\d+\s+)?end\s*(?:(subroutine|function|program)\b.*)?$/;
const F_DECISION = /\bif\s*\(|(?<!select\s*)\bcase\s*\((?!\s*default)|\.and\.|\.or\.|\bwhere\s*\(/g;
const F_DO = /^\s*(?:\d+\s+)?(?:\w+\s*:\s*)?do\b/;
const F_OPEN = /^\s*(?:\w+\s*:\s*)?(?:if\s*\(.*\)\s*then\s*$|do\b(?!\s+\d)|select\s*(?:case|type|rank)\b|where\s*\(.*\)\s*$|forall\s*\(.*\)\s*$|block\s*$|associate\s*\(|critical\b)/;
const F_CLOSE = /^\s*(?:\d+\s+)?(?:end\s*(?:if|do|select|where|forall|block|associate|critical)\b)/;

function fortranFunctions(src, lang) {
  const raw = mask(lang, src).split('\n').map((l) => l.toLowerCase());
  // join free-form continuation lines ("&" at the end) for header / statement matching
  const lines = raw.map((l, i) => (/&\s*$/.test(l) ? l.replace(/&\s*$/, ' ') + (raw[i + 1] || '').replace(/^\s*&/, '') : l));
  const fns = [];
  const stack = [];
  let module = null, inInterface = 0;
  for (let n = 0; n < lines.length; n++) {
    const l = lines[n];
    if (!l.trim()) continue;
    if (/^\s*(abstract\s+)?interface\b/.test(l)) { inInterface++; continue; }
    if (/^\s*end\s*interface\b/.test(l)) { inInterface = Math.max(0, inInterface - 1); continue; }
    if (inInterface) continue;
    const mod = l.match(/^\s*(?:sub)?module\s+(?:\([^)]*\)\s*)?(\w+)\s*$/);
    if (mod && !/^\s*module\s+procedure/.test(l)) { module = mod[1]; continue; }
    if (/^\s*end\s*(sub)?module\b/.test(l)) { module = null; continue; }
    const st = F_START.exec(l);
    if (st && !/^\s*end\b/.test(l)) {
      const name = st[2] || st[5];
      const params = st[3] ? st[3].slice(1, -1).trim() : '';
      stack.push({ name: module && !stack.length ? `${module}.${name}` : stack.length ? `${stack[stack.length - 1].name}.${name}` : name, start: n, params: params ? params.split(',').length : 0 });
      continue;
    }
    if (stack.length && F_END.test(l) && !F_CLOSE.test(l) && !/^\s*end\s*(type|module|interface|enum)\b/.test(l)) {
      const f = stack.pop();
      fns.push({ ...f, end: n });
    }
  }
  return fns.map((f) => {
    const inner = fns.filter((g) => g !== f && g.start > f.start && g.end < f.end);
    const own = (j) => !inner.some((g) => j >= g.start && j <= g.end);
    let complexity = 1, depth = 0, nesting = 0;
    for (let j = f.start + 1; j < f.end; j++) {
      if (!own(j)) continue;
      const l = raw[j];
      // "end if", "enddo", "end select", ... close a block; no decision is counted on them
      if (/^\s*(?:\d+\s+)?end/.test(l)) {
        if (F_CLOSE.test(l) || /^\s*(?:\d+\s+)?end(if|do)\b/.test(l)) depth = Math.max(0, depth - 1);
        continue;
      }
      complexity += (l.match(F_DECISION) || []).length + (F_DO.test(l) ? 1 : 0);
      if (F_OPEN.test(l)) nesting = Math.max(nesting, ++depth);
    }
    return { name: f.name, line: f.start + 1, endLine: f.end + 1, complexity, lines: f.end - f.start + 1, nesting, params: f.params };
  });
}

/** Built-in analysis of one file -> functions, or null when the language is not supported. */
export function analyzeFile(file, src) {
  const lang = langOf(file);
  if (lang === 'python') return pythonFunctions(src);
  if (isFortran(lang)) return fortranFunctions(src, lang);
  if (lang === 'clike') return clikeFunctions(src, file);
  return null;
}

function parseCsvLine(line) {
  const out = [];
  let cur = '', q = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (q) { if (c === '"' && line[i + 1] === '"') { cur += '"'; i++; } else if (c === '"') q = false; else cur += c; }
    else if (c === '"') q = true;
    else if (c === ',') { out.push(cur); cur = ''; }
    else cur += c;
  }
  out.push(cur);
  return out;
}

/** lizard --csv: NLOC,CCN,token,PARAM,length,location,file,function,long_name,start,end */
export function runLizard(exe, absFiles) {
  const byFile = new Map();
  for (let i = 0; i < absFiles.length; i += 50) {
    const chunk = absFiles.slice(i, i + 50);
    const r = run(exe, ['--csv', ...chunk], { quiet: true, allowFail: true });
    for (const line of r.stdout.split(/\r?\n/)) {
      const c = parseCsvLine(line.trim());
      if (c.length < 11 || !/^\d+$/.test(c[1])) continue;
      const fn = { name: c[7], line: +c[9], endLine: +c[10], complexity: +c[1], lines: +c[10] - +c[9] + 1, nesting: null, params: +c[3] };
      (byFile.get(c[6]) || byFile.set(c[6], []).get(c[6])).push(fn);
    }
  }
  return byFile;
}

export function lizardExe(cfg) {
  const engine = cfg.complexity?.engine || 'auto';
  if (engine === 'builtin') return null;
  const exe = which(cfg.complexity?.lizard || 'lizard');
  if (!exe && engine === 'lizard') throw new Error('complexity.engine = "lizard" but lizard is not installed (pip install lizard)');
  return exe;
}
