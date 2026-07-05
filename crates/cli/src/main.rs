#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "gdscript-lsp",
    version,
    about = "Standalone GDScript Language Server"
)]
struct Cli {
    /// Use stdio for LSP communication (default, accepted for editor compatibility)
    #[arg(long, hide = true)]
    stdio: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Check `GDScript` files for errors (CI mode)
    Check(commands::check::CheckArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Check(args)) => commands::check::run(args),
        None => run_lsp_server().await,
    }
}

async fn run_lsp_server() -> Result<()> {
    use gdscript_lsp::backend::Backend;
    use tower_lsp::{LspService, Server};

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
