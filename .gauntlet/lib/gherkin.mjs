// Minimal Gherkin parser -> IR (Uncle Bob's acceptance pipeline: parse -> IR -> generate).
// Supports English and Chinese keywords, Background, Scenario Outline + Examples, Rule,
// doc strings and data tables. Cucumber-expression subset for step patterns.
import fs from 'node:fs';
import path from 'node:path';

const KW = {
  feature: ['Feature', 'Business Need', 'Ability', '功能'],
  rule: ['Rule', '规则'],
  background: ['Background', '背景'],
  outline: ['Scenario Outline', 'Scenario Template', '场景大纲', '剧本大纲'],
  scenario: ['Scenario', 'Example', '场景', '剧本'],
  examples: ['Examples', 'Scenarios', '例子'],
};
const STEP_KW = [
  ['given', ['Given', '假如', '假设', '假定']],
  ['when', ['When', '当']],
  ['then', ['Then', '那么']],
  ['and', ['And', '而且', '并且', '同时']],
  ['but', ['But', '但是']],
  ['and', ['*']],
];

function headerOf(line) {
  for (const [kind, words] of Object.entries(KW)) {
    for (const w of words) {
      const m = line.match(new RegExp(`^${w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*[:：]\\s*(.*)$`));
      if (m) return { kind, name: m[1].trim() };
    }
  }
  return null;
}

function stepOf(line) {
  for (const [type, words] of STEP_KW) {
    for (const w of words) {
      if (line.startsWith(w)) {
        const rest = line.slice(w.length);
        const ascii = /^[A-Za-z*]/.test(w);
        if (ascii && !/^\s/.test(rest)) continue;
        return { type, keyword: w, text: rest.trim() };
      }
    }
  }
  return null;
}

const splitRow = (line) => line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim().replace(/\\\|/g, '|'));

export class GherkinError extends Error {}

export function parseFeature(text, file) {
  const lines = text.replace(/^﻿/, '').split(/\r?\n/);
  const stem = path.basename(file).replace(/\.feature$/i, '').replace(/[^A-Za-z0-9_]+/g, '_') || 'feature';
  const feature = { file, stem, name: '', description: [], tags: [], scenarios: [] };
  let pendingTags = [];
  let background = [];
  let ruleBackground = [];
  let ruleName = '';
  let ctx = null; // {kind:'background'|'scenario'|'outline', steps, ...}
  let lastStep = null;
  let lastType = 'given';
  let examples = null;
  let inDoc = null;
  const err = (n, msg) => new GherkinError(`${file}:${n + 1}: ${msg}`);

  const closeExamples = () => {
    if (examples && ctx?.kind === 'outline') ctx.examples.push(examples);
    examples = null;
  };
  const closeCtx = () => {
    closeExamples();
    if (!ctx) return;
    if (ctx.kind === 'background') {
      if (ruleName) ruleBackground = ctx.steps; else background = ctx.steps;
    } else if (ctx.kind === 'scenario') {
      feature.scenarios.push({ id: `${stem}.L${ctx.line}`, name: ctx.name, rule: ruleName, tags: ctx.tags, line: ctx.line, steps: [...background, ...ruleBackground, ...ctx.steps] });
    } else if (ctx.kind === 'outline') {
      if (!ctx.examples.length) throw err(ctx.line - 1, `Scenario Outline "${ctx.name}" has no Examples table`);
      for (const ex of ctx.examples) {
        if (!ex.header) throw err(ex.line - 1, 'Examples without a header row');
        for (const row of ex.rows) {
          const vals = Object.fromEntries(ex.header.map((h, i) => [h, row.cells[i] ?? '']));
          const sub = (s) => s.replace(/<([^<>]+)>/g, (m, k) => (k in vals ? vals[k] : m));
          const steps = ctx.steps.map((st) => ({
            ...st,
            text: sub(st.text),
            docString: st.docString == null ? null : sub(st.docString),
            table: st.table ? st.table.map((r) => r.map(sub)) : null,
          }));
          feature.scenarios.push({
            id: `${stem}.L${ctx.line}.R${row.line}`,
            name: `${sub(ctx.name)} [${ex.header.map((h) => `${h}=${vals[h]}`).join(', ')}]`,
            rule: ruleName,
            tags: [...ctx.tags, ...ex.tags],
            line: row.line,
            steps: [...background, ...ruleBackground, ...steps],
          });
        }
      }
    }
    ctx = null;
    lastStep = null;
  };

  for (let n = 0; n < lines.length; n++) {
    const raw = lines[n];
    const line = raw.trim();
    if (inDoc) {
      if (line.startsWith(inDoc.fence)) {
        const indent = inDoc.indent;
        lastStep.docString = inDoc.buf.map((l) => l.slice(Math.min(indent, l.length - l.trimStart().length))).join('\n');
        inDoc = null;
      } else inDoc.buf.push(raw);
      continue;
    }
    if (!line || line.startsWith('#')) continue;
    if (line.startsWith('@')) { pendingTags.push(...line.split(/\s+/).filter((t) => t.startsWith('@'))); continue; }
    if (line.startsWith('"""') || line.startsWith('```')) {
      if (!lastStep) throw err(n, 'doc string without a step');
      inDoc = { fence: line.slice(0, 3), indent: raw.length - raw.trimStart().length, buf: [] };
      continue;
    }
    if (line.startsWith('|')) {
      const cells = splitRow(line);
      if (examples) {
        if (!examples.header) examples.header = cells; else examples.rows.push({ cells, line: n + 1 });
      } else if (lastStep) {
        (lastStep.table ||= []).push(cells);
      } else throw err(n, 'table row without a step or Examples');
      continue;
    }
    const h = headerOf(line);
    if (h) {
      const tags = pendingTags; pendingTags = [];
      if (h.kind === 'feature') { feature.name = h.name; feature.tags = tags; continue; }
      if (h.kind === 'rule') { closeCtx(); ruleName = h.name; ruleBackground = []; continue; }
      if (h.kind === 'examples') {
        if (ctx?.kind !== 'outline') throw err(n, 'Examples outside a Scenario Outline');
        closeExamples();
        examples = { name: h.name, tags, header: null, rows: [], line: n + 1 };
        continue;
      }
      closeCtx();
      ctx = { kind: h.kind, name: h.name, tags: [...feature.tags, ...tags], line: n + 1, steps: [], examples: [] };
      lastType = 'given';
      continue;
    }
    const st = stepOf(line);
    if (st) {
      if (!ctx) throw err(n, `step outside a Scenario/Background: ${line}`);
      if (examples) throw err(n, 'step after Examples');
      const type = st.type === 'and' || st.type === 'but' ? lastType : st.type;
      lastType = type;
      lastStep = { keyword: st.keyword, type, text: st.text, line: n + 1, docString: null, table: null };
      ctx.steps.push(lastStep);
      continue;
    }
    if (!ctx && feature.name) { feature.description.push(line); continue; }
    if (ctx && !ctx.steps.length) continue; // free-form scenario description
    throw err(n, `unrecognised line: ${line}`);
  }
  if (inDoc) throw err(lines.length - 1, 'unterminated doc string');
  closeCtx();
  if (!feature.name) throw err(0, 'missing "Feature:" line');
  return feature;
}

export function parseFeatureDir(dir) {
  if (!fs.existsSync(dir)) return [];
  const files = fs.readdirSync(dir, { recursive: true }).filter((f) => /\.feature$/i.test(f)).sort();
  const features = files.map((f) => parseFeature(fs.readFileSync(path.join(dir, f), 'utf8'), path.join(dir, f)));
  const seen = new Map();
  for (const ft of features) {
    for (const sc of ft.scenarios) {
      if (seen.has(sc.id)) throw new GherkinError(`duplicate scenario id ${sc.id} (${seen.get(sc.id)} and ${ft.file}); rename one .feature file`);
      seen.set(sc.id, ft.file);
    }
  }
  return features;
}

/** Cucumber-expression subset -> ECMAScript regex source. Must match gauntlet_acceptance.hpp. */
export function expressionToRegex(expr) {
  if (expr.startsWith('^') && expr.endsWith('$')) return expr;
  let out = '';
  for (let i = 0; i < expr.length; i++) {
    const c = expr[i];
    if (c === '{') {
      const j = expr.indexOf('}', i);
      const name = expr.slice(i + 1, j);
      const map = { int: '(-?\\d+)', float: '(-?\\d+(?:\\.\\d+)?)', word: '(\\S+)', string: '"([^"]*)"', '': '(.*)' };
      if (j < 0 || !(name in map)) throw new GherkinError(`unknown parameter type {${name}} in "${expr}"`);
      out += map[name];
      i = j;
    } else if (c === '(') {
      const j = expr.indexOf(')', i);
      if (j < 0) throw new GherkinError(`unbalanced ( in "${expr}"`);
      out += `(?:${expr.slice(i + 1, j).replace(/[.*+?^$|[\]\\]/g, '\\$&')})?`;
      i = j;
    } else if (c === '\\' && i + 1 < expr.length) {
      out += expr[++i].replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');
    } else out += c.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }
  return `^${out}$`;
}

/** Extract GT_STEP("...") patterns from C++ step-definition sources. */
export function scanStepDefinitions(dir) {
  const defs = [];
  if (!fs.existsSync(dir)) return defs;
  const files = fs.readdirSync(dir, { recursive: true }).filter((f) => /\.(cc|cpp|cxx)$/i.test(f));
  for (const f of files) {
    const p = path.join(dir, f);
    const src = fs.readFileSync(p, 'utf8');
    const re = /GT_STEP\s*\(\s*(?:R"([^(]*)\(([\s\S]*?)\)\1"|"((?:[^"\\]|\\.)*)")/g;
    let m;
    while ((m = re.exec(src))) {
      const pattern = m[2] !== undefined ? m[2] : JSON.parse(`"${m[3].replace(/\\'/g, "'")}"`);
      const line = src.slice(0, m.index).split('\n').length;
      defs.push({ pattern, regex: new RegExp(expressionToRegex(pattern)), file: p, line });
    }
  }
  return defs;
}

export function suggestPattern(text) {
  return text.replace(/"[^"]*"/g, '{string}').replace(/(^|\s)-?\d+\.\d+(?=\s|$)/g, '$1{float}').replace(/(^|\s)-?\d+(?=\s|$)/g, '$1{int}');
}

/** Bind each step to its definition: returns undefined/ambiguous lists. */
export function bindSteps(features, defs) {
  const undefinedSteps = new Map();
  const ambiguous = [];
  let total = 0;
  for (const ft of features) for (const sc of ft.scenarios) for (const st of sc.steps) {
    total++;
    const hits = defs.filter((d) => d.regex.test(st.text));
    if (hits.length === 0) {
      const key = st.text;
      if (!undefinedSteps.has(key)) undefinedSteps.set(key, { text: st.text, at: `${ft.file}:${st.line}`, suggestion: `GT_STEP("${suggestPattern(st.text).replace(/"/g, '\\"')}")` });
    } else if (hits.length > 1) ambiguous.push({ text: st.text, at: `${ft.file}:${st.line}`, matches: hits.map((h) => `${h.file}:${h.line} ${h.pattern}`) });
  }
  return { total, undefinedSteps: [...undefinedSteps.values()], ambiguous };
}
