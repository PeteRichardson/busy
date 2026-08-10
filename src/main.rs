mod cli;
mod cmd;
mod color;
mod config;
mod device;
mod error;
mod output;
mod sanitize;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::error::CliError;
use crate::output::Emitter;

fn main() {
    let cli = Cli::parse();
    let emitter = Emitter {
        json: cli.global.json,
        quiet: cli.global.quiet,
    };

    if let Err(error) = run(&cli, emitter) {
        eprintln!("busy: {error}");
        std::process::exit(error.exit_code());
    }
}

fn run(cli: &Cli, emitter: Emitter) -> Result<(), CliError> {
    let (file, warnings) = config::load_file();
    for warning in &warnings {
        emitter.warn(warning);
    }

    let env = config::Env::from_process();

    match &cli.command {
        Command::Text(args) => {
            let settings = config::resolve(&cli.global, &args.style, &env, &file)?;
            // Task 11 replaces this with `input::read_message(&args.message)?`,
            // which adds `-` for stdin.
            let message = args.message.clone();

            let payload = cmd::text::build_payload(args, &settings, &file, &message)?;

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }

            // Sending arrives in Task 7.
            emitter.success("drawn", &payload)
        }
        Command::Clear => Err(CliError::runtime("`busy clear` arrives in Task 12")),
    }
}
