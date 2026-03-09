mod backend;
mod capabilities;

use futures::io::{AsyncRead, AsyncWrite};
use tower_lsp_server::Server;

pub use backend::{Backend, SERVER_NAME, build_service};

/// Run the server over any `futures::io` transport.
///
/// Native stdio, tests, and the eventual WASM transport can all feed into this function.
pub async fn run_server<I, O>(input: I, output: O)
where
    I: AsyncRead + Unpin,
    O: AsyncWrite,
{
    let (service, socket) = build_service();
    Server::new(input, output, socket).serve(service).await;
}
