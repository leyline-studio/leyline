//! The MCP side: announces the registry and routes each call to its tool.

use base64::Engine as _;
use leyline_sdk::{Library, RevisionOrigin};
use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeRequestParams, InitializeResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::Value;

use crate::registry::ToolId;
use crate::tools::{self, Reply};

const INSTRUCTIONS: &str = "Leyline is a non-destructive RAW photo library. \
    Find photographs with `search`, read them with `get_metadata`, `get_settings` \
    and `render_preview`, and develop them with `apply_edits`: each edit is one \
    revision the photographer can undo. Original files are never modified.";

/// What a writing tool answers while Studio holds the library (ADR 0154 §7).
pub const STUDIO_HAS_IT: &str =
    "Leyline Studio has this library open; close it, or edit from Studio";

pub struct LeylineServer {
    library: Library,
    /// Studio held the library when this server opened it read-only.
    studio_has_it: bool,
}

impl LeylineServer {
    pub fn new(library: Library, studio_has_it: bool) -> LeylineServer {
        LeylineServer {
            library,
            studio_has_it,
        }
    }
}

impl ServerHandler for LeylineServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("leyline", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    /// The client names itself here, and that name becomes the origin of
    /// every revision this session writes (ADR 0154 §6) — self-declared, a
    /// label rather than an identity.
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if !self.studio_has_it {
            self.library
                .declare_origin(RevisionOrigin::mcp(&request.client_info.name));
        }
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(
            ToolId::ALL.into_iter().map(ToolId::describe).collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let Some(tool) = ToolId::from_name(&request.name) else {
            return Err(McpError::invalid_params(
                format!("unknown tool `{}`", request.name),
                None,
            ));
        };
        if self.studio_has_it && !tool.is_read_only() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "`{}` refused: {STUDIO_HAS_IT}",
                tool.name()
            ))])
            .into());
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let library = self.library.clone();
        // The engine is synchronous and a render takes seconds: off the
        // protocol's threads.
        let outcome = tokio::task::spawn_blocking(move || tools::call(&library, tool, arguments))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        // A refusal is a tool result the agent reads and can act on, not a
        // protocol error.
        let result = match outcome {
            Ok(Reply::Json(value)) => CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&value)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            )]),
            Ok(Reply::Jpeg(bytes)) => CallToolResult::success(vec![ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(bytes),
                "image/jpeg",
            )]),
            Err(reason) => CallToolResult::error(vec![ContentBlock::text(reason)]),
        };
        Ok(result.into())
    }
}
