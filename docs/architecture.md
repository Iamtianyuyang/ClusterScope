# ClusterScope Architecture

## Overview

ClusterScope is a distributed GPU cluster monitoring and job control platform. It consists of:

1. **Agent** - A lightweight Rust service running on each GPU node that collects metrics and executes jobs
2. **Central Server** - A Rust service that aggregates data from all agents, serves REST APIs, and manages jobs/alerts
3. **TUI** - A terminal dashboard (ratatui) for visualizing cluster state, metrics, jobs, and alerts
4. **PostgreSQL** - Persistent storage for metrics history, jobs, alerts, users, and audit logs

## Architecture Diagram

```
┌──────────────────┐
│   TUI (ratatui)  │
└────────┬─────────┘
         │ REST
┌────────▼─────────┐
│  Central Server  │
│                  │
│ Node Registry    │
│ Metrics Service  │
│ Job Service      │
│ Alert Service    │
│ Auth Service     │
└───────┬──────────┘
        │ gRPC
   ┌────┴─────┬──────────┐
   │          │          │
┌──▼───┐  ┌──▼───┐  ┌──▼───┐
│Agent │  │Agent │  │Agent │
│Node A│  │Node B│  │Node C│
└──────┘  └──────┘  └──────┘
         │
    ┌────▼────┐
    │PostgreSQL│
    └─────────┘
```

## Communication Protocols

- **Agent → Server**: gRPC streaming (metrics, heartbeats, job logs, job status)
- **TUI → Server**: REST API (read-only monitoring, login)
- **Server → DB**: PostgreSQL via sqlx

## Key Design Decisions

### gRPC for Agent Communication
- Low overhead, streaming support
- Strongly typed protocol via protobuf
- Bidirectional streaming for real-time metrics

### WebSocket for Real-Time Push
- Real-time metric updates without polling (used by the removed web frontend; the TUI polls REST)
- Subscription model (per-node or all nodes)
- Slow client isolation with backlog limits

### Postgres for Persistence
- ACID guarantees for job state
- Efficient time-series queries with indexed columns
- Aggregated retention policy (2s → 1min → 10min)

### Alert State Machine
- Normal → Pending → Firing → Resolved
- Configurable duration prevents false positives from metric spikes
- Per-node, per-GPU deduplication

## Module Structure

```
crates/
├── common/         # Shared types, config, alert engine, job state machine
├── protocol/       # Generated protobuf code
├── storage/        # PostgreSQL queries and models
├── agent/          # Node agent (metrics collection, job execution, gRPC client)
├── server/         # Central server (gRPC server, REST API, auth)
└── scheduler/      # Job scheduling logic
```
