// Language families the language-neutral tools (duplication, built-in complexity, built-in mutation)
// understand without a compiler:
//   clike   C, C++, CUDA, Java, C#, JavaScript/TypeScript, Go, Rust, Kotlin, Swift, Scala, Dart, PHP, ObjC
//   python  Python
//   fortran Fortran, free form (.f90 .f95 .f03 .f08) and fixed form (.f .for .f77: 'fortran-fixed')
// masking blanks comments and string literals (same length, newlines kept) so tokens and operators are
// only ever found in real code.

const CLIKE = /\.(c|cc|cpp|cxx|cu|cuh|h|hh|hpp|hxx|inl|ipp|js|mjs|cjs|jsx|ts|mts|cts|tsx|java|cs|go|rs|kt|kts|swift|scala|dart|php|m|mm)$/i;
const PYTHON = /\.(py|pyw)$/i;
const FORTRAN_FREE = /\.(f90|f95|f03|f08)$/i;
const FORTRAN_FIXED = /\.(f|for|f77)$/i;

export function langOf(file) {
  if (PYTHON.test(file)) return 'python';
  if (FORTRAN_FREE.test(file)) return 'fortran';
  if (FORTRAN_FIXED.test(file)) return 'fortran-fixed';
  if (CLIKE.test(file)) return 'clike';
  return null;
}

/** Blank out comments, string/char/template literals and preprocessor lines of C-like code. */
export function maskSource(src) {
  const out = src.split('');
  let i = 0;
  const blank = (a, b) => { for (let k = a; k < b; k++) if (out[k] !== '\n') out[k] = ' '; };
  while (i < src.length) {
    const c = src[i], n = src[i + 1];
    if (c === '/' && n === '/') { const e = src.indexOf('\n', i); const end = e < 0 ? src.length : e; blank(i, end); i = end; }
    else if (c === '/' && n === '*') { const e = src.indexOf('*/', i + 2); const end = e < 0 ? src.length : e + 2; blank(i, end); i = end; }
    else if (c === 'R' && n === '"' && !/[A-Za-z0-9_]/.test(src[i - 1] || '')) {
      const open = src.indexOf('(', i);
      const delim = src.slice(i + 2, open);
      const close = src.indexOf(`)${delim}"`, open);
      const end = close < 0 ? src.length : close + delim.length + 2;
      blank(i, end); i = end;
    } else if (c === '`') {
      // JS template literal / Go raw string: may span lines
      let j = i + 1;
      while (j < src.length && src[j] !== '`') j += src[j] === '\\' ? 2 : 1;
      blank(i, j + 1); i = j + 1;
    } else if (c === '"' || (c === "'" && !/[0-9A-Fa-f]/.test(src[i - 1] || ''))) {
      let j = i + 1;
      while (j < src.length && src[j] !== c && src[j] !== '\n') j += src[j] === '\\' ? 2 : 1;
      blank(i, j + 1); i = j + 1;
    } else if (c === '#' && /(^|\n)[ \t]*$/.test(src.slice(Math.max(0, i - 200), i))) {
      // preprocessor line (incl. continuations): never mutate
      let j = i;
      while (j < src.length && !(src[j] === '\n' && src[j - 1] !== '\\')) j++;
      blank(i, j); i = j;
    } else i++;
  }
  return out.join('');
}

/** Blank out comments and string literals (incl. triple-quoted and prefixed ones) of Python code. */
export function maskPython(src) {
  const out = src.split('');
  let i = 0;
  const blank = (a, b) => { for (let k = a; k < b; k++) if (out[k] !== '\n') out[k] = ' '; };
  while (i < src.length) {
    const c = src[i];
    if (c === '#') { const e = src.indexOf('\n', i); const end = e < 0 ? src.length : e; blank(i, end); i = end; continue; }
    if (c === '"' || c === "'") {
      // include a string prefix (r, b, f, u, rb, ...) directly in front of the quote
      let s = i;
      while (s > 0 && /[rbfuRBFU]/.test(src[s - 1]) && !/[\w]/.test(src[s - 2] || '')) s--;
      const triple = src.startsWith(c.repeat(3), i);
      const q = triple ? c.repeat(3) : c;
      let j = i + q.length;
      while (j < src.length && !src.startsWith(q, j) && (triple || src[j] !== '\n')) j += src[j] === '\\' ? 2 : 1;
      const end = Math.min(src.length, j + q.length);
      blank(s, end); i = end; continue;
    }
    i++;
  }
  return out.join('');
}

/**
 * Blank out comments, string literals and preprocessor lines of Fortran. Free form: "!" to end of line.
 * Fixed form additionally: a C, c, * or ! in column 1 makes the whole line a comment.
 * Strings use ' or " and escape the quote by doubling it.
 */
export function maskFortran(src, fixed = false) {
  const out = src.split('');
  const blank = (a, b) => { for (let k = a; k < b; k++) if (out[k] !== '\n') out[k] = ' '; };
  let i = 0;
  let lineStart = true;
  while (i < src.length) {
    const c = src[i];
    if (lineStart) {
      lineStart = false;
      const e0 = src.indexOf('\n', i); const eol = e0 < 0 ? src.length : e0;
      if ((fixed && /[Cc*!]/.test(c)) || /^[ \t]*#/.test(src.slice(i, eol))) { blank(i, eol); i = eol; continue; }
    }
    if (c === '\n') { lineStart = true; i++; continue; }
    if (c === '!') { const e = src.indexOf('\n', i); const end = e < 0 ? src.length : e; blank(i, end); i = end; continue; }
    if (c === "'" || c === '"') {
      let j = i + 1;
      while (j < src.length && src[j] !== '\n') {
        if (src[j] === c) { if (src[j + 1] === c) { j += 2; continue; } break; }
        j++;
      }
      blank(i, j + 1); i = j + 1; continue;
    }
    i++;
  }
  return out.join('');
}

export const isFortran = (lang) => lang === 'fortran' || lang === 'fortran-fixed';

export function mask(lang, src) {
  if (lang === 'python') return maskPython(src);
  if (isFortran(lang)) return maskFortran(src, lang === 'fortran-fixed');
  return maskSource(src);
}
