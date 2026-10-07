// compile_commands.json -> clang argument lists, for static analysis of EVERY production translation unit,
// including CUDA (.cu) files that the real build compiles with nvcc.
import fs from 'node:fs';
import path from 'node:path';
import { which } from './util.mjs';

/** Split a shell-ish command line (handles "quotes", 'quotes' and \\ escapes). */
export function splitCommand(cmd) {
  const out = [];
  let cur = '';
  let quote = null;
  let has = false;
  for (let i = 0; i < cmd.length; i++) {
    const c = cmd[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === '\\' && quote === '"' && (cmd[i + 1] === '"' || cmd[i + 1] === '\\')) cur += cmd[++i];
      else cur += c;
    } else if (c === '"' || c === "'") { quote = c; has = true; }
    else if (/\s/.test(c)) { if (has || cur) out.push(cur); cur = ''; has = false; }
    else if (c === '\\' && /["'\\ ]/.test(cmd[i + 1] || '')) { cur += cmd[++i]; has = true; }
    else { cur += c; has = true; }
  }
  if (has || cur) out.push(cur);
  return out;
}

export function loadCompileDb(cfg) {
  const file = path.join(cfg.abs(cfg.buildDir), 'compile_commands.json');
  if (!fs.existsSync(file)) throw new Error(`${file} missing: configure with CMAKE_EXPORT_COMPILE_COMMANDS=ON (gauntlet build does this)`);
  const raw = JSON.parse(fs.readFileSync(file, 'utf8'));
  const seen = new Map();
  for (const e of raw) {
    const args = e.arguments || splitCommand(e.command);
    const abs = path.resolve(e.directory, e.file);
    if (!seen.has(abs)) seen.set(abs, { file: abs, directory: e.directory, args });
  }
  return [...seen.values()];
}

// Flags that carry a separate value argument in gcc/clang/nvcc command lines.
const KEEP_WITH_VALUE = new Set(['-I', '-isystem', '-iquote', '-idirafter', '-include', '-D', '-U', '-imacros']);
const KEEP_JOINED = /^(-I|-isystem|-iquote|-idirafter|-D|-U|-std=|--std=|-include|-imacros|-fopenmp$|-pthread$|-f(no-)?exceptions$|-f(no-)?rtti$)/;

let cudaPathCache;
export function detectCudaPath(entryArgs = []) {
  if (cudaPathCache !== undefined) return cudaPathCache;
  const fromNvcc = (p) => (p ? path.dirname(path.dirname(p)) : null);
  const candidates = [
    fromNvcc(entryArgs.find((a) => /(^|[\\/])nvcc(\.exe)?$/.test(a))),
    fromNvcc(which('nvcc')),
    process.env.CUDA_PATH, process.env.CUDA_HOME, '/usr/local/cuda',
  ];
  cudaPathCache = candidates.find((c) => c && fs.existsSync(path.join(c, 'include', 'cuda_runtime.h'))) || null;
  return cudaPathCache;
}

export function languageOf(file) {
  if (/\.c$/i.test(file)) return 'c';
  if (/\.(cu|cuh)$/i.test(file)) return 'cuda';
  return 'c++';
}

/**
 * Translate one compile-db entry (clang, gcc or nvcc) into args for `clang -fsyntax-only`.
 * Only include paths, defines and the language standard survive; codegen flags are dropped.
 */
export function clangArgsFor(entry, cfg) {
  const a = entry.args;
  const keep = [];
  for (let i = 1; i < a.length; i++) {
    let x = a[i];
    if (x === entry.file || path.resolve(entry.directory, x) === entry.file) continue;
    if (x === '-o' || x === '-c' || x === '-MF' || x === '-MT' || x === '-MQ' || x === '-x') { if (x !== '-c') i++; continue; }
    // nvcc spells some flags with '=' or as --long forms
    x = x.replace(/^--include-path=?/, '-I').replace(/^--define-macro=?/, '-D').replace(/^--undefine-macro=?/, '-U').replace(/^-isystem=/, '-isystem');
    if (KEEP_WITH_VALUE.has(x) && i + 1 < a.length) { keep.push(x, a[++i]); continue; }
    if (x === '--std' || x === '-std') { keep.push(`-std=${a[++i]}`); continue; }
    if (KEEP_JOINED.test(x)) keep.push(x.replace(/^--std=/, '-std='));
  }
  // relative include paths are relative to the entry directory
  const fixed = [];
  for (let i = 0; i < keep.length; i++) {
    const x = keep[i];
    if (['-I', '-isystem', '-iquote', '-idirafter', '-include', '-imacros'].includes(x)) { fixed.push(x, path.resolve(entry.directory, keep[++i])); continue; }
    const m = x.match(/^(-I|-isystem|-iquote|-idirafter)(.+)$/);
    fixed.push(m ? `${m[1]}${path.resolve(entry.directory, m[2])}` : x);
  }
  const lang = languageOf(entry.file);
  const langArgs = lang === 'c' ? ['-x', 'c'] : lang === 'c++' ? ['-x', 'c++'] : cudaArgs(cfg, a);
  return { lang, args: [...langArgs, ...fixed, ...(cfg.static.extraArgs || [])] };
}

function cudaArgs(cfg, entryArgs) {
  // static.cudaPath: '' = auto-detect, 'none' = parse without a toolkit, or an explicit toolkit path
  const forced = cfg.static.cudaPath;
  const cuda = forced === 'none' ? null : forced || detectCudaPath(entryArgs);
  const base = ['-x', 'cuda', `--cuda-gpu-arch=${cfg.static.cudaArch}`, '-nocudalib', '-Wno-unknown-cuda-version'];
  if (cuda) return [...base, `--cuda-path=${cuda}`];
  // No CUDA toolkit: still parse, with the attribute keywords spelled out (device builtins will not resolve).
  return [...base, '-nocudainc', '-D__global__=__attribute__((global))', '-D__device__=__attribute__((device))',
    '-D__host__=__attribute__((host))', '-D__shared__=__attribute__((shared))', '-D__constant__=__attribute__((constant))'];
}
