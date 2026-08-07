use miette::IntoDiagnostic;

fn main() -> miette::Result<()> {
    // The guard mutates the environment, which is only sound while the
    // process is single-threaded — so it runs before the runtime starts.
    let proxied = nash_cli::proxy::proxy_guard()?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(async {
            if !proxied {
                nash_cli::proxy::maybe_proxy().await?;
            }

            nash_cli::Cli::default().exec().await
        })
}
