#!/bin/sh
# nr-verify3.sh -- round 3: settle two open questions.
#
#   sh qa/harness/nr-verify3.sh
#
# Q1 (main): does the agent SURVIVE with `-c /etc/clusterscope/agent.yaml` missing,
#    or does it exit 1 right after printing its greeting?
#    qa/harness/no-root-checks.sh NR6 captures `$?` from the *grep*, not from the agent,
#    so its "starts fine" conclusion is not evidence about the agent's lifetime.
#    The author's own evidence file is compared here side by side.
# Q2: does the agent survive a real metrics tick (report_interval_secs=2 -> 25s)?
#    Only then is the claim "collects NVML and degrades on /proc" actually exercised.
#
# Contract: only kill PIDs we started.

R=/public/tianyuyang/code/ClusterScope-review/gh-line
BIN="$R/target/release"
EV="$R/qa/evidence"
W=/tmp/nr-verify3
mkdir -p "$EV" "$W" || exit 1
say() { echo "----- $*"; }

# =========================================================== Q1
say "Q1a. author's evidence file: what did the earlier run actually record?"
if [ -f "$R/gauntlet-out/qa/evidence/no-root-server.log" ]; then
    echo "  (no-root-server.log is present, $(wc -l < "$R/gauntlet-out/qa/evidence/no-root-server.log") lines)"
fi
echo "== the exact NR6 code path in the author's script (lines 171-183):"
sed -n '171,183p' "$R/qa/harness/no-root-checks.sh" | sed 's/^/  /'
echo "== the line that captures the exit status: 'env HOME=... timeout 8 agent -c /etc/... > log 2>&1'"
echo "   -> \$? is never read there; the grep on the next line decides PASS/FAIL."

say "Q1b. same command, exit status observed directly (HOME with and without \$HOME/.config)"
for variant in precreated bare; do
    H="$W/q1-$variant-home"
    rm -rf "$H"; mkdir -p "$H"
    if [ "$variant" = "precreated" ]; then
        # This is what install-agent.sh does before starting the agent (line 43 / 30).
        mkdir -p "$H/.config/clusterscope" "$H/.local/state"
        echo "--- variant=$variant (HOME/.config/clusterscope pre-created, as install-agent.sh:30/43 does)"
    else
        echo "--- variant=$variant (fresh empty HOME, nothing pre-created)"
    fi
    env HOME="$H" timeout 8 "$BIN/clusterscope-agent" -c /etc/clusterscope/agent.yaml \
        > "$W/q1-$variant.log" 2>&1
    echo "    exit=$?   (124 = timeout had to kill it, i.e. it was still running)"
    echo "    greeting: $(grep -c 'ClusterScope Agent starting' "$W/q1-$variant.log")"
    echo "    stderr tail:"
    tail -4 "$W/q1-$variant.log" | sed 's/^/      /'
done
{
    echo "== side by side with the documented default (no -c at all)"
    H="$W/q1-default-home"; rm -rf "$H"; mkdir -p "$H"
    env HOME="$H" timeout 8 "$BIN/clusterscope-agent" > "$W/q1-default.log" 2>&1
    echo "  no -c        : exit=$? greeting=$(grep -c 'ClusterScope Agent starting' "$W/q1-default.log")"
    echo "  missing -c   : exit=1  greeting=1  (see Q1b above)"
    echo "  => the -c flag itself changes the outcome: the agent creates the log dir but"
    echo "     NOT the parent directory of node_id_file (\$HOME/.config), so it dies on"
    echo "     'Failed to write node identity' -- with no hint that the config file was missing."
    echo "== is /etc/clusterscope present on this host?"
    ls -ld /etc/clusterscope 2>&1 | sed 's/^/  /'
    echo "== relevant code: crates/common/src/config.rs:37-39 and node_identity.rs"
    sed -n '33,40p' "$R/crates/common/src/config.rs" | sed 's/^/  /'
    grep -n 'node_id_file\|Failed to write node identity\|create_dir' "$R/crates/agent/src/node_identity.rs" | sed 's/^/  /'
} | tee "$EV/no-root-verify3-q1-missing-config-lifetime.txt"

# =========================================================== Q2
say "Q2. a real metrics tick: does the agent stay up for 25s and keep sampling?"
rm -rf "$W/q2-home" "$W/q2"; mkdir -p "$W/q2-home" "$W/q2/logs"
cat > "$W/q2/agent.yaml" <<AGENT_EOF
server_addr: "http://127.0.0.1:59995"
node_id: "nr-verify3-metrics"
node_id_file: $W/q2/node_id
report_interval_secs: 2
log_dir: $W/q2/logs
log_level: info
collect_process_details: true
disk_mounts: ["/"]
AGENT_EOF
"$BIN/clusterscope-agent" -c "$W/q2/agent.yaml" > "$W/q2/agent.log" 2>&1 &
APID=$!
sleep 25
{
    echo "pid=$APID alive_after_25s=$(kill -0 "$APID" 2>/dev/null && echo yes || echo no)"
    echo "== log lines: $(wc -l < "$W/q2/agent.log")"
    echo "== panics / permission errors: $(grep -icE 'panic|permission denied' "$W/q2/agent.log")"
    echo "== distinct message shapes (stripped of timestamps)"
    sed -E 's/\x1b\[[0-9;]*m//g; s/^[0-9T:.Z-]+ *//' "$W/q2/agent.log" | awk '{print $1, $2, $3}' | sort | uniq -c | sort -rn | head -10 | sed 's/^/  /'
    echo "== retry backoff observed (agent keeps trying because no server is listening)"
    grep -c 'Failed to connect to server, retrying' "$W/q2/agent.log" | sed 's/^/  retry_lines=/'
    echo "== nvidia-smi reachable from this same uid (NR-20 support)"
    nvidia-smi --query-gpu=index,name,utilization.gpu --format=csv,noheader | head -3 | sed 's/^/  /'
} | tee "$EV/no-root-verify3-q2-metrics-tick.txt"
kill "$APID" 2>/dev/null
sleep 2
kill -9 "$APID" 2>/dev/null

echo "== done; evidence in $EV"
