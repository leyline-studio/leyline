//! A Model Context Protocol server over the Leyline SDK (ADR 0154).
//!
//! An agent — a program driven by a language model — launches
//! `leyline mcp --library <path>` and speaks MCP to it over stdin and stdout.
//! Every tool is one call into the SDK's façade, listed in [`registry`]; the
//! crate depends on the SDK alone, so a tool that reaches past the façade
//! does not compile (§2).
//!
//! The first surface follows one rule (§4): a tool's effect is nothing, a new
//! revision, or new files — anything an agent does can be read back and
//! walked back with the history the photographer already has.

// ADR 0152: no panic on a production path, no `unsafe` outside raw and cli.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![forbid(unsafe_code)]

mod dto;
mod registry;
mod server;
mod tools;

use std::path::Path;

use leyline_sdk::{Library, StudioLock};
use rmcp::ServiceExt;

/// Serves the library at `root` over stdin/stdout until the client closes
/// the stream.
///
/// Blocks the calling thread: it owns a Tokio runtime for the protocol, and
/// every engine call runs on that runtime's blocking pool, since the engine
/// is synchronous.
///
/// When Leyline Studio has the library open (ADR 0154 §7), the library is
/// opened read-only: the reading tools work and the writing ones refuse by
/// name, so the agent and the photographer never write the same catalog
/// from two engines.
pub fn serve_stdio(root: &Path) -> Result<(), String> {
    let studio_has_it = StudioLock::is_held(root).map_err(|e| e.to_string())?;
    let library = if studio_has_it {
        Library::open_read_only(root)
    } else {
        Library::open(root)
    }
    .map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let service = server::LeylineServer::new(library, studio_has_it)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok(())
    })
}
