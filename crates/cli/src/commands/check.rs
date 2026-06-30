use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CheckArgs {
    /// Path to a file or directory to check
    pub path: PathBuf,

    /// Output format
    #[arg(long, default_value = "text", value_parser = ["text", "json", "github"])]
    pub format: String,

    /// Promote warnings to errors and enable stricter rules
    #[arg(long)]
    pub strict: bool,
}

#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn run(args: CheckArgs) -> Result<()> {
    // TODO(LAB-657): implement check subcommand
    tracing::info!(path = %args.path.display(), strict = args.strict, "checking");
    Ok(())
}
