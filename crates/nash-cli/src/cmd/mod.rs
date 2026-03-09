pub mod check;
pub mod lsp;

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Check a Nash project for errors
    #[clap(visible_alias = "c")]
    Check(check::Args),
    /// Start the Nash language server over stdio
    Lsp(lsp::Args),
}

impl Cmd {
    pub async fn exec(self) -> miette::Result<()> {
        match self {
            Cmd::Check(args) => args.exec().await,
            Cmd::Lsp(args) => lsp::exec(args).await,
        }
    }
}
