// Terminal demo recorder -> self-contained HTML "video" (typed replay) + asciinema .cast + .txt.
// Script (JSON):
// { "title": "罗马数字 CLI", "cwd": ".", "steps": [
//     { "say": "把 1994 转成罗马数字" },
//     { "run": "build-gauntlet/roman 1994" } ] }
// Every command is really executed; the replay shows the real output and exit code.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { ensureDir, which } from './util.mjs';

/** POSIX shell for demo commands. On Windows prefer Git Bash (cmd.exe mangles / paths and output encoding). */
function demoShell(override) {
  if (override) return override;
  if (process.platform !== 'win32') return '/bin/sh';
  const git = which('git');
  if (git) {
    let dir = path.dirname(git);
    for (let i = 0; i < 4; i++, dir = path.dirname(dir)) {
      for (const cand of ['bin/bash.exe', 'usr/bin/bash.exe']) if (fs.existsSync(path.join(dir, cand))) return path.join(dir, cand);
    }
  }
  return true; // cmd.exe
}

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function recordDemo(cfg, scriptPath) {
  const script = JSON.parse(fs.readFileSync(scriptPath, 'utf8').replace(/^\uFEFF/, ''));
  const name = path.basename(scriptPath).replace(/\.json$/i, '');
  const cwd = path.resolve(path.dirname(scriptPath), script.cwd || cfg.root);
  const shell = demoShell(script.shell);
  const events = [];
  for (const st of script.steps) {
    if (st.say) { events.push({ kind: 'say', text: st.say }); continue; }
    const t0 = Date.now();
    // Binaries are coverage-instrumented: keep their .profraw out of the working tree.
    const env = { ...process.env, LLVM_PROFILE_FILE: path.join(os.tmpdir(), 'gauntlet-demo-%p.profraw') };
    const r = spawnSync(st.run, { cwd, shell, env, encoding: 'utf8', input: st.stdin ?? '', timeout: (st.timeoutSec || 30) * 1000, windowsHide: true });
    events.push({ kind: 'run', cmd: st.run, out: (r.stdout || '') + (r.stderr || ''), code: r.status ?? 1, ms: Date.now() - t0, expectCode: st.expectCode ?? 0 });
    console.log(`$ ${st.run}  -> exit ${r.status}`);
  }
  const outDir = ensureDir(cfg.out('evidence', 'demos'));
  const title = script.title || name;
  fs.writeFileSync(path.join(outDir, `${name}.html`), replayHtml(title, events));
  fs.writeFileSync(path.join(outDir, `${name}.txt`), events.map((e) => (e.kind === 'say' ? `# ${e.text}` : `$ ${e.cmd}\n${e.out}${e.code ? `[exit ${e.code}]\n` : ''}`)).join('\n'));
  fs.writeFileSync(path.join(outDir, `${name}.cast`), toCast(title, events));
  const failed = events.filter((e) => e.kind === 'run' && e.code !== e.expectCode);
  return { name, events, failed, file: path.join(outDir, `${name}.html`) };
}

function toCast(title, events) {
  const lines = [JSON.stringify({ version: 2, width: 100, height: 30, title })];
  let t = 0.5;
  const emit = (s) => { lines.push(JSON.stringify([Number(t.toFixed(3)), 'o', s])); };
  for (const e of events) {
    if (e.kind === 'say') { emit(`\x1b[36m# ${e.text}\x1b[0m\r\n`); t += 1.2; continue; }
    emit('\x1b[32m$\x1b[0m ');
    for (const ch of e.cmd) { t += 0.04; emit(ch); }
    t += 0.3; emit('\r\n');
    emit(e.out.replace(/\r?\n/g, '\r\n'));
    t += 1.0;
  }
  return lines.join('\n') + '\n';
}

function replayHtml(title, events) {
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>${esc(title)}</title>
<style>body{margin:0;background:#0d1117;color:#e6edf3;font:14px/1.5 ui-monospace,Consolas,"Cascadia Mono",monospace}
.bar{display:flex;gap:8px;align-items:center;padding:8px 12px;background:#161b22;border-bottom:1px solid #30363d;font-family:system-ui,"Microsoft YaHei",sans-serif;font-size:13px}
.bar b{flex:1}.bar button{background:#21262d;color:#e6edf3;border:1px solid #30363d;border-radius:6px;padding:3px 10px;cursor:pointer}
#t{padding:12px 16px;white-space:pre-wrap;word-break:break-all;height:calc(100vh - 60px);overflow:auto;box-sizing:border-box}
.p{color:#3fb950}.say{color:#79c0ff;font-family:system-ui,"Microsoft YaHei",sans-serif}.err{color:#ff7b72}.cur{background:#e6edf3;width:8px;display:inline-block;animation:b 1s steps(1) infinite}@keyframes b{50%{opacity:0}}</style></head>
<body><div class="bar"><b>▶ ${esc(title)}</b><button id="pp">⏸ 暂停</button><button id="rs">↺ 重播</button><button id="sp">1x</button><button id="all">全部显示</button></div><div id="t"></div>
<script>
const EV=${JSON.stringify(events).replace(/</g, '\\u003c')};
const t=document.getElementById('t');let speed=1,paused=false,gen=0;
const sleep=(ms)=>new Promise(r=>setTimeout(r,ms/speed));
async function waitUnpaused(g){while(paused&&g===gen)await sleep(100)}
function add(cls,txt){const s=document.createElement('span');if(cls)s.className=cls;s.textContent=txt;t.appendChild(s);t.scrollTop=t.scrollHeight;return s}
async function play(){const g=++gen;t.textContent='';for(const e of EV){if(g!==gen)return;await waitUnpaused(g);
 if(e.kind==='say'){add('say','# '+e.text+'\\n');await sleep(900);continue}
 add('p','$ ');const c=add('', '');for(const ch of e.cmd){if(g!==gen)return;await waitUnpaused(g);c.textContent+=ch;await sleep(35)}
 add('','\\n');await sleep(250);
 for(const line of e.out.split(/(?<=\\n)/)){if(g!==gen)return;add('',line);await sleep(60)}
 if(e.code!==0)add('err','[exit '+e.code+']\\n');add('','\\n');await sleep(700)}
 add('cur',' ')}
function showAll(){gen++;t.textContent='';for(const e of EV){if(e.kind==='say'){add('say','# '+e.text+'\\n');continue}add('p','$ ');add('',e.cmd+'\\n'+e.out);if(e.code!==0)add('err','[exit '+e.code+']\\n');add('','\\n')}}
pp.onclick=()=>{paused=!paused;pp.textContent=paused?'▶ 继续':'⏸ 暂停'};rs.onclick=()=>{paused=false;pp.textContent='⏸ 暂停';play()};
sp.onclick=()=>{speed=speed===1?2:speed===2?4:1;sp.textContent=speed+'x'};all.onclick=showAll;play();
</script></body></html>`;
}
