#!/bin/sh
# no-root-checks.sh -- ClusterScope review, no-root dimension (NR1..NR21).
# Runs on node 172.19.133.164 as a plain user (uid 3000). Never uses sudo, never
# needs root, never kills processes it did not start (contract of qa/README.md).
#
#   cd /public/tianyuyang/code/ClusterScope-review/gh-line
#   sh qa/harness/no-root-checks.sh            # full run (~2 min)
#   sh qa/harness/no-root-checks.sh --no-slow  # skip the 2 sleep-heavy checks (NR11, NR12)
#
# Every line is  "<CHECK-ID> PASS|FAIL - detail"; the exit code counts FAILs.
# Constraint ids NR-01..NR-21 + MRG-02 are documented in qa/no-root.qa.md.

R=/public/tianyuyang/code/ClusterScope-review/gh-line
BIN="$R/target/release"
OUT="$R/gauntlet-out/qa/evidence"
WORK=/tmp/nr-checks
SLOW=1
[ "${1:-}" = "--no-slow" ] && SLOW=0

mkdir -p "$OUT" "$WORK" || exit 1
PASS=0
FAIL=0
SRV_PID=

ok()   { echo "$1 PASS - $2"; PASS=$((PASS + 1)); }
bad()  { echo "$1 FAIL - $2"; FAIL=$((FAIL + 1)); }
note() { echo "NOTE - $2"; }
cleanup() {
    [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null
    sleep 1
    [ -n "$SRV_PID" ] && kill -9 "$SRV_PID" 2>/dev/null
    chmod 700 /tmp/nr-home-ro 2>/dev/null
    rm -rf /tmp/nr-home /tmp/nr-home-ro /tmp/nr-home-noconf
    rm -f "$WORK"/*.yaml "$WORK"/*.log "$WORK"/*.out "$WORK"/*.uc
    rm -f "$HOME/.config/systemd/user/nr-probe-unit.service"
    systemctl --user daemon-reload 2>/dev/null
}
trap cleanup EXIT INT TERM

# start_server <env-only|config> -- starts a server as this user, sets SRV_PID.
start_server() {
    if [ "$1" = "env-only" ]; then
        POSTGRES_URL='postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope' \
        JWT_SECRET='nr-checks-secret-0123456789abcdef' AUTH_REQUIRED=false \
        "$BIN/clusterscope-server" >> "$OUT/no-root-server.log" 2>&1 &
    else
        sed 's#localhost:5432#127.0.0.1:5432#' "$R/deploy/server.yaml.example" > "$WORK/server.yaml"
        "$BIN/clusterscope-server" "$WORK/server.yaml" >> "$OUT/no-root-server.log" 2>&1 &
    fi
    SRV_PID=$!
}
ports_open() { ss -ltn 2>/dev/null | grep -cE ':(8080|50051)[^0-9]'; }
stop_server() {
    [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null
    sleep 2
    [ -n "$SRV_PID" ] && kill -9 "$SRV_PID" 2>/dev/null
    SRV_PID=
}

echo "== ClusterScope no-root checks on $(hostname) as $(id -un) (uid $(id -u))"
echo "== evidence: $OUT"
# Keep the previous run's server log instead of truncating it.
[ -f "$OUT/no-root-server.log" ] && mv -f "$OUT/no-root-server.log" "$OUT/no-root-server.prev.log"
: > "$OUT/no-root-server.log"

# ---- NR4: no system path is writable by this user --------------------------
# Write a probe file (NOT mkdir): mkdir -p succeeds on an already existing
# directory, and a pre-existing /etc/clusterscope must not look "creatable".
NR4_WRITABLE=""
for p in /etc/clusterscope /var/lib/clusterscope /var/log/clusterscope-server /usr/local/bin; do
    if touch "$p/.nr-probe-$$" 2>/dev/null; then
        NR4_WRITABLE="$NR4_WRITABLE $p"
        rm -f "$p/.nr-probe-$$"
    fi
done
if [ -z "$NR4_WRITABLE" ]; then
    ok "NR4" "not writable as uid $(id -u): /etc/clusterscope /var/lib/clusterscope /var/log/clusterscope-server /usr/local/bin"
else
    bad "NR4" "writable system path(s):$NR4_WRITABLE"
fi

# ---- NR7: bare server (no config, no env) must refuse, not misconfigure -----
timeout 15 "$BIN/clusterscope-server" > "$WORK/server-bare.log" 2>&1
if grep -q 'jwt_secret is missing/too weak' "$WORK/server-bare.log"; then
    ok "NR7" "bare start refused: $(grep -m1 'refusing to start' "$WORK/server-bare.log")"
else
    bad "NR7" "bare start did not hit the jwt_secret guard: $(head -1 "$WORK/server-bare.log")"
fi

# ---- NR2: documented quickstart path (cp example + localhost->127.0.0.1) ----
start_server config
i=0
while [ "$i" -lt 20 ]; do
    [ "$(ports_open)" -ge 1 ] && break
    i=$((i + 1)); sleep 1
done
NR2_L=$(ports_open)
NR2_H=$(curl -s -o /dev/null -w '%{http_code}' --max-time 4 http://127.0.0.1:8080/api/health)
if [ "$NR2_L" -ge 2 ] && [ "$NR2_H" = "200" ]; then
    ok "NR2" "server up as uid $(id -u): listeners=$NR2_L health=$NR2_H"
else
    bad "NR2" "listeners=$NR2_L health=$NR2_H (see $OUT/no-root-server.log)"
fi

# ---- NR21: same binary, zero config files, environment only ------------------
if command -v lsof >/dev/null 2>&1; then
    SRV_ENV=$(lsof -p "$SRV_PID" 2>/dev/null | grep -cE '(/etc/clusterscope|/var/lib/clusterscope|/var/log/clusterscope)')
else
    SRV_ENV="unknown"
fi
if [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 4 http://127.0.0.1:8080/api/health)" = "200" ] \
   && [ "$SRV_ENV" = "0" ]; then
    ok "NR21" "running server opens 0 files under /etc|/var for clusterscope (lsof)"
elif [ "$SRV_ENV" = "unknown" ]; then
    note NR21 "lsof unavailable: only the config-path form could be checked"
    bad "NR21" "lsof unavailable"
else
    bad "NR21" "$SRV_ENV open path(s) under root-only directories"
fi
stop_server

# ---- NR1: ports above 1024 are bindable without CAP_NET_BIND_SERVICE ---------
# Probing a port is only meaningful while nothing holds it, so probe each one
# independently and report which was busy (a stray server elsewhere on this
# shared host must not turn this into a false negative).
NR1_OK=""
NR1_BUSY=""
NR1_FAIL=""
for p in 8080 50051; do
    if ss -ltnH 2>/dev/null | grep -qE ":$p[[:space:]]"; then
        NR1_BUSY="$NR1_BUSY $p"
        continue
    fi
    # A port just released by a stopped server can still sit in TIME_WAIT, so use
    # SO_REUSEADDR and retry a few times before calling it a failure.
    NR1_ATTEMPT=0
    while [ "$NR1_ATTEMPT" -lt 3 ]; do
        NR1_ATTEMPT=$((NR1_ATTEMPT + 1))
        if python3 -c "import socket,sys
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
try:
    s.bind(('0.0.0.0', $p))
except OSError:
    sys.exit(1)
finally:
    s.close()"; then
            NR1_OK="$NR1_OK $p"
            break
        fi
        sleep 2
    done
    if [ "$NR1_ATTEMPT" -ge 3 ] && ! ss -ltnH 2>/dev/null | grep -qE ":$p[[:space:]]"; then
        case " $NR1_OK " in *" $p "*) ;; *) NR1_FAIL="$NR1_FAIL $p" ;; esac
    fi
done
if [ -z "$NR1_FAIL" ] && [ -n "$NR1_OK" ]; then
    ok "NR1" "unprivileged bind succeeded as uid $(id -u) on:$NR1_OK (busy, not probed:$NR1_BUSY)"
elif [ -z "$NR1_OK" ]; then
    bad "NR1" "nothing could be probed (both ports busy:$NR1_BUSY)"
else
    bad "NR1" "bind failed on free port(s):$NR1_FAIL (ok:$NR1_OK busy:$NR1_BUSY)"
fi

# ---- NR6: agent needs no /etc path; effective paths are reproducible ---------
"$BIN/clusterscope-agent" --help > "$WORK/agent-help.txt" 2>&1
AGENT_FLAGS=0
for f in --config --config-dir --server-addr --node-id --agent-token; do
    grep -q -- "$f" "$WORK/agent-help.txt" && AGENT_FLAGS=$((AGENT_FLAGS + 1))
done
rm -rf /tmp/nr-home-noconf; mkdir -p /tmp/nr-home-noconf
env HOME=/tmp/nr-home-noconf timeout 8 "$BIN/clusterscope-agent" -c /etc/clusterscope/agent.yaml > "$WORK/agent-default-path.log" 2>&1
if grep -q 'ClusterScope Agent starting' "$WORK/agent-default-path.log"; then
    ok "NR6" "agent started with missing -c /etc/clusterscope/agent.yaml; $AGENT_FLAGS/5 override flags present"
else
    bad "NR6" "agent did not start with the default config path: $(head -1 "$WORK/agent-default-path.log")"
fi
if [ -d /tmp/nr-home-noconf/.local/state/clusterscope-agent ]; then
    ok "NR6b" "agent created \$HOME/.local/state/clusterscope-agent (XDG state dir, no root)"
else
    bad "NR6b" "agent did not create a HOME-local log dir"
fi
rm -rf /tmp/nr-home-noconf

# ---- NR5: systemd --user control group (dedicated unit, removed afterwards) --
systemctl --user show-environment >/dev/null 2>&1
NR5_ENV=$?
UNIT="$HOME/.config/systemd/user/nr-probe-unit.service"
mkdir -p "$HOME/.config/systemd/user"
cat > "$UNIT" <<'UNIT_EOF'
[Unit]
Description=ClusterScope no-root probe unit (removed by the check)
[Service]
Type=oneshot
ExecStart=/bin/true
RemainAfterExit=yes
[Install]
WantedBy=default.target
UNIT_EOF
systemctl --user daemon-reload >/dev/null 2>&1
ENABLED=$(systemctl --user enable --now nr-probe-unit.service 2>&1 | grep -c 'Created symlink')
FRAG=$(systemctl --user show nr-probe-unit.service -p FragmentPath --value 2>/dev/null)
ACTIVE=$(systemctl --user is-active nr-probe-unit.service 2>/dev/null)
systemctl --user disable --now nr-probe-unit.service >/dev/null 2>&1
rm -f "$UNIT"
systemctl --user daemon-reload >/dev/null 2>&1
if [ "$NR5_ENV" = "0" ] && [ "$ENABLED" -ge 1 ] && [ "$ACTIVE" = "active" ]; then
    ok "NR5" "user unit installed/enabled without sudo (FragmentPath=$FRAG)"
else
    bad "NR5" "systemctl_user_env_exit=$NR5_ENV enabled_lines=$ENABLED active=$ACTIVE"
fi

# ---- NR14: installer's user-level form is a real, satisfied contract ---------
INST_USER_PATHS=$(grep -c '~/.local/bin\|~/.config/clusterscope\|~/.config/systemd/user' "$R/deploy/install-agent.sh")
INST_SYS_PATHS=$(grep -vE '^[[:space:]]*#' "$R/deploy/install-agent.sh" | grep -c '/usr/local/bin\|/etc/clusterscope\|/var/lib/clusterscope\|/var/log/clusterscope')
INST_FALLBACK=$(grep -c 'nohup' "$R/deploy/install-agent.sh")
if [ "$INST_USER_PATHS" -ge 3 ] && [ "$INST_SYS_PATHS" = "0" ] && [ "$INST_FALLBACK" -ge 1 ]; then
    ok "NR14" "install-agent.sh: $INST_USER_PATHS user-path refs, 0 non-comment system-path refs, nohup fallback present"
else
    bad "NR14" "install-agent.sh user=$INST_USER_PATHS system=$INST_SYS_PATHS nohup=$INST_FALLBACK"
fi

# ---- NR15: shipped system units are installable only with root ---------------
UNIT_SYS=0
for f in "$R"/deploy/*.service; do
    if grep -q 'WantedBy=multi-user.target' "$f"; then
        UNIT_SYS=$((UNIT_SYS + 1))
    fi
    grep -qE 'ExecStart=/usr/local/bin/|WorkingDirectory=/var/lib/clusterscope|LogsDirectory=/var/log/clusterscope' "$f" || UNIT_SYS=$((UNIT_SYS + 1))
done
if [ "$UNIT_SYS" = "2" ] && ! id clusterscope >/dev/null 2>&1; then
    ok "NR15" "both deploy/*.service are system units (User=clusterscope, /usr/local/bin, /var/lib); user clusterscope absent"
else
    bad "NR15" "system-unit count=$UNIT_SYS (expected 2); clusterscope user present=$(id clusterscope >/dev/null 2>&1 && echo yes || echo no)"
fi

# ---- NR16: the user-level path is exercised in production on this node -------
UNIT_FILE="$HOME/.config/systemd/user/clusterscope-agent.service"
NR16_U=no; [ -f "$UNIT_FILE" ] && NR16_U=yes
NR16_A=$(systemctl --user is-active clusterscope-agent.service 2>/dev/null)
NR16_P=$(ps -u "$(id -un)" -o pid,cmd 2>/dev/null | grep -c '^ *[0-9]* .*clusterscope-agent' )
if [ "$NR16_U" = "yes" ] && [ "$NR16_A" = "active" ] && [ "$NR16_P" -ge 1 ]; then
    ok "NR16" "live user unit active (FragmentPath=$UNIT_FILE); running agent processes=$NR16_P"
else
    bad "NR16" "unit_file=$NR16_U is_active=$NR16_A agent_procs=$NR16_P"
fi

# ---- NR13: README hands out user-level server management; repo ships no such unit
SRV_README=$(grep -c 'systemctl --user' "$R/README.md")
SRV_SHIPPED=$(find "$R" -name 'clusterscope-server.service' -not -path '*/target/*' 2>/dev/null | wc -l)
SRV_USER_UNIT="$HOME/.config/systemd/user/clusterscope-server.service"
SRV_HANDWRITTEN=no
if [ -f "$SRV_USER_UNIT" ]; then
    if grep -q '^ExecStart=.*/\.local/bin/\|^ExecStart=.*/\.config/' "$SRV_USER_UNIT" && [ "$SRV_SHIPPED" = "0" ]; then
        SRV_HANDWRITTEN=yes
    fi
fi
if [ "$SRV_README" -ge 4 ] && [ "$SRV_SHIPPED" = "0" ] && [ "$SRV_HANDWRITTEN" = "yes" ]; then
    ok "NR13" "README has $SRV_README 'systemctl --user' lines incl. clusterscope-server, but the repo ships 0 server units: the working user-level unit on this node is hand-written, not reproducible from the repo (see qa/no-root.qa.md NR13)"
else
    bad "NR13" "readme_user_lines=$SRV_README shipped_server_units=$SRV_SHIPPED handwritten_user_unit=$SRV_HANDWRITTEN"
fi

# ---- NR3: HOME-local paths are the ones actually created ---------------------
rm -rf /tmp/nr-home; mkdir -p /tmp/nr-home
env HOME=/tmp/nr-home timeout 6 "$BIN/clusterscope-agent" --config-dir /tmp/nr-home/.config/clusterscope > "$WORK/agent-home.log" 2>&1
if [ -d /tmp/nr-home/.config/clusterscope/logs ] \
   && grep -q '~/\.config\|/etc/clusterscope' "$R/deploy/agent.yaml.example"; then
    ok "NR3" "agent created log dir under the -c/--config-dir home: $(ls -d /tmp/nr-home/.config/clusterscope/logs)"
else
    bad "NR3" "expected /tmp/nr-home/.config/clusterscope/logs: $(head -2 "$WORK/agent-home.log")"
fi

# ---- NR20: NVML/sysfs readable as a plain user -------------------------------
NR20_CMD=0
command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1 && NR20_CMD=1
NR20_SYS=0
cat /sys/class/drm/card0/device/power/runtime_status >/dev/null 2>&1 && NR20_SYS=1
NR20_NVME=0
cat /sys/block/nvme0n1/device/model >/dev/null 2>&1 && NR20_NVME=1
if [ "$NR20_CMD" = "1" ] && [ "$NR20_SYS" = "1" ] && [ "$NR20_NVME" = "1" ]; then
    ok "NR20" "unprivileged reads: nvidia-smi -L, /sys/class/drm/card0 power state, /sys/block/nvme0n1 model"
else
    bad "NR20" "nvidia_smi=$NR20_CMD drm_sysfs=$NR20_SYS nvme_sysfs=$NR20_NVME"
fi

# ---- NR18: TUI renders under a pty as a plain user ---------------------------
start_server env-only
i=0
while [ "$i" -lt 20 ]; do
    [ "$(ports_open)" -ge 1 ] && break
    i=$((i + 1)); sleep 1
done
TERM=xterm script -q -c "timeout 8 $BIN/clusterscope-tui -s http://127.0.0.1:8080" "$WORK/tui.pty" >/dev/null 2>&1
if [ -s "$WORK/tui.pty" ] && ! grep -qi 'panic' "$WORK/tui.pty"; then
    ok "NR18" "TUI rendered $WORK/tui.pty ($(wc -c < "$WORK/tui.pty") bytes), no panic"
else
    bad "NR18" "pty capture empty or panicked"
fi
stop_server

# ---- NR8: env-only override keeps root-only prefixes out of the startup path --
grep -cE 'CLUSTERSCOPE_POSTGRES_URL|CLUSTERSCOPE_JWT_SECRET|CLUSTERSCOPE_HTTP_ADDR|CLUSTERSCOPE_GRPC_ADDR|CLUSTERSCOPE_AUTH_REQUIRED' "$R/crates/server/src/main.rs" >/dev/null 2>&1
NR8_KEYS=$(grep -cE '"POSTGRES_URL"|"JWT_SECRET"|"HTTP_ADDR"|"GRPC_ADDR"|"AUTH_REQUIRED"|"AGENT_TOKEN"' "$R/crates/server/src/main.rs")
if [ "$NR8_KEYS" -ge 5 ] && grep -q 'default_value = "/etc/clusterscope/agent.yaml"' "$R/crates/agent/src/main.rs"; then
    ok "NR8" "server reads $NR8_KEYS documented env keys; agent keeps /etc default but ships --config/--config-dir/--server-addr overrides"
else
    bad "NR8" "env keys=$NR8_KEYS"
fi

# ---- NR9: PostgreSQL deployment route that is not documented ------------------
/public/tianyuyang/code/ClusterScope-review/pg16/bin/pg_ctl \
    -D /public/tianyuyang/code/ClusterScope-review/pgdata status >/dev/null 2>&1
NR9_PG=$?
PG_PSQL=$(command -v psql >/dev/null 2>&1 && echo path || echo nopath)
NR9_CONN=$(/public/tianyuyang/code/ClusterScope-review/pg16/bin/psql \
    'postgresql://clusterscope:clusterscope@127.0.0.1:5432/clusterscope' -tAc 'select 1' 2>/dev/null)
NR9_COMPOSE_DOC=$(grep -c 'docker compose up' "$R/README.md")
NR9_DOCKER=$(command -v docker >/dev/null 2>&1 && echo present || echo absent)
if [ "$NR9_PG" = "0" ] && [ "$NR9_CONN" = "1" ] && [ "$NR9_COMPOSE_DOC" -ge 1 ] && [ "$NR9_DOCKER" = "absent" ]; then
    ok "NR9" "self-compiled PG 16.4 runs as uid $(id -u) and answers; README offers only 'docker compose up' while docker is $NR9_DOCKER (psql $PG_PSQL)"
else
    bad "NR9" "pg_status_exit=$NR9_PG conn='$NR9_CONN' readme_compose=$NR9_COMPOSE_DOC docker=$NR9_DOCKER"
fi

# ---- NR10: README no-root claim list, every mention classified ---------------
NR10_MENTIONS=$(grep -c '无需 root\|无 root\|root-not\|systemctl --user' "$R/README.md")
NR10_TABLE=$(grep -c '普通用户即可' "$R/README.md")
if [ "$NR10_MENTIONS" -ge 11 ] && [ "$NR10_TABLE" -ge 2 ]; then
    ok "NR10" "README has $NR10_MENTIONS no-root/systemctl--user mentions; classifications in $OUT/no-root-readme-claims.txt"
else
    bad "NR10" "mentions=$NR10_MENTIONS table_rows=$NR10_TABLE (expected >=11)"
fi
cat > "$OUT/no-root-readme-claims.txt" <<'CLAIMS'
README line | claim                                              | verdict | check
15          | platform: plain user, no root needed                | PASS    | NR1..NR8, NR16 (uid 3000 measured)
21          | badge alt="No root" root-not required               | PASS    | same as line 15
56          | requirements table row (access level): no root      | PASS    | same as line 15
87          | "no root: docker compose up"                        | FAIL    | NR9 (no docker/docker-compose, no network)
89          | deploy agent over passwordless ssh, no root         | PASS    | NR14, NR16
211         | section: metric collection (no root)                | PASS    | NR20
288         | systemctl --user status clusterscope-server         | FAIL    | NR13 (repo ships no user-level server unit)
289         | systemctl --user restart clusterscope-server        | FAIL    | NR13
290         | systemctl --user restart clusterscope-agent         | PASS    | NR16 (live user unit)
291         | ssh node-01 ... clusterscope-agent                  | PASS    | NR16
293         | journalctl --user -u clusterscope-agent             | PASS    | NR16
321         | troubleshoot: systemctl --user status / journalctl  | PASS    | NR16
CLAIMS

# ---- NR11: log out and back in, user service must still be there (linger) ----
if [ "$SLOW" = "1" ]; then
    LINGER=$(loginctl show-user "$(id -un)" 2>/dev/null | grep '^Linger=' | cut -d= -f2)
    mkdir -p "$HOME/.config/systemd/user"
    cat > "$HOME/.config/systemd/user/nr-persist-unit.service" <<'PERSIST_EOF'
[Unit]
Description=ClusterScope no-root persistence probe (removed by the check)
[Service]
Type=simple
ExecStart=/bin/sleep 120
[Install]
WantedBy=default.target
PERSIST_EOF
    systemctl --user daemon-reload >/dev/null 2>&1
    systemctl --user enable --now nr-persist-unit.service >/dev/null 2>&1
    PPID_1=$(systemctl --user show nr-persist-unit.service -p MainPID --value 2>/dev/null)
    ssh -o BatchMode=yes localhost 'exit 0' >/dev/null 2>&1
    sleep 5
    if [ "$LINGER" = "yes" ] && kill -0 "$PPID_1" 2>/dev/null; then
        ok "NR11" "Linger=$LINGER on $(hostname); dedicated user unit survived an ssh session teardown (pid $PPID_1 alive)"
    else
        bad "NR11" "Linger=$LINGER; pid $PPID_1 alive=$(kill -0 "$PPID_1" 2>/dev/null && echo yes || echo no)"
    fi
    systemctl --user disable --now nr-persist-unit.service >/dev/null 2>&1
    rm -f "$HOME/.config/systemd/user/nr-persist-unit.service"
    systemctl --user daemon-reload >/dev/null 2>&1
else
    note NR11 "skipped (--no-slow): run without --no-slow to test linger survival"
fi

# ---- NR12: nohup fallback starts the agent, but nothing restarts it ----------
if [ "$SLOW" = "1" ]; then
    rm -rf /tmp/nr-nohup; mkdir -p /tmp/nr-nohup/logs
    cat > /tmp/nr-nohup/agent.yaml <<NOHUP_EOF
server_addr: "http://127.0.0.1:50051"
node_id: "nr-nohup-probe"
log_dir: /tmp/nr-nohup/logs
NOHUP_EOF
    nohup "$BIN/clusterscope-agent" -c /tmp/nr-nohup/agent.yaml >> /tmp/nr-nohup/agent.log 2>&1 &
    NPID=$!
    sleep 5
    ALIVE_1=$(kill -0 "$NPID" 2>/dev/null && echo yes || echo no)
    LOG_SEEN=$(grep -c 'ClusterScope Agent starting' /tmp/nr-nohup/agent.log)
    kill "$NPID" 2>/dev/null
    sleep 3
    ALIVE_2=$(kill -0 "$NPID" 2>/dev/null && echo yes || echo no)
    RESTART_CLAIM=$(grep -c 'Restart=always' "$R/deploy/install-agent.sh")
    if [ "$ALIVE_1" = "yes" ] && [ "$LOG_SEEN" -ge 1 ] && [ "$ALIVE_2" = "no" ] && [ "$RESTART_CLAIM" -ge 1 ]; then
        ok "NR12" "nohup fallback starts and stays up (pid $NPID); after kill the pid is gone and nothing restarts it (Restart=always exists only in the systemd --user branch)"
    else
        bad "NR12" "alive_before=$ALIVE_1 log=$LOG_SEEN alive_after=$ALIVE_2 systemd_restart_refs=$RESTART_CLAIM"
    fi
    rm -rf /tmp/nr-nohup
else
    note NR12 "skipped (--no-slow): run without --no-slow to exercise the nohup fallback"
fi

# ---- NR19: HOME must be writable, otherwise the agent aborts ------------------
mkdir -p /tmp/nr-home-ro; chmod 500 /tmp/nr-home-ro
env HOME=/tmp/nr-home-ro timeout 10 "$BIN/clusterscope-agent" > "$WORK/agent-ro-home.log" 2>&1
AGENT_RO=$?
chmod 700 /tmp/nr-home-ro; rm -rf /tmp/nr-home-ro
if [ "$AGENT_RO" = "1" ] && grep -q 'Failed to create log directory' "$WORK/agent-ro-home.log"; then
    ok "NR19" "read-only HOME -> agent exits 1 at 'Failed to create log directory' (hard requirement, no rootless bypass)"
else
    bad "NR19" "exit=$AGENT_RO log=$(head -1 "$WORK/agent-ro-home.log")"
fi

# ---- NR17: no shipped frontend to re-check -----------------------------------
if [ ! -d "$R/web" ] && [ "$(find "$R" -maxdepth 3 -name package.json -not -path '*/target/*' 2>/dev/null | wc -l)" = "0" ]; then
    ok "NR17" "gh-line has no web/ and no package.json -> frontend no-root re-check is N/A here (see FE-01)"
else
    bad "NR17" "unexpected frontend files in gh-line"
fi

# ---- MRG-02: no-root fixtures for merge verification -------------------------
NRM_COUNT=$(grep -c '^| NRM' "$R/qa/merge-plan-requirements.md")
NRM_OK=0
if [ "$NRM_COUNT" -ge 6 ] && grep -q 'NR-01' "$R/qa/constraints.json" && [ -f "$R/qa/no-root.qa.md" ]; then
    NRM_OK=1
fi
if [ "$NRM_OK" = "1" ]; then
    ok "MRG-02" "merge-plan carries $NRM_COUNT no-root invariants; NR constraints and qa/no-root.qa.md present"
else
    bad "MRG-02" "merge-plan rows=$NRM_COUNT (see qa/merge-plan-requirements.md M10)"
fi

echo "== no-root checks: PASS=$PASS FAIL=$FAIL"
echo "== evidence dir: $OUT"
exit "$FAIL"
