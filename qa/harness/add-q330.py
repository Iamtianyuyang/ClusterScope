#!/usr/bin/env python3
"""Add check Q330 (the no-root merge-invariant harvest, constraint MRG-02) to
qa/qa-report.json.

Q330 is the pre-merge snapshot of NRM1-NRM8, the material stage 6 needs to answer
M10. It is added to BOTH the `checks` array (so the constraints gate can pair it
with MRG-02) and the `extraNewChecks` array (which is where this stage records its
additions). Nothing else in the file is touched: the entry is spliced in as one
inline line, in the report's own style.

Idempotent. Usage: python3 qa/harness/add-q330.py
"""
import json

REPORT = "qa/qa-report.json"

ENTRY = {
    "id": "Q330",
    "constraint": "MRG-02",
    "title": "no-root 合流不变量 NRM1-NRM8 的合流前快照（M10 的输入）",
    "action": ("grep -c '^| NRM' qa/merge-plan-requirements.md; grep -rn "
               "'/etc/clusterscope|/var/lib/clusterscope|/var/log/clusterscope|/usr/local/bin' crates "
               "deploy/install-agent.sh deploy/tui.sh; grep -rnE "
               "'setuid|setgid|pre_exec|pkexec|sudo |chown' crates/"),
    "expected": ("M10 夹具齐全；NRM3 只有三处可覆盖默认值；NRM5 无真特权原语；"
                 "NRM1/NRM2/NRM6/NRM7/NRM8 由 Q300-Q322 支撑"),
    "actual": ("8 行 NRM 不变量；NRM3=3 处可覆盖默认值（crates/server/src/main.rs:186、"
               "crates/agent/src/main.rs:15、crates/common/src/config.rs:38，与审查前基线同数）；"
               "NRM5=0 个真特权原语（唯一 pre_exec 命中是 job_executor.rs:128 的 libc::setsid() 进程组设置）；"
               "NRM4 需在合流后的树上重跑（B 的 12 个文件未提交）。注意 merge-plan-requirements.md 的 NRM5 "
               "grep 模式把 | 转义了、在 GNU grep 下匹配空集，其「0 命中」不是测量结果（见 N14）"),
    "status": "pass",
    "verdict": "holds",
    "evidence": ["no-root-verify-17-merge-invariants.txt"],
}


def array_bounds(s, key):
    i = s.index('\n  "%s": [' % key)
    start = s.index("[", i)
    depth = 0
    in_str = False
    esc = False
    k = start
    while k < len(s):
        c = s[k]
        if in_str:
            if esc:
                esc = False
            elif c == "\\":
                esc = True
            elif c == '"':
                in_str = False
        else:
            if c == '"':
                in_str = True
            elif c in "[{":
                depth += 1
            elif c in "]}":
                depth -= 1
                if depth == 0:
                    return start, k
        k += 1
    raise SystemExit("array %r not terminated" % key)


def inline(v):
    return json.dumps(v, ensure_ascii=False, separators=(", ", ": "))


def main():
    with open(REPORT, encoding="utf-8") as fh:
        s = fh.read()
    if '"Q330"' in s:
        print("Q330 already present - nothing to do")
        return

    # Rebuild each target array from its own text: the existing entries stay byte for
    # byte, one new inline line is appended. (Splicing a comma next to the closing
    # bracket is what produced the earlier "Expecting ',' delimiter" errors.)
    for key in ("checks", "extraNewChecks"):
        start, close = array_bounds(s, key)
        body = s[start + 1:close].strip("\n")
        if not body.rstrip().endswith(","):
            body = body.rstrip() + ","
        new_block = "[\n" + body.rstrip() + "\n    " + inline(ENTRY) + "\n  ]"
        s = s[:start] + new_block + s[close + 1:]

    json.loads(s)  # must stay valid
    tmp = REPORT + ".tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        fh.write(s)
    import os
    os.replace(tmp, REPORT)
    print("Q330 added to checks + extraNewChecks (%d bytes, %d lines)"
          % (len(s), len(s.splitlines())))


if __name__ == "__main__":
    main()
