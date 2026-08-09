mod cli;
mod color;
mod device;

use clap::Parser;

fn main() {
    let _cli = cli::Cli::parse();
    // Commands are wired up in later tasks.
}
