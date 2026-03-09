use futures::io::{AsyncRead, AsyncWrite};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    InitializeParams, InitializeResult, InitializedParams, MessageType, ServerCapabilities,
    ServerInfo,
};
use tower_lsp_server::{Client, ClientSocket, LanguageServer, LspService, Server};

pub const SERVER_NAME: &str = "nash-language-server";

pub struct Backend {
    client: Client,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub fn server_capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }

    pub fn server_info() -> ServerInfo {
        ServerInfo {
            name: SERVER_NAME.to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: Self::server_capabilities(),
            server_info: Some(Self::server_info()),
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "nash language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Construct the runtime-agnostic LSP service for the current backend.
pub fn build_service() -> (LspService<Backend>, ClientSocket) {
    LspService::new(Backend::new)
}

/// Run the server over any `futures::io` transport.
///
/// Native stdio and the eventual WASM transport both feed into this function.
pub async fn run_server<I, O>(input: I, output: O)
where
    I: AsyncRead + Unpin,
    O: AsyncWrite,
{
    let (service, socket) = build_service();
    Server::new(input, output, socket).serve(service).await;
}

/// Native stdio entry point.
///
/// This is intentionally excluded from `wasm32`: browsers do not expose stdin/stdout,
/// so the web playground will provide its own transport and call `run_server` directly.
#[cfg(not(target_arch = "wasm32"))]
pub async fn run_stdio() {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    run_server(
        tokio::io::stdin().compat(),
        tokio::io::stdout().compat_write(),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::{Backend, SERVER_NAME, build_service};
    use futures::executor::block_on;
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::ls_types::{InitializeParams, TextDocumentSyncCapability};

    #[test]
    fn build_service_constructs_backend() {
        let (service, _socket) = build_service();
        let initialize = block_on(service.inner().initialize(InitializeParams::default()))
            .expect("backend initialize should succeed");

        assert_eq!(initialize.server_info, Some(Backend::server_info()));
    }

    #[test]
    fn initialize_reports_only_minimal_capabilities() {
        let capabilities = Backend::server_capabilities();

        assert_eq!(capabilities.text_document_sync, None);
        assert_eq!(capabilities.hover_provider, None);
        assert_eq!(capabilities.definition_provider, None);
        assert_eq!(capabilities.references_provider, None);
        assert_eq!(capabilities.completion_provider, None);
        assert_eq!(capabilities.code_action_provider, None);
        assert_eq!(capabilities.document_symbol_provider, None);
    }

    #[test]
    fn initialize_response_matches_backend_surface() {
        let (service, _socket) = build_service();
        let initialize = block_on(service.inner().initialize(InitializeParams::default()))
            .expect("backend initialize should succeed");

        assert_eq!(initialize.capabilities, Backend::server_capabilities());
        assert_eq!(initialize.server_info, Some(Backend::server_info()));
        assert_eq!(Backend::server_info().name, SERVER_NAME);
        assert!(!matches!(
            initialize.capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(_))
        ));
    }
}
