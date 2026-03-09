use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    InitializeParams, InitializeResult, InitializedParams, MessageType, ServerInfo,
};
use tower_lsp_server::{Client, ClientSocket, LanguageServer, LspService};

use crate::capabilities::server_capabilities;

pub const SERVER_NAME: &str = "nash-language-server";

pub struct Backend {
    client: Client,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self { client }
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
            capabilities: server_capabilities(),
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

pub fn build_service() -> (LspService<Backend>, ClientSocket) {
    LspService::new(Backend::new)
}
