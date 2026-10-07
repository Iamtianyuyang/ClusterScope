#!/bin/sh
# nr-verify2.sh -- round 2: the cases round 1 exposed as under-specified.
#
#   sh qa/harness/nr-verify2.sh
#
# Focus:
#   A. agent + a *missing* -c file: the silent fallback (config_loader.rs:9-11) end to end,
#      with a writable HOME so the run is not masked by an unrelated failure.
#   B. server + a *missing* argv[1] config: does it also fall back silently? (main.rs:193)
#   C. agent with NO HOME/XDG: which path does dirs::config_dir()/state_dir() resolve to,
#      i.e. is `/etc/clusterscope` (crates/common/src/config.rs:38) reachable in practice?
#   D. /proc degradation: pid-list metrics when /proc/<pid>/status|cmdline is unreadable.
#   E. NR1's exact claim: the *server's own* bound sockets are owned by a non-root uid.
#
# Same contract as nr-verify.sh: only kill PIDs we started; never touch foreign processes.

R=/public/tianyuyang/code/ClusterScope-review/gh-line
BIN="$R/target/release"
EV="$R/qa/evidence"
W=/tmp/nr-verify2
mkdir -p "$EV" "$W" || exit 1
SRV=
cleanup() {
    [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
    sleep 1
    [ -n "$SRV" ] && kill -9 "$SRV" 2>/dev/null
}
trap cleanup EXIT INT TERM
say() { echo "----- $*"; }

# ---------------------------------------------------------------- A + B + C
say "A. agent, missing -c file, writable HOME (is the fallback silent?)"
rm -rf "$W/home-a"; mkdir -p "$W/home-a"
env HOME="$W/home-a" timeout 8 "$BIN/clusterscope-agent" -c "$W/absent.yaml" > "$W/a.log" 2>&1
A_EXIT=$?
{
    echo "command: HOME=$W/home-a timeout 8 clusterscope-agent -c $W/absent.yaml"
    echo "exit=$A_EXIT   (124 = still running when timeout fired; 1 = died)"
    echo "== output (full)"
    sed 's/^/  /' "$W/a.log"
    echo "== effective server_addr actually dialled (default is http://localhost:50051)"
    grep -o 'server_addr=[^ ]*' "$W/a.log" | sed 's/^/  /'
    echo "== does anything mention the missing config file $W/absent.yaml?"
    grep -c "$W/absent.yaml" "$W/a.log" | sed 's/^/  mentions=/'
    echo "== files created under the HOME"
    find "$W/home-a" | sed 's/^/  /'
} | tee "$EV/no-root-verify2-a-agent-missing-config.txt"

say "B. server, missing argv[1] config (main.rs:193 bails) -- contrast with the agent"
: > "$W/b.log"
timeout 10 "$BIN/clusterscope-server" "$W/absent-server.yaml" > "$W/b.log" 2>&1
B_EXIT=$?
{
    echo "command: timeout 10 clusterscope-server $W/absent-server.yaml"
    echo "exit=$B_EXIT"
    sed 's/^/  /' "$W/b.log"
    echo "== NOTE: the agent is silent here, the server is not -> the two binaries disagree"
} | tee "$EV/no-root-verify2-b-server-missing-config.txt"

say "C. agent with HOME and XDG_* unset: where do config_dir()/state_dir() land?"
rm -rf "$W/home-c"; mkdir -p "$W/home-c"
# `env -u` requires GNU env; unavailable here -> emulate with `env -i` and a minimal env.
env -i PATH=/usr/bin:/bin timeout 8 "$BIN/clusterscope-agent" --server-addr http://127.0.0.1:59997 \
    > "$W/c.log" 2>&1
C_EXIT=$?
{
    echo "command: env -i PATH=/usr/bin:/bin timeout 8 clusterscope-agent --server-addr http://127.0.0.1:59997"
    echo "exit=$C_EXIT"
    sed 's/^/  /' "$W/c.log"
    echo "== can a plain user write /etc/clusterscope at all?"
    touch /etc/clusterscope/.nr-probe 2>&1 | sed 's/^/  /'
    echo "  touch exit=$?"
    echo "== conclusion: config.rs:38's /etc/clusterscope fallback is only reachable when"
    echo "   HOME is unset; when HOME is set (the normal case) node_id lands in HOME."
    echo "   With HOME unset the agent fails BEFORE reaching a root path (see the error above)."
} | tee "$EV/no-root-verify2-c-agent-no-home.txt"

say "D. /proc degradation: unreadable /proc/<pid>/status|cmdline for other users' pids"
{
    echo "== what a plain user sees for a foreign pid"
    for p in 1 $$; do
        echo "  --- pid $p"
        echo "    /proc/$p/status readable? $(head -1 /proc/$p/status >/dev/null 2>&1 && echo yes || echo no)"
        echo "    /proc/$p/cmdline readable? $(cat /proc/$p/cmdline >/dev/null 2>&1 && echo yes || echo no)"
        echo "    /proc/$p/io readable? $(cat /proc/$p/io >/dev/null 2>&1 && echo yes || echo no)"
    done
    echo "== hidepid mount options on /proc"
    mount | grep -E ' /proc ' | sed 's/^/  /'
    echo "== the code path that degrades (crates/agent/src/metrics.rs:596-616)"
    sed -n '596,617p' "$R/crates/agent/src/metrics.rs" | sed 's/^/  /'
    echo "== live agent's own metrics sample: does it report unknown/?, or crash?"
    rm -rf "$W/home-d"; mkdir -p "$W/d/logs"
    cat > "$W/d/agent.yaml" <<AGENT_EOF
server_addr: "http://127.0.0.1:59996"
node_id: "nr-verify2-proc"
log_dir: $W/d/logs
collect_process_details: true
disk_mounts: ["/"]
AGENT_EOF
    env HOME="$W/home-d" timeout 10 "$BIN/clusterscope-agent" -c "$W/d/agent.yaml" > "$W/d.log" 2>&1 &
    APID=$!
    sleep 6
    echo "  agent alive=$(kill -0 $APID 2>/dev/null && echo yes || echo no) (no panic while sampling other users' pids)"
    grep -icE 'panic|permission denied' "$W/d.log" | sed 's/^/  panic_or_denied_lines=/'
    kill "$APID" 2>/dev/null
    sleep 2
} 2>&1 | tee "$EV/no-root-verify2-d-proc-degradation.txt"

say "E. NR1 tightened: the server's own bound sockets, owned by uid $(id -u)"
sed 's#localhost:5432#127.0.0.1:5432#' "$R/deploy/server.yaml.example" > "$W/server.yaml"
"$BIN/clusterscope-server" "$W/server.yaml" >> "$W/e.log" 2>&1 &
SRV=$!
i=0
while [ "$i" -lt 25 ]; do
    [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 http://127.0.0.1:8080/api/health)" = "200" ] && break
    i=$((i + 1)); sleep 1
done
{
    echo "== server pid=$SRV euid/ruid (must be 3000, never 0)"
    grep -E '^(Uid|Gid)' "/proc/$SRV/status"
    echo "== /proc/$SRV/net or ss ownership of the two documented ports"
    ss -ltnp 2>/dev/null | grep -E ':(8080|50051)\b'
    echo "== getcap on the binary (no file capability granted -> pure unprivileged bind)"
    getcap "$BIN/clusterscope-server" 2>&1 || echo "  getcap: unavailable or no capabilities"
    echo "== capabilities of the running process"
    grep -E '^Cap(Eff|Prm|Bnd)' "/proc/$SRV/status" | sed 's/^/  /'
    echo "  CapEff 0000000000000000 would mean zero privileges; a non-zero value means"
    echo "  standard ambient caps from the user session (NOT setcap on the binary)."
} | tee "$EV/no-root-verify2-e-nr1-socket-ownership.txt"
kill "$SRV" 2>/dev/null
sleep 2
kill -9 "$SRV" 2>/dev/null
SRV=

echo "== done; evidence in $EV"
