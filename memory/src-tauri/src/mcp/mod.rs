//! MCP (Model Context Protocol) server.
//!
//! Implements JSON-RPC 2.0 over stdio per the MCP spec (2025-11-25):
//!   - `initialize`
//!   - `tools/list`
//!   - `tools/call`
//!   - `notifications/initialized` (ack only)
//!
//! Each `tools/call` is checked against the `permissions` table, touches the
//! relevant entities (Ebbinghaus reinforcement), and is appended to the
//! Merkle-chained, Ed25519-signed `audit_log`.

pub mod server;
pub mod sse;
pub mod tools;

pub use server::serve_stdio;
pub use server::McpContext;
pub use sse::{serve_sse, serve_sse_with_shutdown};
