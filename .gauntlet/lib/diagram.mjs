// Architecture diagrams with Archify (https://github.com/tt-a1i/archify, MIT): the agent writes a typed
// JSON candidate with source evidence (file + line ranges); Archify validates it against the repository
// at the pinned commit and renders a standalone HTML/SVG diagram. The evidence pack embeds the result.
//   node .gauntlet/gauntlet.mjs diagram docs/architecture/overview.architecture.json [--type architecture]
import fs from 'node:fs';
import path from 'node:path';
import { run, ensureDir, writeJson, relToRoot } from './util.mjs';

export function archifyDir(cfg) {
  return cfg.tools.archify.dir ? cfg.abs(cfg.tools.archify.dir) : cfg.out('tools', 'archify');
}

/** Clone the pinned Archify release once (into the gauntlet output dir, i.e. inside the repo's tmp area). */
export function ensureArchify(cfg) {
  const dir = archifyDir(cfg);
  const bin = path.join(dir, 'archify', 'bin', 'archify.mjs');
  if (fs.existsSync(bin)) return bin;
  ensureDir(path.dirname(dir));
  const { repo, ref } = cfg.tools.archify;
  const r = run('git', ['clone', '--depth', '1', '--branch', ref, repo, dir], { allowFail: true });
  if (r.code !== 0 || !fs.existsSync(bin)) {
    const why = (r.stderr || '').trim().split(/\r?\n/).pop() || `${bin} 不存在`;
    throw new Error(`拿不到 Archify（${repo} @ ${ref}）：${why}
  离线环境：把 Archify 放到本机，在 gauntlet.local.json 里设 "tools": { "archify": { "dir": "<路径>" } }。
  拿不到也可以不画：证据包会用自动生成的依赖草图代替，并列为"需要你确认"。`);
  }
  return bin;
}

export function runDiagram(cfg, candidate, { type, name } = {}) {
  const bin = ensureArchify(cfg);
  const cand = path.resolve(cfg.root, candidate);
  const kind = type || (/\.(architecture|workflow|sequence|dataflow|lifecycle)\.json$/.exec(cand) || [, 'architecture'])[1];
  const slug = name || path.basename(cand).replace(/\.(architecture|workflow|sequence|dataflow|lifecycle)?\.?json$/, '');
  const outDir = ensureDir(cfg.out('evidence', 'diagrams'));
  const html = path.join(outDir, `${slug}.html`);
  // Source evidence paths are relative to the git repository root (the project may be a subdirectory).
  const top = run('git', ['rev-parse', '--show-toplevel'], { cwd: cfg.root, quiet: true, allowFail: true }).stdout.trim() || cfg.root;
  const rel = (p) => path.relative(top, p).split(path.sep).join('/');
  // deterministic gates: schema/semantic validation + repository evidence, then strict provenance check
  const deliver = run(process.execPath, [bin, 'deliver', kind, rel(cand), rel(html), '--repo-root', '.', '--quality', 'showcase', '--json'], { cwd: top, allowFail: true, quiet: true });
  const receipt = { candidate: relToRoot(cfg, cand), type: kind, html: relToRoot(cfg, html), archify: `${cfg.tools.archify.repo}@${cfg.tools.archify.ref}`, at: new Date().toISOString() };
  if (deliver.code !== 0) {
    receipt.status = 'fail';
    receipt.stage = 'deliver';
    receipt.output = (deliver.stdout + deliver.stderr).slice(-4000);
  } else {
    const check = run(process.execPath, [bin, 'check', rel(html), '--require-provenance', '--json'], { cwd: top, allowFail: true, quiet: true });
    if (check.code !== 0) {
      receipt.status = 'fail';
      receipt.stage = 'check';
      receipt.output = (check.stdout + check.stderr).slice(-4000);
    } else {
      // real-browser layout check when Chrome/Chromium exists; exit 2 = skipped (no browser on this host)
      const bc = run(process.execPath, [bin, 'browser-check', rel(html), '--require-provenance', '--summary'], { cwd: top, allowFail: true, quiet: true });
      receipt.browserCheck = bc.code === 0 ? 'pass' : bc.code === 2 ? 'skipped (no browser)' : 'fail';
      receipt.status = bc.code === 0 || bc.code === 2 ? 'pass' : 'fail';
      if (bc.code !== 0 && bc.code !== 2) { receipt.stage = 'browser-check'; receipt.output = (bc.stdout + bc.stderr).slice(-4000); }
    }
  }
  writeJson(path.join(outDir, `${slug}.receipt.json`), receipt);
  return receipt;
}

export function listDiagrams(cfg) {
  const dir = cfg.out('evidence', 'diagrams');
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir).filter((f) => f.endsWith('.receipt.json')).map((f) => {
    const r = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    const htmlAbs = cfg.abs(r.html);
    return { ...r, htmlAbs: fs.existsSync(htmlAbs) ? htmlAbs : null };
  });
}
