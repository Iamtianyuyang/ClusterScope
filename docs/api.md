# ClusterScope API Documentation

## Authentication

When `auth_required: true`, API endpoints (except `/api/health`, `/api/login`, and `/api/refresh-token`) require a Bearer JWT token. When `auth_required: false`, read-only GET endpoints are public; write operations still require JWT authentication.

```
Authorization: Bearer <token>
```

## Endpoints

### Auth

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/login` | Login with username/password |
| POST | `/api/refresh-token` | Refresh JWT tokens |

### Server onboarding

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/servers/install` | **Admin only.** SSH into a host, upload this same `clusterscope` binary, install and start its Agent role |

Request body:

```json
{
  "username": "gpu",
  "host": "192.168.0.20",
  "password": "ssh-password",
  "port": 22,
  "node_id": "gpu-09",
  "server_addr": "192.168.10.100:10203"
}
```

- `node_id` is optional; the target hostname is used when it is empty.
- `server_addr` accepts `host:port` or an explicit `http://` / `https://` URL.
- Success returns `{"node_id": "gpu-09", "message": "Agent 已安装并通过 systemd --user 启动。"}`.
- `400` validation error, `401` missing/expired admin token, `502` SSH login or remote command failure, `500` local binary read/generation failure.

The SSH password is used only for this request. It is not written to disk, stored in PostgreSQL, or logged. The endpoint logs in as the supplied **ordinary user** (never root), uploads the currently running `clusterscope` executable to `~/.local/bin/clusterscope`, writes `~/.config/clusterscope/agent.yaml`, and starts `clusterscope agent -c …` through `systemd --user` (falling back to `nohup`). It never calls `sudo` and never writes `/etc` or `/usr/local`.

The admin check is an application role, not OS root. Use the endpoint only on a trusted network; the Web UI warns when the page is served over plain HTTP.

### Nodes

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/nodes` | List all nodes |
| GET | `/api/nodes/{node_id}` | Get node status |
| GET | `/api/nodes/{node_id}/metrics` | Get latest metrics for a node |

### Metrics

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/metrics/history` | Get metrics history |

Query params: `node_id`, `start_time_ms`, `end_time_ms`

Raw rows cover the last 24 hours. Older ranges are served from the hourly
(7 days) and daily (90 days) aggregate tables; aggregated rows carry
`cpu_usage_percent` / `memory_usage_percent` / `gpu_utilization_percent`
plus a `source` tag (`hourly` / `daily`) and `timestamp_ms`, merged with
raw rows and sorted by timestamp.

### Jobs

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/jobs` | List jobs |
| POST | `/api/jobs` | Create a new job |
| GET | `/api/jobs/{job_id}` | Get job details |
| DELETE | `/api/jobs/{job_id}` | Stop a job |
| GET | `/api/jobs/{job_id}/logs` | Get job logs |

`POST /api/jobs` body: `node_id`, `name`, `executable`, `arguments`,
`working_directory`, optional `environment`, and optional `resource_quota`
(e.g. `"gpu:2"`, `"gpus:4"`, or a plain `"2"`) — the scheduler uses it to
reserve GPUs per job; empty/omitted means 1 GPU.

### Alerts

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/alerts/rules` | List alert rules |
| POST | `/api/alerts/rules` | Create alert rule |
| DELETE | `/api/alerts/rules/{rule_id}` | Delete alert rule |
| GET | `/api/alerts/events` | Get alert events |
| POST | `/api/alerts/rules/{rule_id}/ack` | Acknowledge alert |

### Cluster

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/cluster/info` | Get cluster overview |

### Users

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/users` | List users |
| POST | `/api/users` | Create user |

### Audit

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/audit-logs` | List audit logs |

## WebSocket

Connect to `/ws` with query parameter `token=<jwt>`.

Send JSON messages:
```json
{"node_id": "gpu-node-01"}
```

Receives real-time metric updates for subscribed nodes.
