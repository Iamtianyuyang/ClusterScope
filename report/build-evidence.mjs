// 把中文审查者简报注入 kit 生成的证据包，产出单文件 HTML。
// 用法：node report/build-evidence.mjs <kit-index.html> <brief.html> <out.html>
import fs from 'node:fs';
import path from 'node:path';

const [kitPath, briefPath, outPath] = process.argv.slice(2);
if (!kitPath || !briefPath || !outPath) {
  console.error('用法: node report/build-evidence.mjs <kit-index.html> <brief.html> <out.html>');
  process.exit(2);
}

let html = fs.readFileSync(kitPath, 'utf8');
const brief = fs.readFileSync(briefPath, 'utf8');

// 1) 简报插在 <header class="g-head">…</header> 之后、g-frame 之前。
const headerEnd = html.indexOf('</header>');
if (headerEnd < 0) throw new Error('找不到 </header>');
const frameStart = html.indexOf('<div class="g-frame">', headerEnd);
if (frameStart < 0) throw new Error('找不到 <div class="g-frame">');
const anchor = html.slice(headerEnd + '</header>'.length, frameStart);
const injected = anchor.replace(/\s*$/, '\n') + brief;
html = html.slice(0, headerEnd + '</header>'.length) + injected + html.slice(frameStart);

// 2) kit 的模板文案在"只读审查"语境里会误读，改成指向简报的说法（原句保留在简报第 0 节里逐字引用）。
const introFrom = '有闸门未通过，这一版不能合并。红色面板说明了原因。';
const introTo = '有闸门未通过（硬阈值下的质量闸门，属审查结论）。'
  + '这是只读审查、没有待合并的代码改动；请看上面的「审查者简报」第 0 节、第 2 节（无 root 合规）与第 3 节，'
  + '下面的机器面板 A–O 是原始记录。';
if (!html.includes(introFrom)) throw new Error('找不到 kit 的结论引文（intro）');
html = html.replace(introFrom, introTo);

const routeFrom = '有闸门未通过：不要合并。Leader 会按失败的闸门安排返工。';
const routeTo = '有闸门未通过：本审查是只读审查，没有待合并的代码改动——'
  + '这里的含义是「修完必修项前不该继续往上叠新功能」。'
  + '必修项清单见「审查者简报」第 1 节的 F-01/F-16/F-02/F-07~F-11、第 2 节的 NF-01/NF-02（no-root），'
  + '以及合流方案 report/merge-plan.md 的 M8 与 M10（不变量 NRM1–NRM8）。';
if (!html.includes(routeFrom)) throw new Error('找不到 kit 的审阅路线引文（route）');
html = html.replace(routeFrom, routeTo);

// 3) 标题加一个"审查"标记，方便在浏览器标签里区分。
html = html.replace(
  '<title>证据包 · ClusterScope GitHub 已发布线（f9c080b）全维度审查</title>',
  '<title>审查证据包 · ClusterScope GitHub 已发布线（f9c080b）全维度审查</title>',
);

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, html);
const kb = (fs.statSync(outPath).size / 1024).toFixed(0);
console.log(`evidence: ${outPath} (${kb} KB, brief ${(brief.length / 1024).toFixed(1)} KB injected)`);
