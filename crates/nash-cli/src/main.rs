#[tokio::main]
async fn main() -> miette::Result<()> {
    nash_cli::proxy::maybe_proxy().await?;

    let cli = nash_cli::Cli::default();
    cli.exec().await
}
