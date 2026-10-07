#!/bin/sh
# nr-verify.sh -- adversarial re-verification of the no-root dimension (NR1..NR21).
#
# Independent of qa/harness/no-root-checks.sh (written by the specifier): this script
# re-derives every NR claim from scratch with *tighter* assertions where the original
# was looser, and records raw evidence under qa/evidence/.
#
#   cd /public/tianyuyang/code/ClusterScope-review/gh-line
#   sh qa/harness/nr-verify.sh
#
# Contract (qa/README.md rule 1): never kill a process we did not start.
# Every process started here is tracked by PID and killed by PID only.

# 树定位（2026-10-07 M6 合流轮修正）：此前硬编码 gh-line，在别的工作树里跑会静默测旧树。
# 现在按脚本自身位置解析，可用环境变量覆盖（R=<另一个树> sh qa/harness/nr-verify.sh）。
R="${R:-$(cd "$(dirname "$0")/../.." && pwd)}"
BIN="$R/target/release"
EV="$R/qa/evidence"
W=/tmp/nr-verify
PG=/public/tianyuyang/code/ClusterScope-review/pg16/bin
PGDATA=/public/tianyuyang/code/ClusterScope-review/pgdata
CONN='postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope'

mkdir -p "$EV" "$W" || exit 1
SRV=
cleanup() {
    [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
    sleep 1
    [ -n "$SRV" ] && kill -9 "$SRV" 2>/dev/null
    chmod 700 "$W"/*-home-ro 2>/dev/null
    rm -f "$W"/nr-persist-unit.service
}
trap cleanup EXIT INT TERM

say() { echo "----- $*"; }

# start_env_only_server: NR-21 path -- no argv[1], environment only.
start_env_only_server() {
    POSTGRES_URL="$CONN" JWT_SECRET='nr-verify-secret-0123456789abcdef' AUTH_REQUIRED=false \
        "$BIN/clusterscope-server" >> "$W/server-env.log" 2>&1 &
    SRV=$!
    i=0
    while [ "$i" -lt 25 ]; do
        curl -s -o /dev/null --max-time 2 http://127.0.0.1:8080/api/health && break
        i=$((i + 1)); sleep 1
    done
}
stop_server() {
    [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
    sleep 2
    [ -n "$SRV" ] && kill -9 "$SRV" 2>/dev/null
    SRV=
}

# ===========================================================================
say "V0 identity / provenance"
{
    echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host=$(hostname) user=$(id -un) uid=$(id -u) groups=$(id -Gn)"
    echo "git_head=$(git -C "$R" rev-parse HEAD)"
    echo "git_status=[$(git -C "$R" status --porcelain | tr '\n' ';')]"
    echo "script_sha256=$(sha256sum "$R/qa/harness/no-root-checks.sh" | cut -d' ' -f1)"
    echo "binary_sha256_server=$(sha256sum "$BIN/clusterscope-server" | cut -d' ' -f1)"
    echo "binary_sha256_agent=$(sha256sum "$BIN/clusterscope-agent" | cut -d' ' -f1)"
    echo "binary_sha256_tui=$(sha256sum "$BIN/clusterscope-tui" | cut -d' ' -f1)"
} | tee "$EV/no-root-verify-00-identity.txt"

# ===========================================================================
say "V1 linger: per-machine fact and who made it yes"
{
    echo "== loginctl show-user $(id -un)"
    loginctl show-user "$(id -un)" | grep -E '^(Linger|State|Sessions|Display)='
    echo "== enable-linger is a root-only operation (no sudo available here)"
    command -v sudo >/dev/null 2>&1 && echo "sudo binary present" || echo "no sudo binary on PATH"
    echo "== runtime dir owner (created by the user manager when a session/logind user exists)"
    ls -ld "/run/user/$(id -u)" 2>&1
    echo "== persistence of the pre-existing user service (author-run instance)"
    systemctl --user show clusterscope-agent.service -p ActiveEnterTimestamp -p MainPID -p FragmentPath --value | tr '\n' ' '
    echo
} | tee "$EV/no-root-verify-01-linger.txt"

# ===========================================================================
say "V2 NR7: bare server -- exit code AND stderr (script only grepped stderr)"
timeout 15 "$BIN/clusterscope-server" > "$W/bare.log" 2>&1
BARE_EXIT=$?
{
    echo "exit=$BARE_EXIT"
    echo "== stderr+stdout"
    cat "$W/bare.log"
} | tee "$EV/no-root-verify-02-nr07-bare-server.txt"

# ===========================================================================
say "V3 NR2 + NR4: documented start path, listener ownership, /proc/1/cwd alias"
sed 's#localhost:5432#127.0.0.1:5432#' "$R/deploy/server.yaml.example" > "$W/server.yaml"
echo "== config used (server: $(grep -n '^server:' -A3 "$W/server.yaml" | head -5 | tr '\n' ' '))"
"$BIN/clusterscope-server" "$W/server.yaml" >> "$W/server-cfg.log" 2>&1 &
SRV=$!
i=0
while [ "$i" -lt 25 ]; do
    [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 http://127.0.0.1:8080/api/health)" = "200" ] && break
    i=$((i + 1)); sleep 1
done
{
    echo "== process identity of the running server (must be uid $(id -u))"
    ps -o pid,user,uid,cmd -p "$SRV"
    echo "== listeners owned by it"
    ss -ltnp 2>/dev/null | grep -E ':(8080|50051)\b' || ss -ltn | grep -E ':(8080|50051)'
    echo "== health"
    curl -s -o /dev/null -w 'http_code=%{http_code}\n' --max-time 4 http://127.0.0.1:8080/api/health
    echo "== for reference: both ports are >1024 (no CAP_NET_BIND_SERVICE needed)"
    echo "8080 > 1024: yes; 50051 > 1024: yes"
} | tee "$EV/no-root-verify-03-nr02-documented-start.txt"

# ===========================================================================
say "V4 NR21: env-only start, lsof must show 0 root-only files (script measured the config-path server)"
stop_server
: > "$W/server-env.log"
start_env_only_server
{
    echo "== argv of the env-only server (no config file argument)"
    tr '\0' ' ' < "/proc/$SRV/cmdline"; echo
    echo "== health=http_code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 4 http://127.0.0.1:8080/api/health)"
    echo "== lsof root-only path hits:"
    lsof -p "$SRV" 2>/dev/null | grep -E '/etc/clusterscope|/var/(lib|log)/clusterscope' | tee "$W/envroot.txt"
    echo "count=$(wc -l < "$W/envroot.txt")"
    echo "== cwd/root of the process (both must be the repo, not a system dir)"
    ls -l "/proc/$SRV/cwd" "/proc/$SRV/root" 2>&1
    echo "== full open-file list (first 25, for the reviewer)"
    lsof -p "$SRV" 2>/dev/null | head -25
} | tee "$EV/no-root-verify-04-nr21-env-only.txt"

# ===========================================================================
say "V5 NR6/NR6b + silent fallback: which server_addr does the agent actually use?"
# A config file that DOES exist, with an unmistakable log_dir and a bogus server addr.
mkdir -p "$W/ctl/logs"
cat > "$W/ctl/agent.yaml" <<CTL
server_addr: "http://127.0.0.1:59999"
node_id: "nr-verify-control"
log_dir: $W/ctl/logs
log_level: info
CTL
env HOME="$W/ctl-home" timeout 6 "$BIN/clusterscope-agent" -c "$W/ctl/agent.yaml" > "$W/ctl.log" 2>&1
: > "$W/missing.log"
env HOME="$W/missing-home" timeout 8 "$BIN/clusterscope-agent" -c "$W/does-not-exist.yaml" > "$W/missing.log" 2>&1
MISS_EXIT=$?
{
    echo "== control: existing config (server_addr=127.0.0.1:59999) -> which addr does the agent dial?"
    grep -o 'http://[0-9.]*:[0-9]*' "$W/ctl.log" | sort -u | sed 's/^/  control: /'
    echo "== silent fallback: -c $W/does-not-exist.yaml (missing) -> exit=$MISS_EXIT"
    echo "  stderr/stdout (full):"
    sed 's/^/    /' "$W/missing.log"
    echo "  which addr does the agent dial?"
    grep -o 'http://[0-9.]*:[0-9]*' "$W/missing.log" | sort -u | sed 's/^/  missing: /'
    echo "  greeting line: $(grep -m1 'ClusterScope Agent starting' "$W/missing.log" || echo NONE)"
    echo "  log dir created under HOME:"
    find "$W/missing-home" -maxdepth 5 | sed 's/^/    /'
    echo "  any warning about the missing file? (grep -iE 'config|not found|missing|warn'):"
    grep -inE 'config|not found|missing|warn|error' "$W/missing.log" | sed 's/^/    /' || echo "    (none)"
} | tee "$EV/no-root-verify-05-nr06-silent-fallback.txt"

say "V5b override matrix: CLI / env precedence over the (silent) default"
env HOME="$W/ovr-home" timeout 6 "$BIN/clusterscope-agent" --config-dir "$W/ovr-cfgdir" --server-addr http://127.0.0.1:58888 --node-id nr-verify-cli --agent-token tok-verify > "$W/ovr.log" 2>&1
{
    echo "== --server-addr/--node-id/--agent-token/--config-dir given; which addr is dialled?"
    grep -o 'http://[0-9.]*:[0-9]*' "$W/ovr.log" | sort -u | sed 's/^/  /'
    echo "== directories created (log_dir must follow --config-dir)"
    find "$W/ovr-home" "$W/ovr-cfgdir" -maxdepth 4 2>/dev/null | sed 's/^/  /'
    echo "== node_id file honoured?"
    ls -l "$W/ovr-cfgdir" 2>/dev/null | sed 's/^/  /'
} | tee "$EV/no-root-verify-05b-nr08-overrides.txt"

# ===========================================================================
say "V6 NR19: read-only HOME -> exact exit code and message"
mkdir -p "$W/ro-home"; chmod 500 "$W/ro-home"
env HOME="$W/ro-home" timeout 10 "$BIN/clusterscope-agent" > "$W/ro.log" 2>&1
RO_EXIT=$?
chmod 700 "$W/ro-home"
{
    echo "exit=$RO_EXIT   (1 = hard failure at startup, 124 = still running)"
    cat "$W/ro.log"
} | tee "$EV/no-root-verify-06-nr19-readonly-home.txt"

# ===========================================================================
say "V7 NR15: system-level units cannot be installed as uid $(id -u) -- exact failure"
{
    echo "== unit files"
    for f in "$R"/deploy/*.service; do
        echo "--- $f"
        cat "$f"
    done
    echo "== prerequisites the shipped units require"
    id clusterscope 2>&1
    echo "usr_local_bin=$(stat -c '%U:%G %a' /usr/local/bin)"
    for p in /etc/clusterscope /var/lib/clusterscope /var/log/clusterscope-server /var/log/clusterscope-agent; do
        if [ -e "$p" ]; then echo "$p exists $(stat -c '%U:%G %a' "$p")"; else echo "$p absent"; fi
    done
} > "$EV/no-root-verify-07-nr15-units-facts.txt"
# The real proof: try to install it the documented way and capture what systemd says.
{
    echo "== attempt 1: systemctl link (the documented 'just install the unit' step) as uid $(id -u)"
    systemctl link "$R/deploy/server.service" 2>&1 | sed 's/^/  /'
    echo "  exit=$?"
    echo "== attempt 2: copy the unit to /etc/systemd/system (what the README implies)"
    mkdir -p /etc/systemd/system 2>&1 | sed 's/^/  /'
    cp "$R/deploy/server.service" /etc/systemd/system/clusterscope-server.service 2>&1 | sed 's/^/  /'
    echo "  exit=$?"
    echo "== attempt 3: systemctl --user start the shipped unit file (it is a system unit)"
    systemctl --user start "$R/deploy/server.service" 2>&1 | sed 's/^/  /'
    echo "  exit=$?"
    echo "== attempt 4: systemd-analyze verify (parses fine, still needs root to install)"
    systemd-analyze verify "$R/deploy/server.service" 2>&1 | sed 's/^/  /'
    echo "  exit=$?"
} | tee "$EV/no-root-verify-08-nr15-install-attempts.txt"

# ===========================================================================
say "V8 MRG-02 fixtures + the redis/postgresql dependency claim"
{
    echo "== M10 section present"
    grep -n '^| M10 ' "$R/qa/merge-plan-requirements.md"
    echo "== NRM rows: $(grep -c '^| NRM' "$R/qa/merge-plan-requirements.md")"
    grep -n '^| NRM' "$R/qa/merge-plan-requirements.md" | cut -c1-90
    echo "== NR constraints in qa/constraints.json: $(python3 -c "import json;print(sum(1 for c in json.load(open('$R/qa/constraints.json')) if c['id'].startswith('NR-') or c['id']=='MRG-02'))")"
    echo "== redis: declared in units but used by the code?"
    grep -n 'redis' "$R"/deploy/*.service
    echo "redis refs in crates/**: $(grep -ric 'redis' "$R/crates" | grep -v ':0$' | wc -l) files"
    grep -rn 'redis' "$R/crates" | head -10
    echo "== postgresql.service is a distro unit name (Debian/Ubuntu); this host runs PG built into HOME"
    ls /lib/systemd/system/postgresql.service 2>&1
} | tee "$EV/no-root-verify-09-mrg02-and-redis.txt"

# ===========================================================================
say "V9 NR13: repo ships 0 user-level server units; the machine-local one is hand-written"
{
    echo "== README 'systemctl --user' lines"
    grep -n 'systemctl --user' "$R/README.md"
    echo "== units shipped in the repo"
    echo "clusterscope-server.service (excluding target/): $(find "$R" -name 'clusterscope-server.service' -not -path '*/target/*' | wc -l)"
    find "$R" -name '*.service' -not -path '*/target/*' -not -path '*/.git/*'
    echo "== is any repo file able to generate the user-level server unit?"
    grep -rln 'clusterscope-server.service' "$R" --exclude-dir=target --exclude-dir=.git || echo "(no file mentions it)"
    echo "== docs/: systemctl --user or ~/.config/systemd mentions: $(grep -rl 'systemctl --user\|\.config/systemd' "$R/docs" | wc -l)"
    echo "== machine-local unit (NOT from the repo)"
    cat "$HOME/.config/systemd/user/clusterscope-server.service"
    stat -c '%n mtime=%y' "$HOME/.config/systemd/user/clusterscope-server.service"
    echo "state=$(systemctl --user is-enabled clusterscope-server.service 2>&1)/$(systemctl --user is-active clusterscope-server.service 2>&1)"
    echo "ExecStart points into HOME (hand-written marker): $(grep -c 'ExecStart=.*/\.local/bin/\|ExecStart=.*/\.config/' "$HOME/.config/systemd/user/clusterscope-server.service")"
    echo "== clean-machine consequence: 'systemctl --user restart clusterscope-server' on a host without that file"
    echo "  (proof by construction: the command resolves a unit name that no repo artefact installs)"
} | tee "$EV/no-root-verify-10-nr13-no-user-server-unit.txt"

# ===========================================================================
say "V10 NR9: docker/compose promise vs what this environment can actually do"
{
    echo "== README promise"
    grep -n 'docker compose up\|docker-compose\|docker' "$R/README.md" | head -20
    echo "== compose file shipped"
    ls -l "$R/deploy/docker-compose.yml" 2>&1
    echo "== runtime availability"
    command -v docker || echo "docker: absent"
    command -v docker-compose || echo "docker-compose: absent"
    command -v podman || echo "podman: absent"
    echo "== podman images (must be empty for compose to work offline)"
    podman images 2>&1 | head -10
    echo "== the documented command, actually attempted (bounded by timeout)"
    cd "$R/deploy" && timeout 25 podman-compose up -d 2>&1 | head -20
    echo "  podman-compose exit=$?"
    timeout 25 docker compose up -d 2>&1 | head -5
    echo "  docker-compose exit=$?"
    echo "== outbound network (README assumes an image can be pulled)"
    timeout 8 curl -s -o /dev/null -w '%{http_code}\n' https://registry-1.docker.io/v2/ 2>&1 || echo "curl failed (no egress)"
    echo "== the route that DOES work here, undocumented in README"
    "$PG/pg_ctl" -D "$PGDATA" status
    echo "  pg_ctl exit=$?"
    "$PG/psql" "$CONN" -tAc 'select version();'
    "$PG/psql" "$CONN" -tAc 'select current_user, session_user;'
    echo "  psql on PATH? $(command -v psql || echo no)"
    echo "  README mentions building PostgreSQL from source? $(grep -ci '从源码\|from source\|configure --prefix' "$R/README.md") hits"
} | tee "$EV/no-root-verify-11-nr09-docker-vs-source-build.txt"

# ===========================================================================
say "V11 NR1: bind both documented ports as uid $(id -u) (server stopped first)"
stop_server
sleep 3
{
    echo "uid=$(id -u)"
    echo "== port occupancy check before probing"
    ss -ltnH | grep -E ':(8080|50051)[[:space:]]' || echo "(both free)"
    for p in 8080 50051; do
        python3 -c "
import socket,os
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
try:
    s.bind(('0.0.0.0', $p))
    print('bind $p ok as uid', os.getuid(), '(port >1024, no CAP_NET_BIND_SERVICE)')
except OSError as e:
    print('bind $p FAILED:', e)
finally:
    s.close()
"
    done
} | tee "$EV/no-root-verify-12-nr01-unprivileged-bind.txt"

# ===========================================================================
say "V12 NR18: TUI under a pty (server required)"
start_env_only_server
TERM=xterm script -q -c "timeout 8 $BIN/clusterscope-tui -s http://127.0.0.1:8080" "$W/tui.pty" >/dev/null 2>&1
{
    echo "pty_bytes=$(wc -c < "$W/tui.pty")"
    echo "panic_hits=$(grep -ci panic "$W/tui.pty")"
    echo "== first 20 printable lines of the rendering"
    sed 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$W/tui.pty" | tr -s ' \n' ' \n' | head -20 | sed 's/^/  /'
} | tee "$EV/no-root-verify-13-nr18-tui-pty.txt"
stop_server

# ===========================================================================
say "V13 NR20: unprivileged GPU/sysfs reads"
{
    echo "== nvidia-smi -L"
    nvidia-smi -L
    echo "== /sys/class/drm/card0/device/power/runtime_status"
    cat /sys/class/drm/card0/device/power/runtime_status
    echo "== /sys/block/nvme0n1/device/model"
    cat /sys/block/nvme0n1/device/model
    echo "== samples the agent collects itself (proc/dev as a plain user)"
    ls /proc/self/io /proc/self/statm >/dev/null 2>&1 && echo "proc self io/statm readable" || echo "proc self unreadable"
} | tee "$EV/no-root-verify-14-nr20-nvml-sysfs.txt"

# ===========================================================================
say "V14 NR11: user service surviving an SSH teardown (linger), and the other direction"
{
    echo "== Linger value THIS RUN depends on (see qa/no-root.qa.md NR11 / qa-report env note)"
    loginctl show-user "$(id -un)" | grep '^Linger='
    echo "== probe unit"
    mkdir -p "$HOME/.config/systemd/user"
    cat > "$HOME/.config/systemd/user/nr-persist-unit.service" <<'PERSIST_EOF'
[Unit]
Description=ClusterScope no-root persistence probe (removed by the check)
[Service]
Type=simple
ExecStart=/bin/sleep 180
[Install]
WantedBy=default.target
PERSIST_EOF
    systemctl --user daemon-reload >/dev/null 2>&1
    systemctl --user enable --now nr-persist-unit.service >/dev/null 2>&1
    P=$(systemctl --user show nr-persist-unit.service -p MainPID --value)
    echo "  MainPID=$P  is-active=$(systemctl --user is-active nr-persist-unit.service)"
    echo "  fragment=$(systemctl --user show nr-persist-unit.service -p FragmentPath --value)"
    echo "== tear a separate ssh session down (does the user manager kill it?)"
    ssh -o BatchMode=yes localhost 'exit 0' >/dev/null 2>&1
    sleep 5
    kill -0 "$P" 2>/dev/null && echo "  VERDICT: MainPID $P still alive after session teardown (linger keeps the user manager)" || echo "  VERDICT: MainPID $P gone"
    systemctl --user disable --now nr-persist-unit.service >/dev/null 2>&1
    rm -f "$HOME/.config/systemd/user/nr-persist-unit.service"
    systemctl --user daemon-reload >/dev/null 2>&1
    echo "== the other direction: what a node with Linger=no does, and whether we can test it here"
    echo "  loginctl enable-linger/disable-linger requires root; we have no sudo ->"
    command -v sudo >/dev/null 2>&1 && echo "  sudo exists (not used: cluster-level change, forbidden this round)" || echo "  no sudo binary at all"
    echo "  SEMANTICS (systemd-logind): without linger, the per-user systemd instance is"
    echo "  stopped when the last session of that user ends -> all of its units, including"
    echo "  Restart=always ones, are terminated with it. Linger is per-machine state."
    echo "  EVIDENCE that a 'no-linger' node is a real scenario: install-agent.sh:60 only"
    echo "  checks \`systemctl --user show-environment\`, never the Linger property:"
    grep -n 'systemctl --user show-environment\|Linger\|enable-linger' "$R/deploy/install-agent.sh"
} | tee "$EV/no-root-verify-15-nr11-linger-semantics.txt"

# ===========================================================================
say "V15 NR12: nohup fallback -- starts, but no supervision, and pkill footgun"
rm -rf "$W/nohup"; mkdir -p "$W/nohup/logs"
cat > "$W/nohup/agent.yaml" <<NOHUP_EOF
server_addr: "http://127.0.0.1:59998"
node_id: "nr-verify-nohup"
log_dir: $W/nohup/logs
NOHUP_EOF
nohup "$BIN/clusterscope-agent" -c "$W/nohup/agent.yaml" >> "$W/nohup/agent.log" 2>&1 &
NPID=$!
sleep 5
{
    echo "nohup_pid=$NPID alive=$(kill -0 "$NPID" 2>/dev/null && echo yes || echo no)"
    echo "greeting_lines=$(grep -c 'ClusterScope Agent starting' "$W/nohup/agent.log")"
    kill "$NPID" 2>/dev/null
    sleep 3
    echo "after manual kill: alive=$(kill -0 "$NPID" 2>/dev/null && echo yes || echo no)  (nothing restarts it)"
    echo "== supervision exists only in the systemd --user branch of install-agent.sh"
    grep -n 'Restart=always\|nohup\|systemctl --user\|pkill' "$R/deploy/install-agent.sh"
    echo "== FOOTGUN: the nohup branch runs 'pkill -f clusterscope-agent' first (install-agent.sh:80)."
    echo "   On a shared host that kills *every* agent of this user, including ones this"
    echo "   installer did not start. Live proof that such a process exists right now:"
    ps -u "$(id -un)" -o pid,lstart,cmd | grep 'clusterscope-agent' | grep -v grep
} | tee "$EV/no-root-verify-16-nr12-nohup-fallback.txt"

echo "== done; evidence in $EV"
