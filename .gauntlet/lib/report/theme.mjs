// Report themes = sets of CSS variables (idea from answer-me-with-html). Components only reference
// variables, so switching html[data-theme] / html[data-mode] restyles everything.
// Palette rules (from html-anything): one accent + neutrals, no pure #000 / #fff, ok/warn/err only for status.

const SANS = '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", "Noto Sans SC", Roboto, "Helvetica Neue", Arial, sans-serif';
const MONO = 'ui-monospace, SFMono-Regular, "JetBrains Mono", "Cascadia Mono", Menlo, Consolas, "Liberation Mono", monospace';

export const THEMES = {
  blueprint: {
    label: '图纸',
    common: { '--radius': '0px', '--shadow': 'none', '--bw': '1.5px' },
    light: {
      '--bg': '#f5f5f1', '--paper': '#fdfdfb', '--ink': '#16181d', '--ink-2': '#4b5260', '--ink-3': '#8b929e',
      '--line': '#1d2026', '--line-2': '#d6dae1', '--fill': '#f2f4f7',
      '--accent': '#1d5fbf', '--accent-bg': '#e4ecf8',
      '--ok': '#1f7a4d', '--ok-bg': '#e3f3ea', '--err': '#c62828', '--err-bg': '#fbeaea',
      '--warn': '#a8620a', '--warn-bg': '#fdf3e2', '--head-bg': '#16181d', '--head-fg': '#fdfdfb',
    },
    dark: {
      '--bg': '#081322', '--paper': '#0d1c31', '--ink': '#e6edf7', '--ink-2': '#a9b8cc', '--ink-3': '#6b7f99',
      '--line': '#c9d6e8', '--line-2': '#23385a', '--fill': '#12253f',
      '--accent': '#6ea8ff', '--accent-bg': '#16305a',
      '--ok': '#5fd39a', '--ok-bg': '#0f3326', '--err': '#ff7070', '--err-bg': '#3b1620',
      '--warn': '#f0b14a', '--warn-bg': '#3a2a10', '--head-bg': '#e6edf7', '--head-fg': '#081322',
    },
  },
  card: {
    label: '卡片',
    common: { '--radius': '12px', '--shadow': '0 1px 2px rgba(10,10,10,.05), 0 4px 16px rgba(10,10,10,.04)', '--bw': '1px' },
    light: {
      '--bg': '#fafafa', '--paper': '#fefefe', '--ink': '#0a0a0a', '--ink-2': '#6b6b73', '--ink-3': '#a1a1aa',
      '--line': '#e5e7eb', '--line-2': '#eeeef0', '--fill': '#f4f4f5',
      '--accent': '#2563eb', '--accent-bg': '#eff4ff',
      '--ok': '#16a34a', '--ok-bg': '#effbf3', '--err': '#dc2626', '--err-bg': '#fef2f2',
      '--warn': '#d97706', '--warn-bg': '#fffbeb', '--head-bg': '#18181b', '--head-fg': '#fafafa',
    },
    dark: {
      '--bg': '#0a0a0a', '--paper': '#121215', '--ink': '#fafafa', '--ink-2': '#a1a1aa', '--ink-3': '#71717a',
      '--line': '#262626', '--line-2': '#1c1c1f', '--fill': '#18181b',
      '--accent': '#60a5fa', '--accent-bg': '#172554',
      '--ok': '#4ade80', '--ok-bg': '#052e16', '--err': '#f87171', '--err-bg': '#450a0a',
      '--warn': '#fbbf24', '--warn-bg': '#451a03', '--head-bg': '#fafafa', '--head-fg': '#18181b',
    },
  },
};

const block = (sel, vars) => `${sel}{${Object.entries(vars).map(([k, v]) => `${k}:${v}`).join(';')}}`;

export function themeCss() {
  const shared = { '--font-sans': SANS, '--font-mono': MONO };
  return Object.entries(THEMES).map(([name, t]) => {
    const sel = `html[data-theme="${name}"]`;
    return [
      block(`${sel},${sel}[data-mode="light"]`, { ...shared, ...t.common, ...t.light }),
      block(`${sel}[data-mode="dark"]`, t.dark),
      `@media (prefers-color-scheme:dark){${block(`${sel}[data-mode="auto"]`, t.dark)}}`,
    ].join('\n');
  }).join('\n');
}

export const BASE_CSS = `
*,*::before,*::after{box-sizing:border-box}html,body{margin:0;padding:0}
body{background:var(--bg);color:var(--ink);font-family:var(--font-sans);font-size:14px;line-height:1.6;-webkit-font-smoothing:antialiased}
code,pre,kbd{font-family:var(--font-mono)}
a{color:var(--accent)}

/* toolbar */
.g-toolbar{position:fixed;top:12px;right:12px;z-index:10;display:flex;gap:6px}
.g-btn{font:12px/1 var(--font-sans);color:var(--ink);background:var(--paper);border:1px solid var(--line-2);border-radius:var(--radius);padding:7px 10px;cursor:pointer}
.g-btn:hover{border-color:var(--ink-3)}.g-btn:focus-visible{outline:2px solid var(--accent);outline-offset:2px}

/* page head */
.g-sheet{max-width:1680px;margin:0 auto;padding:32px 28px 48px}
.g-head{margin:0 0 24px;padding-right:300px}
.g-eyebrow{font:12px/1.4 var(--font-mono);color:var(--ink-3);letter-spacing:.04em}
.g-head h1{margin:4px 0 0;font-size:26px;line-height:1.25;letter-spacing:-.01em}
.g-verdict{display:inline-flex;align-items:center;gap:6px;margin-top:10px;padding:4px 10px;font-weight:600;border:var(--bw) solid currentColor;border-radius:var(--radius)}
.g-verdict.is-ok{color:var(--ok);background:var(--ok-bg)}.g-verdict.is-err{color:var(--err);background:var(--err-bg)}.g-verdict.is-warn{color:var(--warn);background:var(--warn-bg)}
.g-head-meta{display:flex;flex-wrap:wrap;gap:6px 18px;margin-top:12px;font-size:12px;color:var(--ink-2)}
.g-head-meta b{font-family:var(--font-mono);font-weight:400;color:var(--ink-3);margin-right:6px}
.g-intro{margin-top:12px;max-width:72ch;color:var(--ink)}

/* blueprint frame with rulers (decoration only) */
.g-frame{position:relative}.g-ruler{display:none}
html[data-theme="blueprint"] .g-frame{border:1px solid var(--line);padding:30px}
html[data-theme="blueprint"] .g-frame::before{content:"";position:absolute;inset:18px;border:1px solid var(--line);pointer-events:none}
html[data-theme="blueprint"] .g-ruler{display:flex;position:absolute;font:10px/1 var(--font-mono);color:var(--ink-3)}
.g-ruler span{flex:1;display:flex;align-items:center;justify-content:center}
.g-ruler--top,.g-ruler--bottom{left:18px;right:18px;height:18px}.g-ruler--top{top:0}.g-ruler--bottom{bottom:0}
.g-ruler--left,.g-ruler--right{top:18px;bottom:18px;width:18px;flex-direction:column}.g-ruler--left{left:0}.g-ruler--right{right:0}
.g-ruler--top span+span,.g-ruler--bottom span+span{border-left:1px solid var(--line)}
.g-ruler--left span+span,.g-ruler--right span+span{border-top:1px solid var(--line)}

/* grid + panels */
.g-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:20px;align-items:start}
.g-panel{background:var(--paper);border:var(--bw) solid var(--line);border-radius:var(--radius);box-shadow:var(--shadow);min-width:0;overflow:hidden}
.g-panel-head{display:flex;align-items:stretch;border-bottom:var(--bw) solid var(--line);min-height:36px}
.g-panel-id{display:flex;align-items:center;justify-content:center;min-width:36px;padding:0 8px;background:var(--head-bg);color:var(--head-fg);font-weight:600}
html[data-theme="card"] .g-panel-id{border-radius:6px;min-width:24px;height:24px;margin:10px 0 10px 14px;font-size:12px}
html[data-theme="card"] .g-panel-head{border-bottom-color:var(--line-2)}
.g-panel-head h2{margin:0;padding:8px 12px;font-size:15px;font-weight:600;flex:1;display:flex;align-items:center;gap:8px}
.g-panel-meta{align-self:center;padding:0 12px;font:11px/1.3 var(--font-mono);color:var(--ink-2);text-align:right}
.g-panel-body{padding:14px 16px 16px}.g-panel-body>*+*{margin-top:14px}
.g-panel.is-err{border-color:var(--err)}.g-panel.is-err .g-panel-id{background:var(--err);color:var(--paper)}
.g-panel.is-warn .g-panel-id{background:var(--warn);color:var(--paper)}
@media (max-width:960px){.g-grid{grid-template-columns:minmax(0,1fr)}.g-panel{grid-column:auto!important}.g-head{padding-right:0;padding-top:36px}
  html[data-theme="blueprint"] .g-frame{padding:0;border:0}html[data-theme="blueprint"] .g-frame::before,.g-ruler{display:none!important}.g-sheet{padding:20px 16px 40px}}

/* text */
.g-md>:first-child{margin-top:0}.g-md>:last-child{margin-bottom:0}
.g-md p{margin:0 0 8px;max-width:72ch}.g-md ul,.g-md ol{margin:0 0 8px;padding-left:20px}.g-md li+li{margin-top:4px}
.g-md h3,.g-md h4,.g-md h5,.g-md h6{margin:16px 0 6px;font-size:14px}
.g-md blockquote{margin:0 0 8px;padding:2px 12px;border-left:3px solid var(--line-2);color:var(--ink-2)}
.g-md :not(pre)>code,.g-code-inline{font-size:.9em;background:var(--fill);padding:1px 5px;border-radius:4px}
.g-md img{max-width:100%;border:1px solid var(--line-2);border-radius:var(--radius)}
pre.g-code{margin:0;padding:12px 14px;background:var(--fill);border:1px solid var(--line-2);border-radius:var(--radius);overflow-x:auto;font-size:12.5px;line-height:1.5}
.g-muted{color:var(--ink-2);font-size:12.5px}
.g-mono{font-family:var(--font-mono);font-size:12.5px}

/* tables (html-anything data-report: zebra, hover, sticky header) */
.g-table-wrap{overflow:auto;max-height:var(--max-h,none)}
table.g-table{width:100%;border-collapse:collapse;font-size:13px}
.g-table th{position:sticky;top:0;background:var(--paper);text-align:left;font:11px/1.3 var(--font-mono);color:var(--ink-2);font-weight:400;padding:6px 10px;border-bottom:1px solid var(--line-2);white-space:nowrap}
.g-table td{padding:7px 10px;border-bottom:1px solid var(--line-2);vertical-align:top}
.g-table tbody tr:nth-child(even) td{background:var(--fill)}.g-table tbody tr:hover td{background:var(--accent-bg)}
.g-nw{white-space:nowrap}

/* status badges */
.g-status{white-space:nowrap;font-weight:500}.g-status-icon{display:inline-block;width:1.1em;font-weight:700}
.g-status--ok{color:var(--ok)}.g-status--no{color:var(--err)}.g-status--warn{color:var(--warn)}.g-status--na{color:var(--ink-3)}

/* callout */
.g-callout{border:1px solid var(--line-2);border-left:3px solid var(--accent);background:var(--accent-bg);padding:10px 14px;border-radius:var(--radius)}
.g-callout--ok{border-left-color:var(--ok);background:var(--ok-bg)}.g-callout--warn{border-left-color:var(--warn);background:var(--warn-bg)}.g-callout--err{border-left-color:var(--err);background:var(--err-bg)}
.g-callout-title{font-weight:600;margin-bottom:4px}

/* kv title block */
.g-kv{display:grid;grid-template-columns:repeat(var(--kv-cols,3),minmax(0,1fr));margin:0;border-top:var(--bw) solid var(--line);border-left:var(--bw) solid var(--line)}
.g-kv-cell{border-right:var(--bw) solid var(--line);border-bottom:var(--bw) solid var(--line);padding:8px 10px 10px;min-width:0}
.g-kv-cell--wide{grid-column:1/-1}.g-kv dt{font:11px/1.4 var(--font-mono);color:var(--ink-2)}
.g-kv dd{margin:2px 0 0;font-size:20px;font-weight:600;font-variant-numeric:tabular-nums;overflow-wrap:anywhere}
.g-kv dd small{font-size:12px;font-weight:400;color:var(--ink-2);margin-left:4px}
.g-kv dd.is-ok{color:var(--ok)}.g-kv dd.is-err{color:var(--err)}
html[data-theme="card"] .g-kv{border-color:var(--line);border-radius:var(--radius);overflow:hidden}
html[data-theme="card"] .g-kv-cell{border-color:var(--line)}

/* limits bars: value vs threshold */
.g-lim+.g-lim{margin-top:14px}
.g-lim-head{display:flex;justify-content:space-between;gap:12px;font-size:13px;margin-bottom:4px}
.g-lim-val{font:12px var(--font-mono);color:var(--ok);white-space:nowrap}
.g-lim-track{position:relative;height:12px;border:1px solid var(--line-2);background:var(--fill);border-radius:calc(var(--radius)/2)}
.g-lim-fill{position:absolute;left:0;top:0;bottom:0;background:var(--ok-bg);border-right:2px solid var(--ok)}
.g-lim-mark{position:absolute;top:-5px;bottom:-5px;border-left:2px dashed var(--ink-2)}
.g-lim.is-bad .g-lim-fill{background:var(--err-bg);border-right-color:var(--err)}.g-lim.is-bad .g-lim-val{color:var(--err)}
.g-lim-note{font-size:11px;color:var(--ink-2);margin-top:3px}

/* stage timeline */
.g-tl{list-style:none;margin:0;padding:4px 0 0;display:grid;grid-template-columns:repeat(var(--n,1),minmax(0,1fr))}
.g-tl li{position:relative;text-align:center;padding:0 6px}
.g-tl li::before{content:"";position:absolute;top:34px;left:0;right:0;border-top:var(--bw) solid var(--line)}
.g-tl li:first-child::before{left:50%}.g-tl li:last-child::before{right:50%}
.g-tl-n{display:block;font:11px/1 var(--font-mono);color:var(--ink-3);height:18px}
.g-tl-dot{position:relative;display:flex;align-items:center;justify-content:center;width:26px;height:26px;margin:2px auto 8px;border:var(--bw) solid var(--line);border-radius:50%;background:var(--paper);font-size:13px;font-weight:700}
.g-tl li.is-ok .g-tl-dot{background:var(--ok);border-color:var(--ok);color:var(--paper)}
.g-tl li.is-bad .g-tl-dot{background:var(--err);border-color:var(--err);color:var(--paper)}
.g-tl-title{display:block;font-size:13px;font-weight:600}.g-tl-role{display:block;font:11px var(--font-mono);color:var(--ink-3)}
.g-tl-text{display:block;font-size:12px;color:var(--ink-2);line-height:1.45;margin-top:2px}
@media (max-width:960px){.g-tl{grid-template-columns:minmax(0,1fr)}.g-tl li{text-align:left;padding:0 0 10px 38px}.g-tl li::before{display:none}.g-tl-dot{position:absolute;left:0;top:0;margin:0}.g-tl-n{display:none}}

/* scenarios */
details.g-sc{border:1px solid var(--line-2);border-left:3px solid var(--ink-3);border-radius:var(--radius);padding:6px 10px;background:var(--paper)}
details.g-sc+details.g-sc{margin-top:6px}details.g-sc.is-ok{border-left-color:var(--ok)}details.g-sc.is-bad{border-left-color:var(--err);background:var(--err-bg)}
details.g-sc summary{cursor:pointer;display:flex;gap:8px;align-items:baseline;list-style:none}
details.g-sc summary::-webkit-details-marker{display:none}
.g-sc-name{flex:1;font-weight:500}.g-sc-id{font:11px var(--font-mono);color:var(--ink-3);white-space:nowrap}
ol.g-steps{margin:8px 0 2px;padding-left:22px}ol.g-steps li.is-bad{color:var(--err)}ol.g-steps li.is-skip{color:var(--ink-3)}
.g-kw{font-weight:700;color:var(--accent);margin-right:4px}
pre.g-err{margin:4px 0 0;white-space:pre-wrap;color:var(--err);background:transparent;border:0;padding:0;font-size:12px}
.g-feature+.g-feature{margin-top:16px}.g-feature-title{font-weight:600;margin-bottom:6px}

/* charts */
svg.g-chart{display:block;width:100%;height:auto}
svg .t-lbl{font:11px var(--font-sans);fill:var(--ink-2)}svg .t-val{font:11px var(--font-mono);fill:var(--ink)}
svg .t-thr{font:11px var(--font-sans);fill:var(--warn)}
svg .c-ok{fill:var(--ok)}svg .c-bad{fill:var(--err)}svg .c-frame{fill:none;stroke:var(--line-2)}
svg .c-grid{stroke:var(--line-2);stroke-dasharray:2 3}svg .c-thr{stroke:var(--warn);stroke-width:2;stroke-dasharray:6 4;fill:none}
svg .c-zone{fill:var(--err-bg);opacity:.6}
svg .a-box{fill:var(--paper);stroke:var(--line);stroke-width:1.5}svg .a-name{font:600 13px var(--font-sans);fill:var(--ink)}
svg .a-sub{font:11px var(--font-mono);fill:var(--ink-3)}svg .a-edge{stroke:var(--ink-2);stroke-width:1.5;fill:none}
svg .a-edge.is-bad{stroke:var(--err);stroke-dasharray:6 4;stroke-width:2}svg .a-elbl{font:11px var(--font-mono);fill:var(--ink-2)}
svg .a-elbl.is-bad{fill:var(--err)}

/* media */
.g-demo iframe{display:block;width:100%;height:440px;border:1px solid var(--line-2);border-radius:var(--radius);background:#0d1117}
.g-demo iframe.g-diagram{height:720px;background:var(--paper)}
.g-demo+.g-demo{margin-top:14px}.g-demo-title{font-weight:600;margin-bottom:6px}
figure.g-fig{margin:0}figure.g-fig img,video.g-video{max-width:100%;border:1px solid var(--line-2);border-radius:var(--radius)}
figure.g-fig figcaption{font-size:12px;color:var(--ink-2);margin-top:4px}
.g-scroll{max-height:560px;overflow:auto;padding-right:4px}

/* insights + methodology */
ul.g-insights{list-style:none;padding:0;margin:0}ul.g-insights li{padding:6px 0;border-bottom:1px dashed var(--line-2)}ul.g-insights li:last-child{border-bottom:0}
details.g-method{border-top:1px solid var(--line-2);padding-top:10px}details.g-method summary{cursor:pointer;color:var(--ink-2);font-size:13px}
.g-foot{margin-top:24px;font-size:12px;color:var(--ink-3);text-align:center}
@media print{.g-toolbar{display:none}details{open:true}}
`;

export const PAGE_JS = `(()=>{const root=document.documentElement;const KEY='gauntlet-report-prefs';
let prefs={};try{prefs=JSON.parse(localStorage.getItem(KEY)||'{}')}catch(e){}
if(prefs.theme)root.dataset.theme=prefs.theme;if(prefs.mode)root.dataset.mode=prefs.mode;
const save=()=>{try{localStorage.setItem(KEY,JSON.stringify({theme:root.dataset.theme,mode:root.dataset.mode}))}catch(e){}};
const bind=(name,attr,values)=>{const b=document.querySelector('[data-g="'+name+'"]');if(!b)return;const labels=JSON.parse(b.dataset.labels);
const show=()=>{b.textContent=labels[root.dataset[attr]]||root.dataset[attr]};show();
b.addEventListener('click',()=>{root.dataset[attr]=values[(values.indexOf(root.dataset[attr])+1)%values.length];show();save()})};
bind('theme','theme',['blueprint','card']);bind('mode','mode',['auto','light','dark']);
const copy=document.querySelector('[data-g="copy"]');copy&&copy.addEventListener('click',async()=>{const text=document.getElementById('gauntlet-data').textContent;
try{await navigator.clipboard.writeText(text)}catch(e){const ta=Object.assign(document.createElement('textarea'),{value:text});document.body.append(ta);ta.select();document.execCommand('copy');ta.remove()}
const o=copy.textContent;copy.textContent='已复制';setTimeout(()=>{copy.textContent=o},1400)});
document.querySelectorAll('[data-g="expand"]').forEach(b=>b.addEventListener('click',()=>{const ds=b.closest('.g-panel').querySelectorAll('details');const open=![...ds].every(d=>d.open);ds.forEach(d=>d.open=open)}));
})();`;
