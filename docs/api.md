# ClusterScope API Documentation

## Authentication

All API endpoints (except `/api/health`, `/api/login`, `/api/refresh-token`) require a Bearer JWT token.

```
Authorization: Bearer <token>
```

## Endpoints

### Auth

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/login` | Login with username/password |
| POST | `/api/refresh-token` | Refresh JWT tokens |

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
