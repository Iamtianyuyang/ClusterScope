// Report components. Each returns an HTML string that only uses theme variables / classes from theme.mjs.
// Vocabulary borrowed from answer-me-with-html (panel, kv, limits, callout, timeline, status badges).

export const esc = (s) => String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const pct = (x, digits = 1) => `${(x * 100).toFixed(digits).replace(/\.0+$/, '')}%`;

/** Lettered panel with a title bar. opts: { id, title, meta, span, tone:'err'|'warn', tools } */
export function panel({ id, title, meta = '', span = 1, tone = '', tools = '' }, body) {
  const style = span > 1 ? ` style="grid-column:span ${span}"` : '';
  return `<section class="g-panel${tone ? ` is-${tone}` : ''}" id="panel-${esc(id)}"${style}>
<header class="g-panel-head"><span class="g-panel-id">${esc(id)}</span><h2>${esc(title)}${tools}</h2>${meta ? `<span class="g-panel-meta">${esc(meta)}</span>` : ''}</header>
<div class="g-panel-body">${body}</div></section>`;
}

export const status = (kind, text = '') => {
  const icon = { ok: '✓', no: '✗', warn: '!', na: '–' }[kind] || '–';
  return `<span class="g-status g-status--${kind}"><span class="g-status-icon">${icon}</span>${esc(text)}</span>`;
};

export const callout = (kind, title, bodyHtml = '') =>
  `<div class="g-callout g-callout--${kind}"><div class="g-callout-title">${esc(title)}</div>${bodyHtml}</div>`;

/** cells: [{ k, v, small?, tone?: 'ok'|'err', wide? }] */
export function kv(cells, cols = 3) {
  return `<dl class="g-kv" style="--kv-cols:${cols}">${cells.map((c) =>
    `<div class="g-kv-cell${c.wide ? ' g-kv-cell--wide' : ''}"><dt>${esc(c.k)}</dt><dd${c.tone ? ` class="is-${c.tone}"` : ''}>${esc(c.v)}${c.small ? `<small>${esc(c.small)}</small>` : ''}</dd></div>`).join('')}</dl>`;
}

/**
 * Value vs threshold bar.
 * { label, value, limit, kind: 'max'|'min', scale, fmt, note }
 *   kind 'max': value must be <= limit;  kind 'min': value must be >= limit
 */
export function limits(items) {
  return items.map((it) => {
    const fmt = it.fmt || ((x) => String(x));
    // info: a measurement without a threshold (no pass/fail, no marker)
    const scale = it.scale ?? Math.max(it.value, it.limit ?? 0, 1) * (it.kind === 'max' ? 1.25 : 1);
    const bad = !it.info && (it.kind === 'max' ? it.value > it.limit : it.value < it.limit);
    const w = Math.min(100, (it.value / scale) * 100);
    const m = it.info ? null : Math.min(100, (it.limit / scale) * 100);
    const rule = it.info ? '' : it.kind === 'max' ? `上限 ${fmt(it.limit)}` : `下限 ${fmt(it.limit)}`;
    return `<div class="g-lim${bad ? ' is-bad' : ''}"><div class="g-lim-head"><span>${it.info ? status('na') : bad ? status('no') : status('ok')}${esc(it.label)}</span><span class="g-lim-val">${esc(fmt(it.value))}${rule ? ` · ${esc(rule)}` : ''}</span></div>
<div class="g-lim-track" role="img" aria-label="${esc(it.label)} ${esc(fmt(it.value))}${rule ? `, ${esc(rule)}` : ''}"><div class="g-lim-fill" style="width:${w.toFixed(1)}%"></div>${m === null ? '' : `<div class="g-lim-mark" style="left:${m.toFixed(1)}%"></div>`}</div>
${it.note ? `<div class="g-lim-note">${esc(it.note)}</div>` : ''}</div>`;
  }).join('');
}

/** stages: [{ title, role, state: 'ok'|'bad'|'na', text }] */
export function timeline(stages) {
  return `<ol class="g-tl" style="--n:${stages.length}">${stages.map((s, i) =>
    `<li class="is-${s.state}"><span class="g-tl-n">${i + 1}</span><span class="g-tl-dot">${s.state === 'ok' ? '✓' : s.state === 'bad' ? '✗' : '–'}</span><span class="g-tl-title">${esc(s.title)}</span><span class="g-tl-role">${esc(s.role)}</span><span class="g-tl-text">${esc(s.text)}</span></li>`).join('')}</ol>`;
}

/** columns: [{ h, cell: (row) => html, cls }] — cells return trusted HTML; escape inside. */
export function table(columns, rows, { maxHeight } = {}) {
  return `<div class="g-table-wrap"${maxHeight ? ` style="--max-h:${maxHeight}px"` : ''}><table class="g-table"><thead><tr>${columns.map((c) => `<th>${esc(c.h)}</th>`).join('')}</tr></thead>
<tbody>${rows.map((r) => `<tr>${columns.map((c) => `<td${c.cls ? ` class="${c.cls}"` : ''}>${c.cell(r)}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
}

// ---------- tiny markdown renderer (headings, lists, code, images, links, bold, tables, quotes) ----------
export function markdown(md, mediaResolver = (s) => s) {
  const lines = md.replace(/\r/g, '').split('\n');
  const out = [];
  let i = 0;
  const inline = (s) => esc(s)
    .replace(/!\[([^\]]*)\]\(([^)\s]+)\)/g, (m, alt, src) => `<img alt="${alt}" src="${esc(mediaResolver(src.replace(/&amp;/g, '&')))}">`)
    .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<a href="$2">$1</a>')
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[^*])\*([^*]+)\*/g, '$1<em>$2</em>');
  while (i < lines.length) {
    const l = lines[i];
    if (/^```/.test(l)) {
      const buf = [];
      for (i++; i < lines.length && !/^```/.test(lines[i]); i++) buf.push(lines[i]);
      i++;
      out.push(`<pre class="g-code"><code>${esc(buf.join('\n'))}</code></pre>`);
      continue;
    }
    const h = l.match(/^(#{1,4})\s+(.*)$/);
    if (h) { out.push(`<h${h[1].length + 2}>${inline(h[2])}</h${h[1].length + 2}>`); i++; continue; }
    if (/^\s*\|/.test(l)) {
      const rows = [];
      for (; i < lines.length && /^\s*\|/.test(lines[i]); i++) rows.push(lines[i].trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim()));
      const body = rows.filter((r) => !r.every((c) => /^:?-+:?$/.test(c)));
      out.push(`<div class="g-table-wrap"><table class="g-table"><thead><tr>${body[0].map((c) => `<th>${inline(c)}</th>`).join('')}</tr></thead><tbody>${body.slice(1).map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`);
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(l)) {
      const ordered = /^\s*\d+\./.test(l);
      const items = [];
      for (; i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i]); i++) items.push(lines[i].replace(/^\s*([-*]|\d+\.)\s+/, ''));
      out.push(`<${ordered ? 'ol' : 'ul'}>${items.map((x) => `<li>${inline(x)}</li>`).join('')}</${ordered ? 'ol' : 'ul'}>`);
      continue;
    }
    if (/^>\s?/.test(l)) {
      const buf = [];
      for (; i < lines.length && /^>\s?/.test(lines[i]); i++) buf.push(lines[i].replace(/^>\s?/, ''));
      out.push(`<blockquote>${inline(buf.join(' '))}</blockquote>`);
      continue;
    }
    if (!l.trim()) { i++; continue; }
    const buf = [];
    for (; i < lines.length && lines[i].trim() && !/^(```|#{1,4}\s|\s*\||\s*([-*]|\d+\.)\s|>)/.test(lines[i]); i++) buf.push(lines[i]);
    out.push(`<p>${inline(buf.join(' '))}</p>`);
  }
  return `<div class="g-md">${out.join('\n')}</div>`;
}

// ---------- charts (inline SVG, theme-aware, no CDN) ----------

/** Horizontal bars. items: [{ name, value }] */
export function barChart(items, { threshold = null, unit = '', width = 560, labelWidth = 200, max } = {}) {
  const rowH = 24, top = Math.max(max ?? 0, threshold ?? 0, ...items.map((x) => x.value), 1);
  const plotW = width - labelWidth - 56;
  const h = items.length * rowH + (threshold != null ? 24 : 6);
  const X = (v) => labelWidth + (v / top) * plotW;
  return `<svg class="g-chart" viewBox="0 0 ${width} ${h}" role="img" aria-label="条形图">
${items.map((it, i) => {
    const y = i * rowH + 4;
    const bad = threshold != null && it.value > threshold;
    const name = it.name.length > 30 ? `…${it.name.slice(-29)}` : it.name;
    return `<text x="${labelWidth - 8}" y="${y + 13}" text-anchor="end" class="t-lbl"><title>${esc(it.name)}</title>${esc(name)}</text><rect x="${labelWidth}" y="${y + 3}" width="${Math.max(2, X(it.value) - labelWidth).toFixed(1)}" height="${rowH - 10}" rx="2" class="${bad ? 'c-bad' : 'c-ok'}"/><text x="${(X(it.value) + 6).toFixed(1)}" y="${y + 13}" class="t-val">${esc(it.value)}${unit}</text>`;
  }).join('\n')}
${threshold != null ? `<line x1="${X(threshold)}" x2="${X(threshold)}" y1="0" y2="${h - 18}" class="c-thr"/><text x="${X(threshold)}" y="${h - 5}" text-anchor="middle" class="t-thr">阈值 ${threshold}</text>` : ''}
</svg>`;
}

/** Complexity vs coverage scatter with the CRAP == threshold boundary and a shaded danger zone. */
export function crapScatter(fns, th, width = 640, height = 300) {
  const pad = { l: 44, r: 16, t: 14, b: 34 };
  const maxCC = Math.max(th.complexityMax, ...fns.map((f) => f.complexity)) + 1;
  const X = (cov) => pad.l + cov * (width - pad.l - pad.r);
  const Y = (cc) => height - pad.b - (cc / maxCC) * (height - pad.t - pad.b);
  const boundary = [];
  for (let c = 0; c <= 1.0001; c += 0.02) {
    let lo = 1, hi = maxCC;
    for (let k = 0; k < 40; k++) { const mid = (lo + hi) / 2; if (mid * mid * Math.pow(1 - c, 3) + mid > th.crapMax) hi = mid; else lo = mid; }
    boundary.push([X(c), Y(Math.min(lo, maxCC))]);
  }
  const line = boundary.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  const zone = `${X(0)},${Y(maxCC)} ${line} ${X(1)},${Y(maxCC)}`;
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  const ccTicks = Array.from({ length: 5 }, (_, i) => Math.round((maxCC * i) / 4));
  return `<svg class="g-chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="复杂度与覆盖率散点图">
<polygon points="${zone}" class="c-zone"/>
${ticks.map((t) => `<line x1="${X(t)}" x2="${X(t)}" y1="${pad.t}" y2="${height - pad.b}" class="c-grid"/><text x="${X(t)}" y="${height - pad.b + 14}" text-anchor="middle" class="t-lbl">${t * 100}%</text>`).join('')}
${ccTicks.map((c) => `<line x1="${pad.l}" x2="${width - pad.r}" y1="${Y(c)}" y2="${Y(c)}" class="c-grid"/><text x="${pad.l - 6}" y="${Y(c) + 4}" text-anchor="end" class="t-lbl">${c}</text>`).join('')}
<rect x="${pad.l}" y="${pad.t}" width="${width - pad.l - pad.r}" height="${height - pad.t - pad.b}" class="c-frame"/>
<polyline points="${line}" class="c-thr"/>
<text x="${pad.l + 8}" y="${pad.t + 16}" class="t-thr">阴影区 = CRAP &gt; ${th.crapMax}</text>
${fns.map((f, i) => `<circle cx="${(X(f.coverage) - (i % 3) * 3).toFixed(1)}" cy="${(Y(f.complexity) + (i % 2) * 2).toFixed(1)}" r="5" class="${f.violations.length ? 'c-bad' : 'c-ok'}" fill-opacity=".85"><title>${esc(f.name)}
${esc(f.file)}:${f.line}
CC=${f.complexity} 覆盖率=${pct(f.coverage)} CRAP=${f.crap}</title></circle>`).join('\n')}
<text x="${(width + pad.l) / 2}" y="${height - 4}" text-anchor="middle" class="t-lbl">覆盖率 →</text>
<text x="12" y="${(height - pad.b) / 2}" transform="rotate(-90 12 ${(height - pad.b) / 2})" text-anchor="middle" class="t-lbl">圈复杂度 →</text>
</svg>`;
}

/** Layered module dependency graph (left = depends on right). Forbidden edges are red and dashed. */
export function archGraph(arch) {
  const mods = arch.modules.map((m) => m.name);
  const deps = new Map(mods.map((m) => [m, arch.edges.filter((e) => e.from === m).map((e) => e.to)]));
  const level = new Map();
  const depth = (m, seen = new Set()) => {
    if (level.has(m)) return level.get(m);
    if (seen.has(m)) return 0;
    seen.add(m);
    const v = Math.max(-1, ...(deps.get(m) || []).map((d) => depth(d, seen))) + 1;
    level.set(m, v);
    return v;
  };
  mods.forEach((m) => depth(m));
  const maxL = Math.max(0, ...level.values());
  const cols = Array.from({ length: maxL + 1 }, () => []);
  mods.forEach((m) => cols[maxL - level.get(m)].push(m));
  // Size the viewBox to the graph so text keeps a readable size when the panel scales the SVG.
  const boxW = 112, boxH = 46, gapX = 96, gapY = 22, padX = 8;
  const width = padX * 2 + cols.length * boxW + (cols.length - 1) * gapX;
  const colX = (c) => padX + c * (boxW + gapX);
  const height = Math.max(...cols.map((c) => c.length)) * (boxH + gapY) + gapY;
  const pos = new Map();
  cols.forEach((col, c) => {
    const total = col.length * boxH + (col.length - 1) * gapY;
    col.forEach((m, r) => pos.set(m, { x: colX(c), y: (height - total) / 2 + r * (boxH + gapY) }));
  });
  const files = new Map(arch.modules.map((m) => [m.name, m.files]));
  const edges = arch.edges.map((e) => {
    const a = pos.get(e.from), b = pos.get(e.to);
    const forward = a.x < b.x;
    const x1 = forward ? a.x + boxW : a.x, x2 = forward ? b.x : b.x + boxW;
    const y1 = a.y + boxH / 2, y2 = b.y + boxH / 2;
    const mx = (x1 + x2) / 2;
    const d = a.x === b.x ? `M${a.x + boxW} ${y1} C${a.x + boxW + 50} ${y1} ${a.x + boxW + 50} ${y2} ${a.x + boxW} ${y2}` : `M${x1} ${y1} C${mx} ${y1} ${mx} ${y2} ${x2} ${y2}`;
    const lbl = `${e.refs.length} 处 include${e.allowed ? '' : ' · 违规'}`;
    return `<path d="${d}" class="a-edge${e.allowed ? '' : ' is-bad'}" marker-end="url(#${e.allowed ? 'g-arrow' : 'g-arrow-bad'})"/><text x="${mx}" y="${(y1 + y2) / 2 - 6}" text-anchor="middle" class="a-elbl${e.allowed ? '' : ' is-bad'}">${esc(lbl)}</text>`;
  }).join('\n');
  return `<svg class="g-chart" viewBox="0 0 ${width} ${height}" style="max-width:${Math.round(width * 1.4)}px;margin:0 auto" role="img" aria-label="模块依赖图">
<defs><marker id="g-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L10 5L0 10z" style="fill:var(--ink-2)"/></marker>
<marker id="g-arrow-bad" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L10 5L0 10z" style="fill:var(--err)"/></marker></defs>
${edges}
${mods.map((m) => { const p = pos.get(m); return `<rect x="${p.x}" y="${p.y}" width="${boxW}" height="${boxH}" class="a-box"/><text x="${p.x + boxW / 2}" y="${p.y + 20}" text-anchor="middle" class="a-name">${esc(m)}</text><text x="${p.x + boxW / 2}" y="${p.y + 36}" text-anchor="middle" class="a-sub">${files.get(m)} 个文件</text>`; }).join('\n')}
</svg>`;
}
