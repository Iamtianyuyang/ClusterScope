# ClusterScope Architecture

## Overview

ClusterScope is a distributed CPU / GPU / process monitoring platform. It consists of:

1. **Agent** — a lightweight Rust service on each compute node that collects metrics
2. **Central Server** — aggregates Agent data, persists history, and serves REST / WebSocket APIs
3. **Web** — a dependency-free read-only dashboard embedded in the Server binary
4. **PostgreSQL** — persistent storage for metrics history and compatible management APIs

## Architecture Diagram

```
┌──────────────────┐
│  Web 浏览器       │
│  CPU/GPU/进程     │
└────────┬─────────┘
         │ REST + WebSocket
┌────────▼─────────┐
│  Central Server  │
│                  │
│ Static Web UI    │
│ Node Registry    │
│ Metrics Service  │
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

- **Agent → Server**: gRPC streaming for metrics and heartbeats
- **Web → Server**: same-origin REST polling plus WebSocket invalidation
- **Server → DB**: PostgreSQL via sqlx

## Web Dashboard

The `web/` directory contains native HTML, CSS, and JavaScript modules. It has no runtime package or build-step dependency. `crates/server/src/web.rs` embeds those assets at compile time and serves them with a restrictive Content Security Policy.

The product intentionally has only three concerns:

1. Cluster CPU / GPU overview
2. Live and historical trends
3. GPU and Top CPU processes

Job and alert management APIs remain available for compatibility, but are not part of the Web product surface. Unknown metrics are rendered as `—`, never fabricated as zero. The logo-themed dark palette, reduced-motion preference, keyboard, touch, and narrow-screen layouts are handled locally without a UI framework.

## Key Design Decisions

### gRPC for Agent Communication
- Low overhead, streaming support
- Strongly typed protocol via protobuf

### REST + WebSocket for Monitoring
- REST provides complete snapshots and history
- WebSocket messages trigger throttled refreshes when new metrics arrive
- Polling remains a working fallback when WebSocket is unavailable

### Embedded Static Web Assets
- One portable Server binary; no Nginx or separate frontend process
- Same-origin API avoids CORS and keeps authentication simple
- Assets are cached with revalidation so updates take effect after Server upgrades

### Postgres for Persistence
- ACID guarantees
- Indexed time-series queries
- Raw, hourly, and daily retention tiers
