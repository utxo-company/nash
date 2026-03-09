use miette::Result;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(clap::Args, Debug)]
pub struct Args;

pub async fn exec(_: Args) -> Result<()> {
    nash_language_server::run_server(
        tokio::io::stdin().compat(),
        tokio::io::stdout().compat_write(),
    )
    .await;
    Ok(())
}
