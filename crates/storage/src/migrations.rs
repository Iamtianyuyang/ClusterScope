//! Database migrations.
//!
//! The initial schema is applied inline in `DatabasePool::run_migrations`
//! (see `lib.rs`). This module is reserved for future versioned migrations
//! managed via `sqlx::migrate!`.
