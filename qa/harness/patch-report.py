#!/usr/bin/env python3
"""Append the stage-5 no-root addendum to qa/qa-report.json WITHOUT touching a single
existing byte.

The report is hand-written compact JSON (one entry per line). Re-serialising it churned
234 lines of pure formatting for 23 real additions, so this patcher edits the text:
it finds the array that follows each anchor, inserts the new entries just before that
array's closing "  ]," line in the same inline style, and rewrites the few scalar/
container values that must grow (verdictRule, summary, three environment keys, the
mergePlanFacts M10 section, and the tail arrays).

Input: the JSON blocks produced by apply-nr-entries.py --blocks (see that script).
Usage: python3 qa/harness/patch-report.py
"""
import json
import os
import re
import sys

REPORT = "qa/qa-report.json"
BLOCKS = "/tmp/nr-blocks.json"


def inline(v):
    return json.dumps(v, ensure_ascii=False, separators=(", ", ": "))


def line_anchor(s, key):
    """The top-level key's own line: '\\n  "key":'. Inline entries inside the arrays also
    contain '"evidence": [' etc., so plain substring search is not enough."""
    a = '\n  "%s":' % key
    n = s.count(a)
    if n != 1:
        raise SystemExit("expected exactly one top-level %r, found %d" % (key, n))
    return a


def array_close(s, key):
    """Return the offset of the newline that introduces the closing bracket line of the
    top-level array `key`."""
    i = s.index(line_anchor(s, key)) + len(line_anchor(s, key))
    j = s.index("[", i)
    depth = 0
    in_str = False
    esc = False
    k = j
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
                    # s[k] is the ']' of the anchor array; its line starts after the
                    # preceding newline.
                    nl = s.rindex("\n", 0, k)
                    return nl
        k += 1
    raise SystemExit("unterminated array after %r" % key)


def insert_lines(s, anchor, lines):
    pos = array_close(s, anchor)  # the '\n' before the closing bracket line
    head = s[:pos].rstrip()
    if not head.endswith(","):
        head += ","
    body = "".join("    %s,\n" % inline(x) for x in lines)
    return head + "\n" + body + s[pos + 1:]


def grow_value(s, key, extra):
    """Append `extra` to the string value of a top-level key."""
    pat = re.compile(r'(\n  "%s": ")(.*?)(",\n)' % re.escape(key), re.S)
    m = pat.search(s)
    if not m:
        raise SystemExit("anchor not found: %s" % key)

    def repl(mo):
        return mo.group(1) + mo.group(2) + extra + mo.group(3)

    return pat.sub(repl, s, count=1)


def object_close(s, key):
    """Offset of the newline before the line that closes the top-level object `key`."""
    a = line_anchor(s, key)
    k = s.index("{", s.index(a) + len(a))
    depth = 0
    in_str = False
    esc = False
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
                    return s.rindex("\n", 0, k)
        k += 1
    raise SystemExit("unterminated object at %r" % key)


def add_environment_keys(s, keys):
    pos = object_close(s, '"environment": {')
    head = s[:pos].rstrip()
    if not head.endswith(","):
        head += ","
    body = "".join(
        '    %s: %s,\n' % (json.dumps(k, ensure_ascii=False), json.dumps(v, ensure_ascii=False))
        for k, v in keys.items()
    )
    return head + "\n" + body + s[pos + 1:]


def add_merge_plan_section(s, key, value):
    pos = object_close(s, '"mergePlanFacts": {')
    head = s[:pos].rstrip()
    if not head.endswith(","):
        head += ","
    body = '    %s: %s\n' % (json.dumps(key, ensure_ascii=False), json.dumps(value, ensure_ascii=False))
    return head + "\n" + body + s[pos + 1:]


def main():
    with open(REPORT, encoding="utf-8") as fh:
        s = fh.read()
    if "Q300" in s:
        print("Q300 already present - nothing to do")
        return
    with open(BLOCKS, encoding="utf-8") as fh:
        b = json.load(fh)

    # Collect every edit first, then apply them from the END of the file backwards so
    # no edit invalidates another edit's offsets.
    edits = []

    def line_at(pos, text):
        head = text[:pos].rstrip()
        if not head.endswith(","):
            head += ","
        return head + "\n"

    for key, items in [
        ("checks", b["checks"]),
        ("findings", b["findings"]),
        ("checklistDiscoveries", b["discoveries"]),
        ("unverifiable", b["unverifiable"]),
        ("evidence", b["evidence"]),
        ("stage5HarnessAdditions", b["harness"]),
        ("extraNewChecks", b["extraNewChecks"]),
        ("reproduceInOneGo", b["reproduce"]),
    ]:
        pos = array_close(s, key)
        add = "".join("    %s,\n" % inline(x) for x in items)
        edits.append((pos, add, key))

    pos = object_close(s, "environment")
    edits.append((pos, "".join(
        '    %s: %s,\n' % (json.dumps(k, ensure_ascii=False), json.dumps(v, ensure_ascii=False))
        for k, v in b["environmentKeys"].items()), "environment"))

    pos = object_close(s, "mergePlanFacts")
    edits.append((pos, '    %s: %s\n' % (
        json.dumps("M10_noRootInvariants_stage5", ensure_ascii=False),
        json.dumps(b["mergePlanM10"], ensure_ascii=False)), "mergePlanFacts"))

    # scalar growth edits change length too -> handle them via regex after the inserts
    for pos, add, key in sorted(edits, key=lambda e: -e[0]):
        s = line_at(pos, s) + add + s[pos + 1:]

    # An inserted block always ends with a comma; when it happens to be the last element
    # before a closing brace/bracket that comma is invalid. Fix by text, then verify by
    # parsing.
    s = re.sub(r",(\s*\n\s*[\]\}])", r"\1", s)

    s = grow_value(s, "verdictRule", b["verdictRuleAdd"])
    s = grow_value(s, "summary", b["summaryAdd"])

    try:
        json.loads(s)
    except json.JSONDecodeError as exc:
        raise SystemExit("patched file is not valid JSON: %s" % exc)

    tmp = REPORT + ".tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        fh.write(s)
    os.replace(tmp, REPORT)
    print("patched: %d bytes, %d lines" % (len(s), len(s.splitlines())))


if __name__ == "__main__":
    main()
