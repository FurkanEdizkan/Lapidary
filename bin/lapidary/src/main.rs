//! Desktop binary. Ships properly in Phase 4; the subcommands are declared here so the
//! CLI surface is stable from the start.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "lapidary",
    version,
    about = "Lapidary — a visual index for 3D part libraries"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Watch a workspace directory and return changed files as new revisions.
    Agent,
    /// Run a job worker against a Lapidary server.
    Worker,
    /// Start a local Lapidary stack.
    Up,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let name = match cli.command {
        Commands::Agent => "agent",
        Commands::Worker => "worker",
        Commands::Up => "up",
    };
    bail!(
        "`lapidary {name}` arrives in Phase 4. Until then, run the stack with `podman compose -f deploy/compose.yaml up`."
    )
}
