mod cli;
mod cmd;
mod color;
mod config;
mod device;
mod error;
mod image;
mod input;
mod output;
mod sanitize;
mod validate;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::error::CliError;
use crate::output::Emitter;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let emitter = Emitter::new(cli.global.json, cli.global.quiet);

    if let Err(error) = run(&cli, &emitter).await {
        emitter.failure(&error);
        std::process::exit(error.exit_code());
    }
}

async fn run(cli: &Cli, emitter: &Emitter) -> Result<(), CliError> {
    let (file, warnings) = config::load_file();
    // A malformed config file means "your configuration is not being
    // applied" — that must reach the user even under --quiet.
    for warning in &warnings {
        emitter.warn_always(warning);
    }

    let env = config::Env::from_process();

    match &cli.command {
        Command::Text(args) => {
            let settings = config::resolve(&cli.global, &args.style, &env, &file)?;
            let message = input::read_message(&args.message)?;

            let (payload, transliterated) =
                cmd::text::build_payload(args, &settings, &file, &message)?;

            if transliterated {
                emitter.warn(
                    "the message contained characters the bar's bitmap-ASCII fonts cannot \
                     render (smart quotes, dashes, or similar) and was transliterated to \
                     plain ASCII",
                );
            }

            for warning in validate::bounds_warnings(&payload) {
                emitter.warn(&warning);
            }

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }

            let device = device::Device::connect(&settings)?;

            // POST display/draw upserts by id and never removes, so a previous
            // multi-element draw would leave its other elements on screen.
            // Replacing by default makes every invocation independent of history;
            // --keep is for scripts that update one element of a live layout.
            if !args.delivery.keep {
                device.clear().await?;
            }
            device.draw(&payload).await?;

            emitter.success("drawn", Some(&payload))
        }
        Command::Asset(asset) => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;
            match asset {
                cli::AssetCmd::Upload(args) => {
                    cmd::asset::upload(args, &settings, emitter, cli.global.dry_run).await
                }
                cli::AssetCmd::List => {
                    cmd::asset::list(&settings, emitter, cli.global.dry_run).await
                }
                cli::AssetCmd::Delete(args) => {
                    cmd::asset::delete(args, &settings, emitter, cli.global.dry_run).await
                }
            }
        }
        Command::Draw(_) => Err(CliError::runtime("`busy draw` arrives in Task 6")),
        Command::Clear => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;
            cmd::clear::run(&settings, emitter, cli.global.dry_run).await
        }
    }
}
