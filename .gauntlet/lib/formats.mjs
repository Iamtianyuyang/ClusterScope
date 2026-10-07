// Readers for the standard report formats every ecosystem can produce. The commands adapter turns
// them into the gauntlet's own JSON files, so gates, `next` and the evidence pack never see a tool.
//   tests      JUnit XML            (pytest, jest/vitest, node --test, go-junit-report, cargo-nextest, ctest, surefire, ...)
//   coverage   LCOV / Cobertura XML (coverage.py, c8/istanbul, node --test, gcovr, llvm-cov, tarpaulin, JaCoCo->cobertura, ...)
//   findings   SARIF 2.1            (eslint, ruff, semgrep, clang, cppcheck, golangci-lint, clippy-sarif, ...)
//   mutation   mutation-testing-report-schema (Stryker, mull, Infection, PIT plugin, mutmut, ...)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { relToRoot } from './util.mjs';

export { parseJunit } from './build.mjs';

/** Map a path written by some tool to a repo-relative posix path (tries each base dir). */
export function resolveReported(cfg, p, bases = []) {
  if (!p) return null;
  let f = p;
  if (/^file:/i.test(f)) { try { f = fileURLToPath(f); } catch { f = f.replace(/^file:\/*/i, ''); } }
  if (path.isAbsolute(f)) return relToRoot(cfg, f);
  for (const b of [...bases, cfg.root]) {
    const abs = path.resolve(b, f);
    if (fs.existsSync(abs)) return relToRoot(cfg, abs);
  }
  return relToRoot(cfg, path.resolve(cfg.root, f));
}

/** LCOV -> Map<reportedPath, Map<line, hits>> */
export function parseLcov(text) {
  const files = new Map();
  let cur = null;
  for (const raw of text.split(/\r?\n/)) {
    const l = raw.trim();
    if (l.startsWith('SF:')) { cur = new Map(); files.set(l.slice(3), cur); }
    else if (l.startsWith('DA:') && cur) {
      const [line, hits] = l.slice(3).split(',');
      cur.set(+line, Math.max(cur.get(+line) || 0, +hits || 0));
    } else if (l === 'end_of_record') cur = null;
  }
  return files;
}

/** Cobertura XML -> { files: Map<reportedPath, Map<line, hits>>, sources: [dirs] } */
export function parseCobertura(xml) {
  const sources = [...xml.matchAll(/<source>([^<]*)<\/source>/g)].map((m) => m[1].trim()).filter(Boolean);
  const files = new Map();
  for (const m of xml.matchAll(/<class\b[^>]*\bfilename="([^"]*)"[^>]*>([\s\S]*?)<\/class>/g)) {
    const cur = files.get(m[1]) || new Map();
    for (const l of m[2].matchAll(/<line\b[^>]*\bnumber="(\d+)"[^>]*\bhits="(\d+)"/g)) cur.set(+l[1], Math.max(cur.get(+l[1]) || 0, +l[2]));
    files.set(m[1], cur);
  }
  return { files, sources };
}

/** SARIF 2.1 -> [{ tool, ruleId, level, message, uri, line, col }] */
export function parseSarif(json) {
  const out = [];
  for (const runRec of json.runs || []) {
    const tool = runRec.tool?.driver?.name || 'sarif';
    const bases = runRec.originalUriBaseIds || {};
    for (const r of runRec.results || []) {
      if (r.suppressions?.length) continue;
      const loc = r.locations?.[0]?.physicalLocation;
      let uri = loc?.artifactLocation?.uri || '';
      const base = loc?.artifactLocation?.uriBaseId && bases[loc.artifactLocation.uriBaseId]?.uri;
      if (base && !/^[a-z]+:/i.test(uri)) uri = base.replace(/\/?$/, '/') + uri;
      out.push({
        tool,
        ruleId: r.ruleId || r.rule?.id || '',
        level: r.level || 'warning',
        message: r.message?.text || r.message?.markdown || '',
        uri: decodeURIComponent(uri),
        line: loc?.region?.startLine || 0,
        col: loc?.region?.startColumn || 0,
      });
    }
  }
  return out;
}

// Compiler diagnostics in the two classic layouts, independent of which compiler printed them:
//   GNU style   file:line[:col]: warning: message [-Wflag]      (gcc, clang, icx / MPI wrappers, gfortran, nvcc, ...)
//   paren style file(line[,col]): warning C4996: message        (MSVC, classic Intel compilers, ...)
const GNU_DIAG = /^(.+?):(\d+):(?:(\d+):)?\s*(?:fatal\s+)?(warning|error)\s*(?:#\d+(?:-[A-Z])?)?:\s*(.*?)(?:\s+\[([^\]]+)\])?\s*$/i;
const PAREN_DIAG = /^(.+?)\((\d+)(?:,(\d+))?\)\s*:\s*(?:fatal\s+)?(warning|error)\s*(#?[A-Za-z]*\d+(?:-[A-Z])?)?\s*:\s*(.*?)\s*$/i;

/** Compiler output -> [{ level, message, uri, line, col, ruleId }] */
export function parseDiagnostics(text) {
  const out = [];
  const seen = new Set();
  for (const raw of text.split(/\r?\n/)) {
    const l = raw.trim();
    let m = GNU_DIAG.exec(l), d = null;
    if (m && !/^\s*In file included from/.test(l)) d = { uri: m[1], line: +m[2], col: +(m[3] || 0), level: m[4].toLowerCase(), message: m[5], ruleId: m[6] || '' };
    else if ((m = PAREN_DIAG.exec(l))) d = { uri: m[1], line: +m[2], col: +(m[3] || 0), level: m[4].toLowerCase(), ruleId: m[5] || '', message: m[6] };
    if (!d) continue;
    const key = `${d.uri}:${d.line}:${d.ruleId}:${d.message}`;
    if (seen.has(key)) continue;   // headers repeat their warnings once per translation unit
    seen.add(key);
    out.push(d);
  }
  return out;
}

const ELEMENTS_STATUS = {
  Killed: 'KILLED', Timeout: 'TIMEOUT', RuntimeError: 'KILLED', Survived: 'SURVIVED', NoCoverage: 'NO_COVERAGE',
  CompileError: 'COMPILE_ERROR', Ignored: 'ACCEPTED', Pending: 'SURVIVED',
};

/** mutation-testing-report-schema -> per-mutant results in the gauntlet's own shape */
export function parseMutationElements(json) {
  const results = [];
  for (const [file, rec] of Object.entries(json.files || {})) {
    const lines = (rec.source || '').split(/\r?\n/);
    for (const m of rec.mutants || []) {
      const line = m.location?.start?.line || 0;
      const status = ELEMENTS_STATUS[m.status] || 'SURVIVED';
      results.push({
        reportedFile: file,
        line,
        col: m.location?.start?.column || 0,
        op: `${m.mutatorName}${m.replacement != null ? ` -> ${m.replacement}` : ''}`,
        status,
        source: lines[line - 1] ?? undefined,
        ...(status === 'ACCEPTED' ? { reason: m.statusReason || 'ignored by the mutation tool configuration' } : {}),
      });
    }
  }
  return results;
}
