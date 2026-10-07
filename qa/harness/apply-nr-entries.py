#!/usr/bin/env python3
"""Add the 23 NR-* (no-root) paired check entries to qa/qa-report.json.

Stage-5 addendum for the "make it work without root" requirement.
Rules honoured here:
  * the existing 81 check entries and all their fields are left byte-identical;
  * only new entries are appended (checks, findings, checklistDiscoveries,
    evidence, stage5HarnessAdditions, unverifiable);
  * environment/verdictRule/summary are extended, never rewritten;
  * the file is re-serialised with the same indent/ensure_ascii conventions
    the report already uses (indent=1, ASCII-escaped unicode).

Run from the repo root:  python3 qa/harness/apply-nr-entries.py
"""
import json
import os
import sys

REPORT = "qa/qa-report.json"
TS = "2026-10-07T05:5xZ"

with open(REPORT, encoding="utf-8") as fh:
    d = json.load(fh)

before_checks = len(d["checks"])
before_findings = len(d["findings"])

# --------------------------------------------------------------------------
# 23 paired entries -- one per entry of the NR-* / MRG-02 block added by stage 1.
# semantics (same as the existing 81):
#   status  pass = what actually happened matches the checklist's expectation
#   verdict holds = the constraint text (including "finding" style "X is broken")
#                   is confirmed true / violated = the constraint text is disproved
# --------------------------------------------------------------------------
NR_CHECKS = [
    dict(
        id="Q300", constraint="NR-01",
        title="Ports above 1024 are bindable without privileges; the server's own sockets belong to uid 3000",
        action="server stopped, both ports free -> python3 SO_REUSEADDR bind on 8080 and 50051 as uid 3000; then start the server and read /proc/<pid>/status Uid + ss -ltnp",
        expected="both binds succeed; the server's euid/ruid = 3000/3000; both listeners are owned by pid of uid 3000",
        actual="bind 8080 ok as uid 3000; bind 50051 ok as uid 3000; Uid: 3000 3000 3000 3000; ss -ltnp: 0.0.0.0:8080 and 0.0.0.0:50051 both users:((\"clusterscope-se\",pid=1901587)) whose Uid is 3000; getcap prints nothing (no file capability) and CapEff = 0000000000000000 (zero effective privileges)",
        status="pass", verdict="holds",
        evidence=["no-root-verify-12-nr01-unprivileged-bind.txt",
                  "no-root-verify2-e-nr1-socket-ownership.txt"],
        note="tighter than the checklist: the checklist only proved uid for the bind probe; this entry also shows the real server process carries zero capabilities",
    ),
    dict(
        id="Q301", constraint="NR-02",
        title="README:84-86 documented server start path works as a plain user",
        action="sed 's#localhost:5432#127.0.0.1:5432#' deploy/server.yaml.example > <w>/server.yaml && ./target/release/clusterscope-server <w>/server.yaml",
        expected="8080 and 50051 listening, /api/health = 200, process owned by uid 3000",
        actual="PID 1894692 user=tianyuy user=uid 3000 cmd=target/release/clusterscope-server /tmp/nr-verify/server.yaml; two listeners (8080, 50051); health http_code=200",
        status="pass", verdict="holds",
        evidence=["no-root-verify-03-nr02-documented-start.txt",
                  "no-root-server.log"],
    ),
    dict(
        id="Q302", constraint="NR-03",
        title="agent default paths stay inside HOME",
        action="./target/release/clusterscope-agent --help; HOME=<tmp> timeout 6 agent --config-dir <tmp>/.config/clusterscope; find <tmp>",
        expected="5 override flags present; <tmp>/.config/clusterscope/logs created; nothing under /etc or /var",
        actual="5/5 override flags (--config --config-dir --server-addr --node-id --agent-token); created /tmp/nr-home/.config/clusterscope/logs; independent re-run also created <HOME>/.local/state/clusterscope-agent; zero writes outside HOME",
        status="pass", verdict="holds",
        evidence=["no-root-verify-05-nr06-silent-fallback.txt"],
        note="the checklist's claim that --config-dir also moves node_id_file is CONFIRMED, but see Q305/NF-01 for what happens when that parent dir does not exist",
    ),
    dict(
        id="Q303", constraint="NR-04",
        title="No system path is writable by uid 3000",
        action="touch probe file in /etc/clusterscope /var/lib/clusterscope /var/log/clusterscope-server /usr/local/bin; stat /usr/local/bin",
        expected="all four DENIED; /usr/local/bin = root:root 755",
        actual="all four refused (probe file could not be created); /usr/local/bin = root:root 755; /etc/clusterscope does not exist at all on this host (independent re-check: ls -ld /etc/clusterscope -> No such file or directory)",
        status="pass", verdict="holds",
        evidence=["no-root-verify-07-nr15-units-facts.txt"],
    ),
    dict(
        id="Q304", constraint="NR-05",
        title="systemd --user can install/enable/start/remove a unit without sudo",
        action="write ~/.config/systemd/user/nr-probe-unit.service; systemctl --user daemon-reload; enable --now; is-active; show FragmentPath; disable --now; rm",
        expected="Created symlink, is-active=active, FragmentPath under $HOME/.config/systemd/user",
        actual="PASS on both the author-script run and my re-run: FragmentPath=/public/tianyuyang/.config/systemd/user/nr-probe-unit.service, is-active=active, no sudo used, unit removed afterwards",
        status="pass", verdict="holds",
        evidence=["no-root-checks-author-script-rerun.txt"],
    ),
    dict(
        id="Q305", constraint="NR-06",
        title="CONTRADICTION: agent dies (exit 1) right after greeting when -c points at a missing file and $HOME/.config does not exist",
        action="env HOME=<fresh empty dir> timeout 8 ./target/release/clusterscope-agent -c /etc/clusterscope/agent.yaml ; echo exit=$?",
        expected="agent does not crash: config_loader.rs:9-11 is a silent fallback (checklist wording); the author-script's NR6 also recorded 'exit 124 = killed by timeout, so it kept running'",
        actual="exit=1. INFO ClusterScope Agent starting server_addr=http://localhost:50051 node_id=None (the silent fallback to defaults IS real) then: Error: Failed to write node identity to \"<HOME>/.config/node_id\" / Caused by: No such file or directory (os error 2). The same command with $HOME/.config pre-created survives (exit=124, keeps retrying localhost:50051). The failure message never mentions the missing config file (mentions=0). Strace-free proof: changing only whether $HOME/.config exists flips exit 1 -> 124 (see nr-verify4/d vs e in the evidence file).",
        status="pass", verdict="violated",
        evidence=["no-root-verify-05-nr06-silent-fallback.txt",
                  "no-root-verify2-a-agent-missing-config.txt",
                  "no-root-verify3-q1-missing-config-lifetime.txt"],
        deviation="The SQL/stage-1 evidence claimed the agent stays alive (author-script NR6 PASS, qa/no-root.qa.md NR6 'exit 124'). My re-run of that exact script prints NR6 PASS too, but the process had already exited by then: the check greps the greeting line, which is emitted ~0.4 s before the crash, and never inspects the exit status. Recorded as finding NF-01.",
    ),
    dict(
        id="Q306", constraint="NR-06b",
        title="agent log dir defaults to $HOME/.local/state/clusterscope-agent and is created",
        action="env HOME=<tmp> timeout 8 agent -c /etc/clusterscope/agent.yaml ; ls -ld <tmp>/.local/state/clusterscope-agent",
        expected="directory exists, owner is the invoking user",
        actual="directory created and owned by tianyuyang (uid 3000) in every variant; observed in the author-script run, in nr-verify V5 and in nr-verify2 A. Independent check of the /etc fallback: config.rs:38 is only reached when $HOME is unset, and even then the agent resolves its identity via the passwd home and keeps running (exit=124) - so /etc/clusterscope is not written to in practice.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-05-nr06-silent-fallback.txt",
                  "no-root-verify2-c-agent-no-home.txt"],
    ),
    dict(
        id="Q307", constraint="NR-07",
        title="Bare server refuses to start instead of running with a weak JWT secret",
        action="timeout 15 ./target/release/clusterscope-server ; echo exit=$?  (also: with a missing argv[1] config)",
        expected="stderr contains 'refusing to start: jwt_secret is missing/too weak with auth_required: true', exit 1 -> expected FAIL is the audit conclusion",
        actual="exit=1 exactly (the author-script never checked the exit code); stderr: 'Error: refusing to start: jwt_secret is missing/too weak with auth_required: true. Set a strong jwt_secret in server.yaml (or JWT_SECRET env), or set auth_required: false for trusted LANs.' Second guard found: argv[1] pointing at a missing file gives exit=1 'Error: Config file not found: <path>' (main.rs:193) - the server is louder than the agent about missing config.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-02-nr07-bare-server.txt",
                  "no-root-verify2-b-server-missing-config.txt"],
    ),
    dict(
        id="Q308", constraint="NR-08",
        title="CLI / environment variables can bypass every root-only path",
        action="grep the 6 env keys in crates/server/src/main.rs; agent --help; then run the agent with --config-dir/--server-addr/--node-id/--agent-token and see which address it dials; also run the server with env only",
        expected="server honours 6 env keys (each with a CLUSTERSCOPE_ variant), agent has 5 override flags, the effective address follows the override",
        actual="server: POSTGRES_URL / JWT_SECRET / HTTP_ADDR / GRPC_ADDR / AUTH_REQUIRED / AGENT_TOKEN (+ CLUSTERSCOPE_ prefix, main.rs:200-219); agent dialled http://127.0.0.1:58888 (the CLI value) instead of the default, and created <config-dir>/logs + <config-dir>/node_id; server started with env only and served health=200",
        status="pass", verdict="holds",
        evidence=["no-root-verify-05b-nr08-overrides.txt",
                  "no-root-verify-04-nr21-env-only.txt"],
    ),
    dict(
        id="Q309", constraint="NR-09",
        title="PostgreSQL: the compose promise is not executable here; the source-build route is",
        action="pg_ctl status; psql select version(); grep README docker compose; command -v docker/docker-compose; podman images; attempt the documented command; curl registry-1.docker.io",
        expected="PG 16.4 up and answering as uid 3000; README offers only docker compose; docker absent -> the promise does not hold in this environment",
        actual="pg_ctl exit=0 (PID 4176115, -k /tmp, listen 127.0.0.1); select version() = 'PostgreSQL 16.4 on x86_64-pc-linux-gnu, compiled by gcc (GCC) 11.5.0'; select current_user => clusterscope; README:87 'no root: docker compose up' (+ README:57 'one-click via deploy/docker-compose.yml' and README:337 listing docker-compose as shipped); docker absent, docker-compose absent, podman 5.6.0 with 0 images; the documented command itself: 'timeout: failed to run command podman-compose: Permission denied' / 'failed to run command docker: Permission denied'; no egress (curl to registry-1.docker.io fails); README has 0 hits for building PostgreSQL from source, while that is exactly how the working instance was produced",
        status="pass", verdict="holds",
        evidence=["no-root-verify-11-nr09-docker-vs-source-build.txt"],
        deviation="the checklist's classification table lists README:87 but not README:57 - a second, stronger compose claim that the same failure covers (see N12)",
    ),
    dict(
        id="Q310", constraint="NR-10",
        title="README: every no-root / systemctl --user mention reconciled line by line",
        action="grep -n '无需 root|无 root|root-not|systemctl --user' README.md; classify each line against the measured results",
        expected="9 lines hold; :87 (docker compose) and :288/:289 (systemctl --user ... clusterscope-server) do not hold",
        actual="13 lines matched (15, 21, 56, 57, 87, 89, 211, 288, 289, 290, 291, 293, 321): 10 hold, 3 do not (:57 one-click compose, :87 docker compose up, :288/:289 server user-unit management). :290/:291/:293/:321 hold because the machine-local user agent unit really is active (Q316). The earlier classification table covers 12 of the 13 lines (it omits :57).",
        status="pass", verdict="holds",
        evidence=["no-root-readme-claims.txt",
                  "no-root-verify-09-mrg02-and-redis.txt",
                  "no-root-verify-10-nr13-no-user-server-unit.txt"],
        deviation="stage-1 bookkeeping differs from the measured counts: constraints.json#NR-10 says '11 mentions' (grep finds 21 matches over 13 lines) and #NR-13 says 'README 命中 4 行 systemctl --user' (grep finds 5: 288/289/290/291/321). Conclusions unchanged; recorded as N10.",
    ),
    dict(
        id="Q311", constraint="NR-11",
        title="A user service survives an SSH teardown on this host (Linger=yes) - and the Linger=no branch cannot be reproduced here",
        action="loginctl show-user tianyuyang; install nr-persist-unit.service (ExecStart=/bin/sleep 180); enable --now; tear down a separate ssh session (ssh localhost 'exit 0'); kill -0 MainPID",
        expected="Linger=yes, State=active, MainPID still alive after the session ends",
        actual="Linger=yes, State=active; MainPID=1897321 alive after the session teardown; the pre-existing user agent (started 2026-09-02 12:59:26, PID 266643) has since outlived every session, which is production-grade evidence that the user-level agent service really does stay resident. The Linger=no branch was NOT reproduced: enable/disable-linger needs root and this host must not have its logind config changed (Leader's instruction); SEMANTICS recorded instead - without linger logind stops the whole per-user manager when the last session of that user ends, so all of its units die with it; install-agent.sh:60 only probes 'systemctl --user show-environment' and never checks Linger, so an installer on a no-linger node silently produces a service that dies at logout.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-15-nr11-linger-semantics.txt",
                  "no-root-verify-01-linger.txt"],
        environmentFact="Linger=yes on lyy-node03 is PROBABLY NOT the cluster default: the Leader first read Linger=no, then ran 'loginctl enable-linger tianyuyang' (no output) during early probing, and stage 1 read Linger=yes afterwards. I did not change it and cannot attribute it independently - the yes observed here is therefore a probe artefact, not evidence about a clean node. See the environment.linger note and unverifiable[].",
    ),
    dict(
        id="Q312", constraint="NR-12",
        title="nohup fallback starts the agent, but has no supervision and contains a pkill footgun",
        action="nohup clusterscope-agent -c <tmp>/agent.yaml & ; sleep 5; kill -0 -> yes; grep greeting; kill <pid>; sleep 3; kill -0 -> no; grep install-agent.sh",
        expected="starts and stays up, but after a manual kill the pid is gone and nothing restarts it (expected FAIL = conclusion)",
        actual="nohup_pid alive=yes, greeting_lines=1, after manual kill alive=no (nothing respawns it); Restart=always appears only inside the systemd --user branch of install-agent.sh (line 70), the nohup branch (81-83) has none. Extra finding: the nohup branch first runs 'pkill -f clusterscope-agent' (line 80), which kills every agent of that user including ones the installer did not start - the live PID 266643 on this host is exactly such a victim. Recorded as NF-02.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-16-nr12-nohup-fallback.txt"],
    ),
    dict(
        id="Q313", constraint="NR-13",
        title="The repo ships 0 user-level server units; README:288-289 can therefore only work on an already hand-configured host",
        action="grep -n 'systemctl --user' README.md; find . -name clusterscope-server.service -not -path './target/*' | wc -l; cat ~/.config/systemd/user/clusterscope-server.service; grep -r 'systemctl --user|\\.config/systemd' docs/ | wc -l",
        expected="README mentions user-level server management; 0 such units in the repo; the machine-local unit is hand-written; docs/ has none",
        actual="README: 5 lines (288, 289, 290, 291, 321); repo: 0 files named clusterscope-server.service and only deploy/server.service (system unit) + deploy/agent.service; docs/: 0 hits; the only file that mentions clusterscope-server.service anywhere in the repo is qa/ and GAUNTLET.md (the audit's own text), i.e. no shipped installer or doc creates it; the machine-local unit exists (mtime 2026-08-10 10:36:45, ExecStart=/public/tianyuyang/.local/bin/clusterscope-server /public/tianyuyang/.config/clusterscope/server.yaml, WantedBy=default.target, state=disabled/inactive) with an ExecStart into $HOME, i.e. hand-written, not a repo artefact. On a clean host 'systemctl --user restart clusterscope-server' has no unit to resolve.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-10-nr13-no-user-server-unit.txt"],
        deviation="the checklist says README has 4 systemctl --user lines; grep finds 5 (288/289/290/291/321 - the checklist's own table lists :293 as journalctl, but the grep pattern 'systemctl --user' does not match it; line 321 does). Conclusion unchanged (see N10).",
    ),
    dict(
        id="Q314", constraint="NR-14",
        title="deploy/install-agent.sh is a genuine user-level installer",
        action="bash -n deploy/install-agent.sh; grep user paths; grep non-comment system paths; grep nohup/systemctl --user",
        expected="SYNTAX-OK; >=3 user-path refs; 0 non-comment system-path refs; both branches present",
        actual="syntax OK; 10 user-path refs (~/.local/bin, ~/.config/clusterscope, ~/.config/systemd/user); 0 non-comment references to /usr/local/bin, /etc/clusterscope, /var/lib/clusterscope, /var/log/clusterscope; systemd --user branch at line 60 with Restart=always (70) and nohup fallback at 81-83",
        status="pass", verdict="holds",
        evidence=["no-root-checks-author-script-rerun.txt",
                  "no-root-verify-16-nr12-nohup-fallback.txt"],
    ),
    dict(
        id="Q315", constraint="NR-15",
        title="The two shipped units need root - with the exact systemd verdicts",
        action="cat deploy/*.service; id clusterscope; stat /usr/local/bin; systemctl link deploy/server.service; cp to /etc/systemd/system; systemctl --user start; systemd-analyze verify",
        expected="both units are system units (User=clusterscope, /usr/local/bin, /var/lib/clusterscope, multi-user.target); user clusterscope does not exist -> expected FAIL is the conclusion",
        actual="deploy/server.service: After=network.target postgresql.service redis.service, User/Group=clusterscope, ExecStart=/usr/local/bin/clusterscope-server /etc/clusterscope/server.yaml, WorkingDirectory=/var/lib/clusterscope, LogsDirectory=/var/log/clusterscope-server, WantedBy=multi-user.target; deploy/agent.service mirrors it. id clusterscope -> no such user; /usr/local/bin root:root 755. Installation attempts as uid 3000: 'systemctl link' -> 'Failed to link unit: Interactive authentication required.'; 'cp ... /etc/systemd/system/' -> 'Permission denied'; 'systemctl --user start <shipped unit>' -> 'Unit ...mount not found' (a system unit is not in the user manager); 'systemd-analyze verify' -> 'Command /usr/local/bin/clusterscope-server is not executable: No such file or directory' plus 'LogsDirectory= path is absolute, ignoring'. redis is referenced only by the unit (After=...redis.service): crates/** uses redis_url exactly twice, both in crates/common/src/config.rs (declaration line 66, default line 101 -> 'redis://localhost:6379'), never read; postgresql.service does not exist on this host either at /lib/systemd/system/postgresql.service (PG was built into $HOME).",
        status="pass", verdict="holds",
        evidence=["no-root-verify-07-nr15-units-facts.txt",
                  "no-root-verify-08-nr15-install-attempts.txt",
                  "no-root-verify-09-mrg02-and-redis.txt"],
        deviation="extra fact beyond the checklist: the units also pull 'After=postgresql.service', a distro unit that does not exist on a node running a HOME-local PostgreSQL - so even a root user on this cluster would chase a non-existent dependency (N11).",
    ),
    dict(
        id="Q316", constraint="NR-16",
        title="A user-level agent service is running in production on this node",
        action="systemctl --user is-active clusterscope-agent.service; show FragmentPath; ls ~/.local/bin + ~/.config/clusterscope; ps -u $(id -un) -o pid,lstart,cmd | grep clusterscope-agent",
        expected="active; FragmentPath under HOME; binary and config in HOME; process running",
        actual="is-active=active; FragmentPath=/public/tianyuyang/.config/systemd/user/clusterscope-agent.service; ExecStart=/public/tianyuyang/.local/bin/clusterscope-agent -c /public/tianyuyang/.config/clusterscope/agent.yaml; PID 266643 started Wed 2026-09-02 12:59:26 and still alive on 2026-10-07 13:5x (35 days), owner uid 3000 - the strongest available evidence that the user-level path works without root, and that it survives session churn (see Q311)",
        status="pass", verdict="holds",
        evidence=["no-root-verify-01-linger.txt",
                  "no-root-verify-16-nr12-nohup-fallback.txt"],
    ),
    dict(
        id="Q317", constraint="NR-17",
        title="No frontend in this line, so the frontend no-root re-check is N/A",
        action="ls web; find . -maxdepth 3 -name package.json -not -path './target/*' | wc -l",
        expected="No such file or directory; 0 package.json -> N/A (see FE-01)",
        actual="ls: web: No such file or directory; 0 package.json outside target/. Verified on the branch HEAD 24b5b0d. If the merge brings A's web/ in, M10 requires this re-run.",
        status="pass", verdict="holds",
        evidence=["no-root-checks-author-script-rerun.txt"],
    ),
    dict(
        id="Q318", constraint="NR-18",
        title="TUI renders under a pty as a plain user",
        action="server up (env-only) then TERM=xterm script -q -c 'timeout 8 clusterscope-tui -s http://127.0.0.1:8080' <pty file>",
        expected="pty capture non-empty, no panic",
        actual="pty 1185 bytes, panic hits 0; the capture contains 'connected to http://127.0.0.1:8080' and a full alternate-screen TUI frame; the author-script run reproduced 1185 bytes as well",
        status="pass", verdict="holds",
        evidence=["no-root-verify-13-nr18-tui-pty.txt"],
    ),
    dict(
        id="Q319", constraint="NR-19",
        title="A read-only HOME makes the agent fail hard at startup",
        action="mkdir <tmp>/ro-home; chmod 500; HOME=<tmp>/ro-home timeout 10 clusterscope-agent; echo exit=$?",
        expected="exit 1 with 'Failed to create log directory: ...' (expected FAIL = conclusion)",
        actual="exit=1; 'Error: Failed to create log directory: \"/tmp/nr-verify/ro-home/.local/state/clusterscope-agent\"' + 'Caused by: Permission denied (os error 13)'. No env or flag bypasses it: the log dir is a hard precondition in config_loader.rs:39-42. The server is unaffected because it logs to stderr only.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-06-nr19-readonly-home.txt"],
    ),
    dict(
        id="Q320", constraint="NR-20",
        title="GPU and disk telemetry is readable without privileges",
        action="nvidia-smi -L; cat /sys/class/drm/card0/device/power/runtime_status; cat /sys/block/nvme0n1/device/model",
        expected="all three succeed (6x L20 on this host)",
        actual="nvidia-smi -L lists GPU 0..5 as NVIDIA L20; runtime_status=active; nvme model 'IEIT NS6500G2U384'. /proc tightened as well: a plain user cannot read /proc/1/io ('Permission denied') but can read /proc/1/status and /proc/1/cmdline, which is exactly the pair the collector uses (crates/agent/src/metrics.rs:599-616, best-effort, documented 'never fails'); /proc mount has no hidepid option. A 25 s run with collect_process_details=true produced 0 panic/permission lines and the process stayed alive.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-14-nr20-nvml-sysfs.txt",
                  "no-root-verify2-d-proc-degradation.txt",
                  "no-root-verify3-q2-metrics-tick.txt"],
    ),
    dict(
        id="Q321", constraint="NR-21",
        title="The same binary starts with zero config files (environment only) and opens no root-only path",
        action="env POSTGRES_URL=... JWT_SECRET=... AUTH_REQUIRED=false ./target/release/clusterscope-server   (no argv[1]); curl /api/health; lsof -p <pid> | grep -cE '/etc/clusterscope|/var/(lib|log)/clusterscope'",
        expected="health=200; 0 hits under root-only directories",
        actual="argv is exactly the binary with no config argument; health http_code=200; lsof root-only hits count=0; /proc/<pid>/cwd -> the repo, /proc/<pid>/root -> /; the open-file list shows only the binary, libc/libm/libgcc/ld, /dev/null, the redirected log in /tmp, the DB socket pair to 127.0.0.1:5432 and the two listeners. Startup also logs 'No config file given - using defaults + environment overrides'.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-04-nr21-env-only.txt"],
        deviation="the author-script's NR21 measured the CONFIG-path server (start_server config), not the env-only command its own text describes. I re-ran the env-only variant explicitly; the claim holds, the script's evidence did not support it.",
    ),
    dict(
        id="Q322", constraint="MRG-02",
        title="Merge-plan fixtures for the M10 no-root invariants are all in place",
        action="grep -n '^| M10 ' qa/merge-plan-requirements.md; grep -c '^| NRM'; python3 -c count NR-* in qa/constraints.json; ls qa/no-root.qa.md",
        expected="M10 exists, >=6 NRM rows, all NR-* constraints present -> stage 6 must answer every NRM",
        actual="M10 at line 38; 8 NRM rows (NRM1-NRM8); 23 NR-* / MRG-02 constraints in qa/constraints.json (104 total, the original 81 unchanged); qa/no-root.qa.md present. The M10 acceptance wording ('only NR-13/NR-15 may FAIL') needs one amendment for the NR-06 result of Q305 - NF-01 shows a third expected-FAIL condition that is not a merge artefact.",
        status="pass", verdict="holds",
        evidence=["no-root-verify-09-mrg02-and-redis.txt"],
    ),
]

assert len(NR_CHECKS) == 23, len(NR_CHECKS)

# --------------------------------------------------------------------------
# New findings (product issues found by this addendum).
# --------------------------------------------------------------------------
NR_FINDINGS = [
    dict(
        id="NF-01", constraint="NR-06", severity="major",
        title="agent started with a missing -c file dies immediately (exit 1) when $HOME/.config is absent - and the no-root harness reports this as PASS",
        impact="The audit's no-root conclusion 'the agent does not crash when its config file is missing; it falls back silently and keeps running' is wrong for the very scenario the checklist constructs: a fresh user with a clean HOME. Copying a system unit's ExecStart (`-c /etc/clusterscope/agent.yaml`), or any deployment where $HOME/.config has not been created yet, gives a process that prints one INFO line and then exits 1 - no dashboard node, no metrics, no retry loop. In addition the operator is told nothing about the real cause: the error names $HOME/.config/node_id, never the missing config file.",
        rootCause="crates/agent/src/config_loader.rs:9-11 silently substitutes AgentConfig::default() when the -c path does not exist (server_addr=http://localhost:50051, node_id=None, node_id_file=$HOME/.config/node_id). Then config_loader.rs:40 creates only the log dir ($HOME/.local/state/clusterscope-agent) - it does not create the parent of node_id_file. crates/agent/src/node_identity.rs:27 then fails with 'Failed to write node identity to \"...\"/No such file or directory (os error 2)' and the process exits 1. Note the asymmetry with the server: crates/server/src/main.rs:193 bails with 'Config file not found: <path>', so the two binaries disagree about missing config files.",
        repro=[
            "cd /public/tianyuyang/code/ClusterScope-review/gh-line",
            "H=/tmp/nr-f01; rm -rf $H; mkdir -p $H",
            "env HOME=$H timeout 8 ./target/release/clusterscope-agent -c /etc/clusterscope/agent.yaml; echo exit=$?   # -> exit=1",
            "mkdir -p $H/.config && env HOME=$H timeout 8 ./target/release/clusterscope-agent -c /etc/clusterscope/agent.yaml; echo exit=$?   # -> exit=124 (survives)",
            "env HOME=$H ./target/release/clusterscope-agent -c /tmp/definitely-absent.yaml 2>&1 | tail -3   # error names node_id, not the config file",
        ],
        evidence=["no-root-verify-05-nr06-silent-fallback.txt",
                  "no-root-verify2-a-agent-missing-config.txt",
                  "no-root-verify3-q1-missing-config-lifetime.txt"],
        harnessNote="qa/harness/no-root-checks.sh:171-177 decides NR6 with `grep -q 'ClusterScope Agent starting'` and never reads the agent's exit status; the greeting line is printed before the crash, so the check passed while the process was already dead. Its own doc (qa/no-root.qa.md NR6) records 'exit 124 = killed by timeout' because a later `$?` in that file corresponds to the grep, not to the agent. Fix the check to assert the exit status (124 under timeout = still running) before changing any conclusion.",
        verdictNote="This does NOT weaken NR-06b (the log dir really is created, Q306) and it does not affect the systemd --user path from install-agent.sh, which pre-creates ~/.config/clusterscope (lines 30/43) and therefore survives - see nr-verify4 variant e.",
    ),
    dict(
        id="NF-02", constraint="NR-12", severity="major",
        title="install-agent.sh's nohup fallback starts with `pkill -f clusterscope-agent`, killing every agent of that user",
        impact="On a shared cluster (this very host) the fallback path of the documented installer terminates agents it did not start - including the resident production agent (PID 266643, running since 2026-09-02). The audit itself had to adopt a 'never pkill' rule (qa/README.md rule 1) precisely because of this; the shipped script violates it. A second agent instance also silently competes with the first for the same node_id, which README:298 warns against ('one agent per machine').",
        rootCause="deploy/install-agent.sh:80 `pkill -f clusterscope-agent 2>/dev/null || true` inside the else-branch (no systemd --user available). The pattern matches the absolute path of any clusterscope-agent process of the same uid, not just the one this installer would write to ~/.local/bin.",
        repro=[
            "grep -n 'pkill' /public/tianyuyang/code/ClusterScope-review/gh-line/deploy/install-agent.sh   # -> 80: pkill -f clusterscope-agent",
            "ps -u $(id -un) -o pid,lstart,cmd | grep clusterscope-agent | grep -v grep   # -> PID 266643 since 2026-09-02 (would be killed)",
            "sed -n '79,84p' deploy/install-agent.sh   # the branch that runs it",
        ],
        evidence=["no-root-verify-16-nr12-nohup-fallback.txt"],
        containment="Not executed by this audit - running it would have killed the resident agent. Recorded from source + a live process listing.",
    ),
]

# --------------------------------------------------------------------------
# Checklist/bookkeeping discoveries for the no-root block (QA-package notes).
# --------------------------------------------------------------------------
NR_DISCOVERIES = [
    dict(
        id="N10", severity="minor",
        title="NR-10/NR-13 line accounting drifts from the measured grep",
        detail="constraints.json#NR-10 says README has '11 mentions' of no-root/systemctl --user; the grep in the check finds 21 matches over 13 distinct lines (15, 21, 56, 57, 87, 89, 211, 288, 289, 290, 291, 293, 321). #NR-13 says 'README 命中 4 行 systemctl --user'; the actual grep finds 5 (288, 289, 290, 291, 321). The author-script's own asserted count ('README has 11 mentions') agrees with the constraint text, not with the grep - it is a threshold (>=11), not the measured number. Conclusions (which lines hold) are unaffected; Q310 lists the corrected line set.",
        evidence="no-root-verify-10-nr13-no-user-server-unit.txt / no-root-readme-claims.txt",
    ),
    dict(
        id="N11", severity="info",
        title="The shipped system units also depend on postgresql.service, which does not exist on a HOME-PostgreSQL node",
        detail="deploy/server.service:3 `After=network.target postgresql.service redis.service`. redis is unused by the code (redis_url is declared at crates/common/src/config.rs:66 and defaulted at :101, never read). postgresql.service is a distro unit name; on this cluster PostgreSQL 16.4 was compiled into HOME (no systemd unit at all: /lib/systemd/system/postgresql.service absent). So the system-unit path is doubly unworkable here: uninstallable without root (NR-15) and dependent on services this cluster does not run.",
        evidence="no-root-verify-09-mrg02-and-redis.txt / no-root-verify-07-nr15-units-facts.txt",
    ),
    dict(
        id="N12", severity="minor",
        title="README has a second docker promise (line 57) that the NR-10 table does not classify",
        detail="README:57 'PostgreSQL | v16+(server 必需;可用 deploy/docker-compose.yml 一键起)' and README:337 (deploy/ listing includes docker-compose) make the same promise as :87 but more strongly, and neither is in the stage-1 classification table. Both fail for the same reason (no docker/docker-compose, no egress) - the failure mode is already recorded, only the per-line reconciliation (NR-10) was incomplete.",
        evidence="no-root-verify-11-nr09-docker-vs-source-build.txt / no-root-verify-09-mrg02-and-redis.txt",
    ),
    dict(
        id="N13", severity="info",
        title="The no-root harness records evidence in gauntlet-out/ (gitignored) and deletes its per-check logs",
        detail="qa/harness/no-root-checks.sh writes to $R/gauntlet-out/qa/evidence and its cleanup trap removes /tmp/nr-checks/*.log, so the raw logs behind each NR line (agent-default-path.log, server-bare.log, ...) do not survive the run: only the one-line PASS/FAIL summary and no-root-server.log remain. Reproducibility therefore rests on re-running the script, not on archived raw output. This addendum's harness (qa/harness/nr-verify*.sh) writes its raw output to qa/evidence/ instead, which is committed.",
        evidence="qa/harness/no-root-checks.sh:28-38,61-64",
    ),
]

# --------------------------------------------------------------------------
NR_EVIDENCE = [
    "qa/evidence/no-root-verify-00-identity.txt",
    "qa/evidence/no-root-verify-01-linger.txt",
    "qa/evidence/no-root-verify-02-nr07-bare-server.txt",
    "qa/evidence/no-root-verify-03-nr02-documented-start.txt",
    "qa/evidence/no-root-verify-04-nr21-env-only.txt",
    "qa/evidence/no-root-verify-05-nr06-silent-fallback.txt",
    "qa/evidence/no-root-verify-05b-nr08-overrides.txt",
    "qa/evidence/no-root-verify-06-nr19-readonly-home.txt",
    "qa/evidence/no-root-verify-07-nr15-units-facts.txt",
    "qa/evidence/no-root-verify-08-nr15-install-attempts.txt",
    "qa/evidence/no-root-verify-09-mrg02-and-redis.txt",
    "qa/evidence/no-root-verify-10-nr13-no-user-server-unit.txt",
    "qa/evidence/no-root-verify-11-nr09-docker-vs-source-build.txt",
    "qa/evidence/no-root-verify-12-nr01-unprivileged-bind.txt",
    "qa/evidence/no-root-verify-13-nr18-tui-pty.txt",
    "qa/evidence/no-root-verify-14-nr20-nvml-sysfs.txt",
    "qa/evidence/no-root-verify-15-nr11-linger-semantics.txt",
    "qa/evidence/no-root-verify-16-nr12-nohup-fallback.txt",
    "qa/evidence/no-root-verify2-a-agent-missing-config.txt",
    "qa/evidence/no-root-verify2-b-server-missing-config.txt",
    "qa/evidence/no-root-verify2-c-agent-no-home.txt",
    "qa/evidence/no-root-verify2-d-proc-degradation.txt",
    "qa/evidence/no-root-verify2-e-nr1-socket-ownership.txt",
    "qa/evidence/no-root-verify3-q1-missing-config-lifetime.txt",
    "qa/evidence/no-root-verify3-q2-metrics-tick.txt",
    "qa/evidence/no-root-checks-author-script-rerun.txt",
]

NR_HARNESS = [
    dict(
        file="qa/harness/nr-verify.sh",
        purpose="Adversarial re-derivation of NR1-NR21 from a clean state with tighter assertions than no-root-checks.sh: NR7 exit code, NR1 socket ownership + CapEff, NR21 env-only (not config-path) server, agent config fallback with a writable HOME, the exact systemd verdicts for installing a system unit, README line classification, docker vs source-built PostgreSQL. Raw output is written to qa/evidence/ and committed.",
        note="Written by stage 5 (QA addendum). Does not modify any existing script, constraint or product file; only kills PIDs it started.",
    ),
    dict(
        file="qa/harness/nr-verify2.sh",
        purpose="Round 2 of the same re-verification: the cases round 1 exposed as under-specified - missing-config fallback end to end, server vs agent disagreement on a missing config, HOME unset, /proc read limits, and socket ownership of the server's own listeners.",
        note="Same contract. Evidence committed under qa/evidence/no-root-verify2-*.txt",
    ),
    dict(
        file="qa/harness/nr-verify3.sh",
        purpose="Round 3 (decisive experiment for NF-01): reproduces the author-script's exact NR6 command and reads the agent's exit status directly, then flips only the existence of $HOME/.config; plus a 25 s metrics tick to exercise NVML/process collection.",
        note="Same contract. Evidence committed under qa/evidence/no-root-verify3-*.txt",
    ),
]

NR_UNVERIFIABLE = [
    dict(
        constraint="NR-11",
        item="'a user service dies at logout on a Linger=no node' measured directly",
        blockedBy="enable/disable-linger needs root and changing it is a cluster-level change that this stage must not make (Leader's instruction). Recorded instead: the systemd-logind semantics, the fact that install-agent.sh never checks Linger (line 60), and Linger=yes on this host - which is itself probably a stage-0 probe artefact (Leader read Linger=no first, then ran `loginctl enable-linger tianyuyang`; stage 1 read yes afterwards).",
    ),
]

# --------------------------------------------------------------------------
# Apply
# --------------------------------------------------------------------------
existing_ids = {c.get("id") for c in d["checks"]}
for c in NR_CHECKS:
    assert c["id"] not in existing_ids, f"duplicate check id {c['id']}"
d["checks"].extend(NR_CHECKS)

existing_fids = {f.get("id") for f in d["findings"]}
for f in NR_FINDINGS:
    assert f["id"] not in existing_fids, f"duplicate finding id {f['id']}"
d["findings"].extend(NR_FINDINGS)

disc_ids = {x.get("id") for x in d["checklistDiscoveries"]}
for x in NR_DISCOVERIES:
    assert x["id"] not in disc_ids, f"duplicate discovery id {x['id']}"
d["checklistDiscoveries"].extend(NR_DISCOVERIES)

d["evidence"].extend(e for e in NR_EVIDENCE if e not in d["evidence"])
d["stage5HarnessAdditions"].extend(NR_HARNESS)
d["unverifiable"].extend(NR_UNVERIFIABLE)

d["environment"]["linger"] = (
    "Linger=yes on lyy-node03, State=active, RuntimePath=/run/user/3000 - and this is PROBABLY NOT "
    "the cluster default: the Leader first read Linger=no, then ran `loginctl enable-linger tianyuyang` "
    "(no output) during early probing, and stage 1 read Linger=yes afterwards. This stage neither changed "
    "it nor can attribute it independently, so treat the yes as a probe artefact of this machine, not as "
    "evidence about a clean node. Consequence: the NR-11 'service survives logout' result is valid for "
    "this host as it stands today, and the complementary 'Linger=no kills the service at logout' path "
    "could NOT be reproduced here (no root, and cluster-level config must not be touched)."
)
d["environment"]["noRootVerification"] = (
    "Re-run from a clean state on 2026-10-07T05:52Z by stage 5: `sh qa/harness/no-root-checks.sh` "
    "(script sha256 c5fb868f..., 439 lines) -> PASS=23 FAIL=0 exit 0, matching the author's claim, and "
    "then independently re-derived with qa/harness/nr-verify{,2,3}.sh (tighter assertions, raw evidence "
    "committed to qa/evidence/). Binaries: sha256 server 95b89cd4..., agent c7633c5a..., tui 1d0ba845... "
    "(target/release, 2026-10-07 03:09). Two results deviate from the stage-1 write-up: NF-01 (the agent "
    "dies with exit 1 on a missing -c file in a clean HOME; the harness's NR6 PASS is a false positive) "
    "and the NR-21 evidence gap (the script measured the config-path server while asserting the env-only "
    "one). Everything else reproduces as claimed."
)

d["verdictRule"] = (
    d["verdictRule"].rstrip(". ")
    + ". Addendum (stage 5, no-root dimension): the same rule is applied to the 23 NR-*/MRG-02 entries "
      "added by stage 1 (NR-01..NR-21, NR-06b, MRG-02), for a total of 104 constraints. Two of the 23 "
      "reproduce a condition the checklist did not anticipate and are therefore recorded as "
      "verdict=violated with a new finding (NF-01: NR-06; the harness's own NR6 PASS is a false "
      "positive). The pre-existing 81 entries are untouched and still pass as before."
)

d["summary"] = (
    d["summary"].rstrip(". ")
    + ". Stage-5 addendum for the 'no root' requirement: the 23 NR-*/MRG-02 constraints were re-run "
      "from a clean state and independently re-derived on the real release binaries; the author-script's "
      "23/23 PASS reproduces, but two of its conclusions do not survive adversarial checking - the agent "
      "exits 1 (not 124) on a missing -c file in a clean HOME (new finding NF-01, harness false positive) "
      "and its NR-21 evidence measured the config-path server rather than the env-only one. New findings: "
      "NF-01, NF-02 (install-agent.sh:80 pkill). Neither the shipped system units nor the hand-written "
      "user unit change the central result: server, agent and TUI really do run as uid 3000 with zero "
      "capabilities and zero writes outside HOME."
)

if len(sys.argv) > 1 and sys.argv[1] == "--dry-run":
    print("checks %d -> %d, findings %d -> %d" %
          (before_checks, len(d["checks"]), before_findings, len(d["findings"])))
    sys.exit(0)

if len(sys.argv) > 1 and sys.argv[1] == "--blocks":
    # Stage the new content for qa/harness/patch-report.py, which edits the report as
    # TEXT so that none of the pre-existing bytes change. This path does NOT write the
    # report.
    blocks = {
        "checks": NR_CHECKS,
        "findings": NR_FINDINGS,
        "discoveries": NR_DISCOVERIES,
        "unverifiable": NR_UNVERIFIABLE,
        "evidence": NR_EVIDENCE,
        "harness": NR_HARNESS,
        "extraNewChecks": [dict(
            id="Q331",
            title="no-root 维度的 23 条 NR-* 配对 + 对抗性复跑（含 2 条与 stage-1 结论不同的新发现）",
            actual=("sh qa/harness/no-root-checks.sh -> PASS=23 FAIL=0 exit 0（复跑两次，脚本 sha256 c5fb868f...），"
                    "再用 qa/harness/nr-verify{,2,3}.sh 逐条独立复推；23/23 在 qa-report.json 里配对成 Q300-Q322"
                    "（constraints 闸门实测 met）。新发现 NF-01（agent 在干净 HOME + 缺失 -c 文件下 exit 1，"
                    "harness 的 NR6 PASS 是假阳性）与 NF-02（install-agent.sh:80 的 pkill 会杀掉同用户所有 agent）"),
            status="pass",
            evidence=("qa/evidence/no-root-checks-author-script-rerun.txt + "
                      "qa/evidence/no-root-verify*.txt"),
        )],
        "reproduce": [
            "sh qa/harness/no-root-checks.sh                     # no-root 全量：PASS=23 FAIL=0（第 5 阶段增补复跑两次）",
            "sh qa/harness/nr-verify.sh && sh qa/harness/nr-verify2.sh && sh qa/harness/nr-verify3.sh   # 对抗性独立复推，证据落 qa/evidence/",
        ],
        "verdictRuleAdd": (
            "；增补（第 5 阶段，no-root 维度）：同一条规则适用于第 1 阶段新增的 23 条 NR-*/MRG-02"
            "（NR-01…NR-21、NR-06b、MRG-02），合计 104 条；23 条全部配对（Q300-Q322/Q330）。"
            "其中 1 条复现出的实际行为与清单预期不一致（NR-06：agent 在干净 HOME + 缺失 -c 文件时 exit 1，"
            "清单写的是静默回退不崩），按本规则记 verdict=violated 并新增 finding NF-01；"
            "原有 81 条一个字段都没改。"),
        "summaryAdd": (
            "增补（第 5 阶段 no-root 维度）：23 条 NR-* 约束在真实 release 二进制的干净状态上复跑并逐条独立复推；"
            "作者脚本的 23/23 PASS 可复现，但其中两条结论经不起对抗性检查——agent 在干净 HOME + 缺失 -c 文件时是 "
            "exit 1（不是「被 timeout 收掉」；harness 的 NR6 PASS 是假阳性，判据取的是 grep 的退出码），"
            "以及 NR-21 的证据量的是配置路径而非 env-only 那条（补齐后成立）。新增 finding：NF-01、"
            "NF-02（install-agent.sh:80 的 pkill）。核心结论不变：server/agent/TUI 确实能以 uid 3000、"
            "零 capability、零 HOME 外写入跑起来。"),
        "environmentKeys": {
            "linger": (
                "Linger=yes on lyy-node03（State=active，RuntimePath=/run/user/3000）——这很可能不是集群默认值："
                "Leader 前期探测先读到 Linger=no，随后执行过一次 `loginctl enable-linger tianyuyang`（无输出），"
                "第 1 阶段再读到 yes。本阶段既没有改它、也无法独立归因，因此应把这里的 yes 视为本机的探测产物，"
                "而不是干净节点的证据。后果：NR-11「用户服务活过登出」只对本机当前状态成立，"
                "「linger=no 时服务随会话结束而死」这条路径在本机无法复现（无 root，且不允许改集群级配置）。"),
            "noRootVerification": (
                "第 5 阶段增补（2026-10-07T05:52Z 起，干净状态）：`sh qa/harness/no-root-checks.sh` "
                "-> PASS=23 FAIL=0 exit 0（与作者自述一致，复跑两次），随后用 qa/harness/nr-verify{,2,3}.sh "
                "对 23 条逐条独立复推（原始输出已提交到 qa/evidence/）。二进制 sha256：server 95b89cd4...、"
                "agent c7633c5a...、tui 1d0ba845...（target/release，2026-10-07 03:09）。"
                "两条与 stage-1 写法不同，已如实记录为 finding：NF-01（agent 在干净 HOME 下遇缺失 -c 文件是 exit 1，"
                "不是「不崩」；harness 的 NR6 PASS 是假阳性）与 NR-21 的证据缺口（脚本量的是配置路径的 server，"
                "断言写的却是 env-only 那条；补齐后成立）。其余全部复现。"),
        },
        "mergePlanM10": d["mergePlanFacts"].get("M10_noRootInvariants_stage5", {}),
    }
    # stage-6 facing M10 harvest (same content as apply-nr-entries2.py installs)
    blocks["mergePlanM10"] = {
        "note": ("Measurements taken by the stage-5 addendum so stage 6 can answer M10 (NRM1-NRM8) with numbers. "
                 "Commands and raw output: qa/evidence/no-root-verify-17-merge-invariants.txt ; paired entries "
                 "Q300-Q322 and Q330."),
        "NRM1_runtime_unprivileged": ("holds (Q300/Q301/Q302/Q306/Q318/Q320): uid 3000, CapEff=0, listeners owned "
                                      "by uid 3000, TUI renders in a pty"),
        "NRM2_zero_config": ("holds for the server (Q321: argv=[binary] only, health=200, lsof root-only hits=0). "
                             "For the agent the stage-1 wording needs the NF-01 correction: the silent fallback is "
                             "real, but with a clean $HOME the agent then dies at 'Failed to write node identity' "
                             "(exit 1) - see Q305/NF-01"),
        "NRM3_hardcoded_root_paths": ("3 sites, all overridable defaults, same as the pre-audit baseline "
                                      "(crates/server/src/main.rs:186, crates/agent/src/main.rs:15, "
                                      "crates/common/src/config.rs:38). No new site was introduced by this audit"),
        "NRM4_tree_B_12_files": ("not evaluable on the gh-line tree - those 12 files are uncommitted in "
                                 "/public/tianyuyang/code/ClusterScope and must be re-checked on the merged branch: "
                                 "for f in <12 files>; do git diff -- \"$f\" | grep -nE "
                                 "'/etc/|/var/lib|/var/log|/usr/local|pre_exec|setuid|setgid|chown|CAP_'; done"),
        "NRM5_privilege_primitives": ("0 real ones. One textual pre_exec hit (crates/agent/src/job_executor.rs:128, "
                                      "libc::setsid() for the job process group) - no setuid/setgid/pkexec/sudo/chown "
                                      "anywhere. NB: the grep pattern printed in merge-plan-requirements.md is "
                                      "escaped, so it matches nothing at all (N14) - do not cite its 0"),
        "NRM6_deployment_dichotomy": ("unchanged and both halves proven: deploy/{server,agent}.service are system "
                                      "units uid 3000 cannot install (Q315: 'Failed to link unit: Interactive "
                                      "authentication required.', cp -> Permission denied), while "
                                      "deploy/install-agent.sh is a real user-level installer (Q314). README:288-289 "
                                      "still hands out user-level server management with 0 shipped user units (Q313)"),
        "NRM7_doc_consistency": ("README:87 (and the un-classified :57) docker-compose promise still fails here: no "
                                 "docker, no docker-compose, podman with 0 images, no egress (Q309). The working "
                                 "source-built PG 16.4 route is still undocumented (0 hits for 'from source')"),
        "NRM8_reproducible_proof": ("after the merge: sh qa/harness/no-root-checks.sh (currently PASS=23 FAIL=0, "
                                    "reproduced twice) plus nr-verify.sh / nr-verify2.sh / nr-verify3.sh. Accept only "
                                    "expected-FAIL conditions that are already dispositioned (NR-13/NR-15) and add "
                                    "the NR-06/NF-01 condition to that list"),
    }
    with open("/tmp/nr-blocks.json", "w", encoding="utf-8") as fh:
        json.dump(blocks, fh, ensure_ascii=False, indent=1)
    print("staged %d checks, %d findings, %d discoveries into /tmp/nr-blocks.json"
          % (len(blocks["checks"]), len(blocks["findings"]), len(blocks["discoveries"])))
    sys.exit(0)

tmp = REPORT + ".tmp"
with open(tmp, "w", encoding="utf-8") as fh:
    json.dump(d, fh, ensure_ascii=True, indent=1)
    fh.write("\n")
os.replace(tmp, REPORT)
print("checks %d -> %d, findings %d -> %d" %
      (before_checks, len(d["checks"]), before_findings, len(d["findings"])))
