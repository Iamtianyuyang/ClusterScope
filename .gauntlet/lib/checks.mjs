// Gates over the committed stage hand-offs:
//   qa/constraints.json  every explicit constraint of the requirement (servers, directories, tools, ...)
//                        [{ "id": "C1", "text": "在 172.19.133.162 上构建并测试", "verify": "how QA proves it" }]
//   qa/qa-report.json    QA's checks; a check proves a constraint with "constraint": "C1"
// No agent may declare a constraint "out of scope": an unproven constraint fails the gate and needs a human.
import { readJson, pathOf } from './util.mjs';

export function qaGate(cfg) {
  const qa = readJson(pathOf(cfg, 'qa', 'qa-report.json'), null);
  if (!qa) return { pass: false, error: `${cfg.paths.qa}/qa-report.json missing` };
  const checks = qa.checks || [];
  const failed = checks.filter((c) => c.status !== 'pass');
  return { pass: qa.verdict === 'pass' && checks.length > 0 && failed.length === 0, checks: checks.length, failed: failed.length };
}

export function constraintsGate(cfg) {
  const constraints = readJson(pathOf(cfg, 'qa', 'constraints.json'), null);
  if (!constraints) return { pass: false, error: `${cfg.paths.qa}/constraints.json missing: the Specifier must list every explicit constraint of the requirement`, items: [] };
  const checks = readJson(pathOf(cfg, 'qa', 'qa-report.json'), { checks: [] }).checks || [];
  const items = constraints.map((c) => {
    const proofs = checks.filter((k) => k.constraint === c.id || (Array.isArray(k.constraint) && k.constraint.includes(c.id)));
    const state = !proofs.length ? 'unproven' : proofs.every((k) => k.status === 'pass') ? 'met' : 'violated';
    return { ...c, state, proofs: proofs.map((k) => k.id || k.title) };
  });
  return { pass: items.every((i) => i.state === 'met'), total: items.length, met: items.filter((i) => i.state === 'met').length, items };
}
