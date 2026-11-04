import test from 'node:test';
import assert from 'node:assert/strict';
import {
  escapeHtml, fmtBytes, fmtPercent, historySample, linePath,
  memoryPercent, normalizeMetrics,
} from './lib.js';

test('HTML content is escaped', () => {
  assert.equal(escapeHtml('<img src=x onerror="bad">'), '&lt;img src=x onerror=&quot;bad&quot;&gt;');
});

test('unknown byte and percentage values stay unknown', () => {
  assert.equal(fmtBytes(null), '—');
  assert.equal(fmtBytes(undefined), '—');
  assert.equal(fmtPercent(null), '—');
  assert.equal(memoryPercent({ memory_used_bytes: null, memory_total_bytes: 100 }), null);
  assert.equal(memoryPercent({ memory_used_bytes: 25, memory_total_bytes: 100 }), 25);
});

test('metrics support both wire aliases', () => {
  const raw = { node_id: 'n1', gpu_metrics: [{ index: 0 }], cpu_core_metrics: [{ core_id: 0 }] };
  const metric = normalizeMetrics(raw);
  assert.equal(metric.gpus.length, 1);
  assert.equal(metric.cpu_cores.length, 1);
  assert.equal(normalizeMetrics({}), null);
});

test('line paths ignore missing samples and clamp percentages', () => {
  assert.equal(linePath([]), '');
  assert.equal(linePath([null, undefined]), '');
  assert.ok(linePath([50]).includes(' L '), 'a single sample renders as a visible point');
  const path = linePath([0, 120, null, 50]);
  assert.ok(path.startsWith('M 10 170'));
  assert.ok(path.includes(' C '));
  assert.ok(!path.includes('NaN'));
});

test('history samples retain GPU memory and aggregate averages', () => {
  const raw = historySample({ timestamp_ms: 42, cpu_usage_percent: 25, gpu_metrics: [{ utilization_gpu: 80, memory_used_bytes: 1, memory_total_bytes: 2 }] });
  assert.equal(raw.timestamp, 42);
  assert.equal(raw.gpus[0].mem, 50);
  const aggregate = historySample({ gpu_utilization_percent: 63 });
  assert.equal(aggregate.gpuAverage, 63);
});
