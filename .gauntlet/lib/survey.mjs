// `gauntlet survey`: a deterministic first look at a repository for the stage-0 Surveyor, so every model
// starts from facts instead of guesses: languages and sizes, build systems, test frameworks, CI commands,
// lint / architecture tools already configured, entry points, docs, gauntlet state, and whether the
// project profile GAUNTLET.md is stale. It also drafts an adapter + commands; the Surveyor must still
// RUN the commands to confirm them. Output: <outDir>/survey.json and survey.md.
import fs from 'node:fs';
import path from 'node:path';
import { run, which, toPosix, writeJson, pathOf, resolveBase } from './util.mjs';

const LANG = {
  c: 'C', h: 'C/C++ header', cc: 'C++', cpp: 'C++', cxx: 'C++', hh: 'C++', hpp: 'C++', hxx: 'C++', cu: 'CUDA', cuh: 'CUDA',
  js: 'JavaScript', mjs: 'JavaScript', cjs: 'JavaScript', jsx: 'JavaScript', ts: 'TypeScript', mts: 'TypeScript', cts: 'TypeScript', tsx: 'TypeScript',
  py: 'Python', go: 'Go', rs: 'Rust', java: 'Java', kt: 'Kotlin', kts: 'Kotlin', scala: 'Scala', cs: 'C#', swift: 'Swift',
  rb: 'Ruby', php: 'PHP', dart: 'Dart', m: 'Objective-C', mm: 'Objective-C++', lua: 'Lua', r: 'R', jl: 'Julia', f90: 'Fortran', f95: 'Fortran', f03: 'Fortran', f08: 'Fortran', f: 'Fortran', for: 'Fortran', f77: 'Fortran',
};
const PRUNE = new Set(['.git', 'node_modules', '.venv', 'venv', 'env', '__pycache__', '.tox', '.nox', '.mypy_cache', '.pytest_cache', 'dist', 'target',
  '.gradle', '.idea', '.vs', '.vscode', 'gauntlet-out', '.gauntlet', 'coverage', '.next', '.nuxt', '.cache', 'bazel-bin', 'bazel-out', 'bazel-testlogs']);
const THIRD_PARTY = /(^|\/)(third_party|3rdparty|vendor|external|extern|deps|_deps)(\/|$)/i;
const TEST_PATH = /(^|\/)(tests?|testing|__tests__|spec|specs)\/|(_test|_tests|\.test|\.spec)\.[^/]+$|(^|\/)test_[^/]+$|(^|\/)conftest\.py$/i;
const BUILD_DIR = /^(build|cmake-build[^/]*|out|bin\/Debug|bin\/Release|obj)(\/|$)/i;

const MARKERS = [
  ['CMakeLists.txt', 'CMake'], ['Makefile', 'Make'], ['makefile', 'Make'], ['GNUmakefile', 'Make'], ['meson.build', 'Meson'],
  ['WORKSPACE', 'Bazel'], ['WORKSPACE.bazel', 'Bazel'], ['MODULE.bazel', 'Bazel'], ['configure.ac', 'Autotools'], ['SConstruct', 'SCons'],
  ['package.json', 'npm'], ['pyproject.toml', 'Python (pyproject)'], ['setup.py', 'Python (setup.py)'], ['setup.cfg', 'Python (setup.cfg)'],
  ['requirements.txt', 'Python (requirements)'], ['tox.ini', 'tox'], ['noxfile.py', 'nox'], ['go.mod', 'Go modules'], ['Cargo.toml', 'Cargo'],
  ['pom.xml', 'Maven'], ['build.gradle', 'Gradle'], ['build.gradle.kts', 'Gradle'], ['Gemfile', 'Bundler'], ['composer.json', 'Composer'],
  ['pubspec.yaml', 'Dart/Flutter'], ['Package.swift', 'SwiftPM'],
];
const MARKER_EXT = [[/\.sln$/i, 'Visual Studio solution'], [/\.vcxproj$/i, 'MSBuild (C++)'], [/\.csproj$/i, '.NET']];
const LINT_CONFIGS = [
  [/^\.eslintrc(\.\w+)?$|^eslint\.config\.\w+$/, 'eslint'], [/^\.prettierrc(\.\w+)?$|^prettier\.config\.\w+$/, 'prettier'],
  [/^\.?ruff\.toml$/, 'ruff'], [/^\.flake8$/, 'flake8'], [/^mypy\.ini$|^\.mypy\.ini$/, 'mypy'], [/^\.pylintrc$|^pylintrc$/, 'pylint'],
  [/^\.clang-tidy$/, 'clang-tidy'], [/^\.clang-format$/, 'clang-format'], [/^\.golangci\.ya?ml$|^\.golangci\.toml$/, 'golangci-lint'],
  [/^clippy\.toml$|^\.clippy\.toml$/, 'clippy'], [/^rustfmt\.toml$|^\.rustfmt\.toml$/, 'rustfmt'], [/^\.editorconfig$/, 'editorconfig'],
  [/^\.pre-commit-config\.ya?ml$/, 'pre-commit'], [/^\.dependency-cruiser\.\w+$/, 'dependency-cruiser (arch)'], [/^\.importlinter$/, 'import-linter (arch)'],
  [/^\.cppcheck$|^cppcheck\.cfg$/, 'cppcheck'], [/^stryker\.conf\.\w+$|^stryker\.config\.\w+$/, 'Stryker (mutation)'], [/^\.semgrep\.ya?ml$/, 'semgrep'],
];
const DOCS = /^(README(\.\w+)?|AGENTS\.md|CLAUDE\.md|CONTRIBUTING(\.\w+)?|GEMINI\.md|\.cursorrules|copilot-instructions\.md|ARCHITECTURE\.md|DEVELOPMENT\.md|HACKING(\.\w+)?)$/i;

function walk(root, limit = 60000) {
  const files = [];
  const rec = (dir, rel) => {
    let ents;
    try { ents = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of ents) {
      if (files.length >= limit) return;
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) {
        if (PRUNE.has(e.name) || /^cmake-build/i.test(e.name) || /^build(-.*)?$/i.test(e.name) || /^bazel-/.test(e.name)) continue;
        rec(path.join(dir, e.name), r);
      } else if (e.isFile()) files.push(r);
    }
  };
  rec(root, '');
  return files;
}

const read = (root, rel, max = 2 << 20) => {
  try { const p = path.join(root, rel); return fs.statSync(p).size <= max ? fs.readFileSync(p, 'utf8') : ''; } catch { return ''; }
};
const lineCount = (s) => (s ? s.split('\n').filter((l) => l.trim()).length : 0);

/** `run:` commands of GitHub workflows, `script:` of GitLab, `sh` steps of Jenkins. */
function ciCommands(root, files) {
  const out = [];
  for (const f of files) {
    const text = read(root, f);
    if (/^\.github\/workflows\/.+\.ya?ml$/.test(f)) {
      const lines = text.split(/\r?\n/);
      for (let i = 0; i < lines.length; i++) {
        const m = lines[i].match(/^(\s*)-?\s*run:\s*(.*)$/);
        if (!m) continue;
        if (m[2] && !/^[|>]-?\s*$/.test(m[2])) { out.push({ file: f, cmd: m[2].trim() }); continue; }
        const ind = m[1].length;
        for (let j = i + 1; j < lines.length; j++) {
          const l = lines[j];
          if (l.trim() && l.match(/^\s*/)[0].length <= ind) break;
          if (l.trim()) out.push({ file: f, cmd: l.trim() });
        }
      }
    } else if (/(^|\/)\.gitlab-ci\.ya?ml$/.test(f) || /azure-pipelines\.ya?ml$/.test(f)) {
      for (const m of text.matchAll(/^\s*-\s+(?!name:|uses:)(.+)$/gm)) out.push({ file: f, cmd: m[1].trim() });
    } else if (/(^|\/)Jenkinsfile$/.test(f)) {
      for (const m of text.matchAll(/\bsh\s+['"]([^'"]+)['"]/g)) out.push({ file: f, cmd: m[1].trim() });
    }
  }
  return out.filter((c) => c.cmd && !/^#/.test(c.cmd)).slice(0, 80);
}

function profileState(cfg, root) {
  const file = pathOf(cfg, 'profile');
  const rel = path.relative(root, file).split(path.sep).join('/');
  if (!fs.existsSync(file)) return { exists: false };
  const text = fs.readFileSync(file, 'utf8');
  const recorded = (text.match(/commit[:：]\s*`?([0-9a-f]{7,40})`?/i) || [])[1] || null;
  // the profile is as fresh as the last commit that touched it (that commit also carries the survey's
  // config changes); the recorded hash is only the fallback for a profile that is not committed yet
  const last = run('git', ['log', '-1', '--format=%h', '--', rel], { cwd: root, quiet: true, allowFail: true }).stdout.trim();
  const commit = last || recorded;
  const state = { exists: true, commit, stale: [], changedFiles: 0 };
  if (!commit) { state.stale.push('GAUNTLET.md 没有提交，也没有记录 commit（写成 `commit: <hash>`）'); return state; }
  const r = run('git', ['diff', '--name-only', commit, '--', '.'], { cwd: root, quiet: true, allowFail: true });
  if (r.code !== 0) { state.stale.push(`记录的 commit ${commit} 在本仓库里找不到`); return state; }
  const changed = r.stdout.split(/\r?\n/).filter((f) => f && !f.startsWith('.gauntlet/') && !f.startsWith(`${cfg.outDir}/`));
  state.changedFiles = changed.length;
  const important = changed.filter((f) => {
    const base = path.posix.basename(f);
    return MARKERS.some(([m]) => m === base) || MARKER_EXT.some(([re]) => re.test(base)) || LINT_CONFIGS.some(([re]) => re.test(base))
      || /^\.github\/workflows\//.test(f) || /\.gitlab-ci\.ya?ml$|Jenkinsfile$/.test(f) || base === 'gauntlet.config.json';
  });
  state.stale.push(...important.map((f) => `构建 / CI / 质量配置变了：${f}`));
  return state;
}

export function survey(cfg) {
  const root = cfg.root;
  const files = walk(root);
  const base = (f) => path.posix.basename(f);
  const depth = (f) => f.split('/').length - 1;

  // languages: production vs test vs third-party lines
  const langs = {};
  const dirs = {};
  for (const f of files) {
    const ext = (f.match(/\.([A-Za-z0-9]+)$/) || [])[1]?.toLowerCase();
    const lang = LANG[ext];
    if (!lang || BUILD_DIR.test(f)) continue;
    const n = lineCount(read(root, f));
    const kind = THIRD_PARTY.test(f) ? 'thirdParty' : TEST_PATH.test(f) ? 'test' : 'product';
    const L = langs[lang] || (langs[lang] = { files: 0, product: 0, test: 0, thirdParty: 0 });
    L.files++; L[kind] += n;
    if (kind === 'product') {
      const top = f.includes('/') ? f.split('/')[0] : '.';
      const d = dirs[top] || (dirs[top] = { lines: 0, langs: {}, exts: new Set() });
      d.lines += n; d.langs[lang] = (d.langs[lang] || 0) + n; d.exts.add(ext);
    }
  }
  const langList = Object.entries(langs).map(([name, v]) => ({ name, ...v })).sort((a, b) => b.product - a.product);

  // build systems / manifests (up to depth 3)
  const build = [];
  for (const f of files) {
    if (depth(f) > 3) continue;
    const hit = MARKERS.find(([m]) => m === base(f)) || MARKER_EXT.find(([re]) => re.test(base(f)));
    if (hit) build.push({ file: f, system: hit[1] });
  }

  // ecosystem details
  const tests = new Set();
  const tools = new Set();
  const notes = [];
  const pkg = files.includes('package.json') ? (() => { try { return JSON.parse(read(root, 'package.json')); } catch { return null; } })() : null;
  if (pkg) {
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };
    for (const [d, t] of [['jest', 'jest'], ['vitest', 'vitest'], ['mocha', 'mocha'], ['ava', 'ava'], ['@playwright/test', 'playwright'], ['cypress', 'cypress']]) if (deps[d]) tests.add(t);
    for (const [d, t] of [['eslint', 'eslint'], ['typescript', 'tsc'], ['@stryker-mutator/core', 'Stryker (mutation)'], ['dependency-cruiser', 'dependency-cruiser (arch)'], ['c8', 'c8 (coverage)'], ['nyc', 'nyc (coverage)'], ['jest-junit', 'jest-junit (JUnit)']]) if (deps[d]) tools.add(t);
    if (pkg.scripts) notes.push(`package.json scripts: ${Object.entries(pkg.scripts).map(([k, v]) => `${k}=\`${v}\``).join('；')}`);
    if (pkg.bin) notes.push(`package.json bin: ${JSON.stringify(pkg.bin)}`);
  }
  const jsTests = files.filter((f) => /\.(m|c)?[jt]sx?$/.test(f) && TEST_PATH.test(f)).slice(0, 200);
  if (jsTests.some((f) => /from ['"]node:test['"]|require\(['"]node:test['"]\)/.test(read(root, f, 200000)))) tests.add('node:test');
  const pyproject = read(root, 'pyproject.toml') + read(root, 'setup.cfg') + read(root, 'tox.ini') + files.filter((f) => /^requirements.*\.txt$/.test(base(f))).map((f) => read(root, f)).join('\n');
  if (pyproject) {
    if (/pytest/.test(pyproject) || files.some((f) => base(f) === 'conftest.py' || base(f) === 'pytest.ini')) tests.add('pytest');
    for (const [re, t] of [[/pytest-cov|\bcoverage\b/, 'coverage.py'], [/\bruff\b/, 'ruff'], [/\bmypy\b/, 'mypy'], [/import-linter|importlinter/, 'import-linter (arch)'], [/\bmutmut\b/, 'mutmut (mutation)'], [/\bbehave\b/, 'behave (Gherkin)'], [/pytest-bdd/, 'pytest-bdd (Gherkin)']]) if (re.test(pyproject)) tools.add(t);
  }
  if (files.some((f) => f.endsWith('_test.go'))) tests.add('go test');
  if (files.includes('Cargo.toml')) tests.add('cargo test');
  const cmakeText = files.filter((f) => base(f) === 'CMakeLists.txt' || f.endsWith('.cmake')).slice(0, 50).map((f) => read(root, f)).join('\n');
  if (cmakeText) {
    if (/gtest|GTest|googletest/.test(cmakeText)) tests.add('GoogleTest');
    if (/Catch2/.test(cmakeText)) tests.add('Catch2');
    if (/doctest/.test(cmakeText)) tests.add('doctest');
    if (/enable_testing|add_test/.test(cmakeText)) tests.add('CTest');
    if (/LANGUAGES[^)]*CUDA|enable_language\(\s*CUDA/.test(cmakeText)) notes.push('CMake 工程启用了 CUDA');
    if (/find_package\(\s*MPI|CMAKE_(C|CXX|Fortran)_COMPILER\b/.test(cmakeText)) notes.push('CMake 工程用了 MPI 或指定了编译器：先确认 clang 能原样编译它，否则用 commands 适配器、保留项目自己的编译器');
  }
  if (files.some((f) => base(f) === 'pom.xml' || /build\.gradle/.test(base(f)))) tests.add('JUnit (JVM)');
  const features = files.filter((f) => f.endsWith('.feature'));

  const lint = [];
  for (const f of files) {
    if (depth(f) > 2) continue;
    const hit = LINT_CONFIGS.find(([re]) => re.test(base(f)));
    if (hit) lint.push({ file: f, tool: hit[1] });
  }
  const ciFiles = files.filter((f) => /^\.github\/workflows\/.+\.ya?ml$|(^|\/)\.gitlab-ci\.ya?ml$|(^|\/)Jenkinsfile$|azure-pipelines\.ya?ml$|^\.circleci\/config\.ya?ml$/.test(f));
  const docs = files.filter((f) => depth(f) <= 2 && DOCS.test(base(f)));
  const entries = files.filter((f) => !TEST_PATH.test(f) && !THIRD_PARTY.test(f) && (/(^|\/)main\.(c|cc|cpp|cxx|cu|go|rs|java|kt|cs|swift)$/.test(f) || /(^|\/)__main__\.py$/.test(f) || /^cmd\/[^/]+\/main\.go$/.test(f) || /^bin\/[^/]+$/.test(f) || /^src\/bin\/.+\.rs$/.test(f))).slice(0, 30);

  // gauntlet state
  const gauntlet = {
    kit: fs.existsSync(path.join(root, '.gauntlet', 'VERSION')) ? read(root, '.gauntlet/VERSION').trim() : null,
    config: fs.existsSync(path.join(root, 'gauntlet.config.json')) ? { adapter: cfg.adapter, ratchet: !!cfg.ratchet?.enabled } : null,
    baseline: fs.existsSync(path.join(root, cfg.ratchet?.baseline || 'gauntlet-baseline.json')),
    profile: profileState(cfg, root),
    base: (() => { const b = resolveBase(cfg, 'auto'); return b ? { name: b.name, how: b.how } : null; })(),
    features: features.length,
  };

  // drafts: adapter, sources, commands
  const productLangs = langList.filter((l) => l.product > 0);
  const main = productLangs[0]?.name || null;
  const cish = ['C', 'C++', 'C/C++ header', 'CUDA'];
  const cOnly = productLangs.length > 0 && productLangs.every((l) => cish.includes(l.name));
  const rootCMake = files.includes('CMakeLists.txt');
  const adapter = cOnly && rootCMake ? 'cmake-clang' : 'commands';
  const reasons = [];
  if (adapter === 'cmake-clang') reasons.push('根目录有 CMakeLists.txt，产品代码全是 C/C++/CUDA');
  else if (cOnly) reasons.push('C/C++ 项目但不是 CMake 构建（Make / Bazel / MSBuild……）：用 commands 适配器');
  else reasons.push(`主要语言 ${productLangs.map((l) => l.name).join('、') || '未知'}：用 commands 适配器`);
  if (adapter === 'cmake-clang' && !which('clang')) reasons.push('⚠ 本机没有 clang：用 commands 适配器，保留项目自己的编译器');
  if (adapter === 'cmake-clang') reasons.push('cmake-clang 会用 clang 重新编译整个工程；项目依赖特定编译器（MPI 包装器、厂商编译器……）编不过时改用 commands');
  const sources = Object.entries(dirs).filter(([d]) => d !== '.').sort((a, b) => b[1].lines - a[1].lines)
    .map(([d, v]) => `${d}/**/*.${v.exts.size === 1 ? [...v.exts][0] : `{${[...v.exts].sort().join(',')}}`}`);
  if (dirs['.']) sources.push(...[...dirs['.'].exts].map((e) => `*.${e}`));
  const draft = adapter === 'commands' ? draftCommands({ tests, tools, pkg, cOnly, files }) : null;

  const result = {
    root: toPosix(root), at: new Date().toISOString(),
    commit: run('git', ['rev-parse', '--short', 'HEAD'], { cwd: root, quiet: true, allowFail: true }).stdout.trim() || null,
    files: files.length, languages: langList,
    productDirs: Object.entries(dirs).map(([d, v]) => ({ dir: d, lines: v.lines, langs: v.langs })).sort((a, b) => b.lines - a.lines),
    build, tests: [...tests], tools: [...tools], lint, ci: { files: ciFiles, commands: ciCommands(root, ciFiles) },
    docs, entries, features: features.length, notes, gauntlet,
    recommendation: { adapter, reasons, sources, commands: draft },
  };
  writeJson(cfg.out('survey.json'), result);
  fs.writeFileSync(cfg.out('survey.md'), toMarkdown(result));
  return result;
}

function draftCommands({ tests, tools, pkg, cOnly }) {
  const J = { junit: ['{out}/junit.xml'] };
  if (tests.has('node:test')) return { test: { run: 'node --test --experimental-test-coverage --test-reporter=spec --test-reporter-destination=stdout --test-reporter=junit --test-reporter-destination={out}/junit.xml --test-reporter=lcov --test-reporter-destination={out}/lcov.info', ...J, lcov: ['{out}/lcov.info'] } };
  if (tests.has('vitest')) return { test: { run: 'npx vitest run --reporter=default --reporter=junit --outputFile.junit={out}/junit.xml --coverage --coverage.reporter=lcov --coverage.reportsDirectory={out}/coverage', ...J, lcov: ['{out}/coverage/lcov.info'] }, $note: '需要 @vitest/coverage-v8' };
  if (tests.has('jest')) return { test: { run: 'npx jest --ci --coverage --coverageReporters=lcov --coverageDirectory={out}/coverage --reporters=default --reporters=jest-junit', env: { JEST_JUNIT_OUTPUT_DIR: '{out}' }, ...J, lcov: ['{out}/coverage/lcov.info'] }, $note: '需要 jest-junit' };
  if (tests.has('pytest')) return { test: { run: 'python -m pytest --junitxml={out}/junit.xml --cov --cov-report=lcov:{out}/lcov.info', ...J, lcov: ['{out}/lcov.info'] }, $note: '需要 pytest-cov' };
  if (tests.has('go test')) return { test: { run: 'gotestsum --junitfile {out}/junit.xml -- -coverprofile={out}/cover.out ./... && gocover-cobertura < {out}/cover.out > {out}/cobertura.xml', ...J, cobertura: ['{out}/cobertura.xml'] }, $note: '需要 gotestsum、gocover-cobertura' };
  if (tests.has('cargo test')) return { test: { run: 'cargo llvm-cov nextest --lcov --output-path {out}/lcov.info --profile ci', ...J, lcov: ['{out}/lcov.info'] }, $note: '需要 cargo-llvm-cov、cargo-nextest；在 .config/nextest.toml 的 [profile.ci.junit] 里把 JUnit 输出指向 {out}/junit.xml' };
  if (cOnly) return { build: '<项目原本的构建命令（编译器照旧），加上该编译器的覆盖率插桩选项>', test: { run: '<项目原本的测试命令> && <把覆盖率导出成 Cobertura 或 LCOV 的命令>', ...J, cobertura: ['{out}/cobertura.xml'] }, lint: [{ name: 'compiler', run: '<一次完整重新编译的命令，打开项目平时的告警选项>', parse: 'diagnostics' }], $note: 'C/C++ 项目：编译器、MPI 包装器、构建系统都照原样；告警用 parse=diagnostics 读取任意编译器输出' };
  if (pkg?.scripts?.test) return { test: { run: 'npm test', ...J, lcov: ['{out}/lcov.info'] }, $note: 'npm test 需要加上输出 JUnit 与 LCOV 的参数' };
  return { test: { run: '<跑全部测试并输出 JUnit + 覆盖率的命令>', ...J, lcov: ['{out}/lcov.info'] }, $note: '没有识别出测试框架' };
}

function toMarkdown(r) {
  const L = [];
  const g = r.gauntlet;
  L.push(`# 摸底：${r.root}`, '', `扫描 ${r.files} 个文件，commit \`${r.commit || '-'}\`。以下是机器识别的事实和草稿，命令必须实际运行确认。`, '');
  L.push('## Gauntlet 现状', '');
  L.push(`- 工具：${g.kit ? `已安装 ${g.kit}` : '未安装'}；配置：${g.config ? `adapter=${g.config.adapter}${g.config.ratchet ? '，棘轮模式' : ''}` : '没有 gauntlet.config.json'}；基线：${g.baseline ? '有' : '无'}；场景文件：${g.features} 个`);
  L.push(g.base ? `- 分支基线：\`${g.base.name}\`（${g.base.how}）——写进档案的 \`base:\`` : '- 分支基线：**找不到**（默认分支不叫 main / master / develop / trunk，远端也没有 origin/HEAD）：在档案里写 \`base: <默认分支>\`');
  if (!g.profile.exists) L.push('- 项目档案 GAUNTLET.md：**没有**（需要完整摸底）');
  else if (g.profile.stale.length) L.push(`- 项目档案 GAUNTLET.md：**可能过期**（记录于 ${g.profile.commit || '?'}，之后改动了 ${g.profile.changedFiles} 个文件）`, ...g.profile.stale.map((s) => `  - ${s}`));
  else L.push(`- 项目档案 GAUNTLET.md：最新（记录于 ${g.profile.commit}，之后改动了 ${g.profile.changedFiles} 个文件，没有构建 / CI / 质量配置变化）`);
  L.push('', '## 语言（非空行数）', '', '| 语言 | 产品代码 | 测试 | 第三方 | 文件 |', '|---|---|---|---|---|');
  for (const l of r.languages) L.push(`| ${l.name} | ${l.product} | ${l.test} | ${l.thirdParty} | ${l.files} |`);
  L.push('', '## 产品代码目录', '', ...r.productDirs.slice(0, 15).map((d) => `- \`${d.dir}/\` ${d.lines} 行（${Object.entries(d.langs).map(([k, v]) => `${k} ${v}`).join('，')}）`));
  L.push('', '## 构建与测试', '', ...r.build.map((b) => `- ${b.system}：\`${b.file}\``), `- 测试框架：${r.tests.join('、') || '未识别'}`, `- 相关工具：${r.tools.join('、') || '无'}`, ...r.notes.map((n) => `- ${n}`));
  L.push('', '## CI 里实际跑的命令', '');
  if (!r.ci.files.length) L.push('- 没有 CI 配置');
  else L.push(...r.ci.commands.slice(0, 40).map((c) => `- \`${c.cmd}\`  <sub>${c.file}</sub>`));
  L.push('', '## 已配置的质量工具', '', ...(r.lint.length ? r.lint.map((l) => `- ${l.tool}：\`${l.file}\``) : ['- 没有']));
  L.push('', '## 入口与文档', '', `- 入口：${r.entries.map((e) => `\`${e}\``).join('、') || '未识别'}`, `- 文档：${r.docs.map((e) => `\`${e}\``).join('、') || '无'}`);
  L.push('', '## 草稿（必须实际运行确认）', '', `- 适配器：**${r.recommendation.adapter}**（${r.recommendation.reasons.join('；')}）`, `- sources：\`${JSON.stringify(r.recommendation.sources)}\``);
  if (r.recommendation.commands) L.push('- commands：', '```json', JSON.stringify(r.recommendation.commands, null, 2), '```');
  return `${L.join('\n')}\n`;
}

export function printSurvey(r) {
  const g = r.gauntlet;
  console.log(`SURVEY  ${r.files} files  languages: ${r.languages.filter((l) => l.product).map((l) => `${l.name} ${l.product}`).join(', ') || '-'}`);
  console.log(`        build: ${[...new Set(r.build.map((b) => b.system))].join(', ') || '-'}   tests: ${r.tests.join(', ') || '-'}   CI: ${r.ci.files.length} file(s)`);
  console.log(`        gauntlet: kit=${g.kit || 'no'} config=${g.config ? g.config.adapter : 'no'} profile=${!g.profile.exists ? 'missing' : g.profile.stale.length ? 'STALE' : 'up to date'}`);
  console.log(`        recommend adapter: ${r.recommendation.adapter}`);
  console.log(`SURVEY: ${!g.profile.exists ? 'FULL' : g.profile.stale.length || !g.config ? 'REFRESH' : 'UP-TO-DATE'}  (details: survey.md)`);
}

export const surveyVerdict = (r) => (!r.gauntlet.profile.exists ? 'FULL' : r.gauntlet.profile.stale.length || !r.gauntlet.config ? 'REFRESH' : 'UP-TO-DATE');
