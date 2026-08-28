use clap::{Parser, Subcommand};
use kcv::cmd;
use kcv::error::{Error, Result};
use kcv::prompt;
use kcv::store::KeychainStore;
use std::path::PathBuf;

/// Store environment secrets in the macOS keychain and inject them into a
/// process. All of an environment's secrets live in one keychain item, so
/// reading them costs a single authorization.
#[derive(Parser)]
#[command(name = "kcv", version, about, long_about = None)]
struct Cli {
    /// Environment name. Defaults to $KCV_ENV.
    #[arg(short, long, global = true)]
    environment: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Store one or more variables. An argument without '=' is prompted for.
    Set {
        /// KEY=VALUE pairs, or a bare KEY to be prompted for.
        #[arg(required = true, value_name = "KEY=VALUE")]
        assignments: Vec<String>,
    },
    /// List the environment's variable names, one per line.
    List,
    /// Print one variable's value to stdout.
    Get {
        /// Name of the variable to read.
        #[arg(value_name = "KEY")]
        key: String,
    },
    /// Print the whole environment to stdout in .env format.
    Export,
    /// List every environment, one per line.
    #[command(visible_alias = "envs")]
    Environments,
    /// Remove one or more variables from the environment.
    Unset {
        /// Names of the variables to remove.
        #[arg(required = true, value_name = "KEY")]
        keys: Vec<String>,
    },
    /// Import variables from a .env file, then offer to delete it.
    Import {
        /// Path to the .env file to read.
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },
    /// Run a command with the environment's secrets injected.
    Exec {
        /// The command and its arguments, after `--`.
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "COMMAND"
        )]
        argv: Vec<String>,
    },
}

fn environment_name(flag: Option<String>) -> Result<String> {
    flag.or_else(|| std::env::var("KCV_ENV").ok())
        .filter(|s| !s.is_empty())
        .ok_or(Error::MissingEnvironment)
}

fn run(cli: Cli) -> Result<()> {
    let store = KeychainStore::open()?;
    // Resolved per arm: `environments` spans every environment and so takes
    // no environment name.
    let environment = || environment_name(cli.environment.clone());

    match cli.command {
        Commands::Environments => {
            for name in cmd::environments(&store)? {
                println!("{name}");
            }
            Ok(())
        }
        Commands::Set { assignments } => {
            let environment = environment()?;
            let count = cmd::set(&store, &environment, &assignments)?;
            let plural = if count == 1 { "" } else { "s" };
            eprintln!("Stored {count} variable{plural} in environment {environment:?}");
            Ok(())
        }
        Commands::List => {
            let environment = environment()?;
            for key in cmd::list(&store, &environment)? {
                println!("{key}");
            }
            Ok(())
        }
        Commands::Get { key } => {
            let environment = environment()?;
            println!("{}", cmd::get(&store, &environment, &key)?);
            Ok(())
        }
        Commands::Export => {
            let environment = environment()?;
            // format already ends every line, so print without adding another.
            print!("{}", cmd::export(&store, &environment)?);
            Ok(())
        }
        Commands::Unset { keys } => {
            let environment = environment()?;
            let count = cmd::unset(&store, &environment, &keys)?;
            let plural = if count == 1 { "" } else { "s" };
            eprintln!("Removed {count} variable{plural} from environment {environment:?}");
            Ok(())
        }
        Commands::Import { path } => {
            let environment = environment()?;
            let count = cmd::import(&store, &environment, &path)?;
            let plural = if count == 1 { "" } else { "s" };
            eprintln!("Imported {count} variable{plural} into environment {environment:?}");
            offer_to_delete(&path)
        }
        // Returns only on failure.
        Commands::Exec { argv } => cmd::exec(&store, &environment()?, &argv).map(|_| ()),
    }
}

/// Asks whether to remove the imported file. Without a terminal there is
/// nobody to ask, so the file is kept and we say so rather than staying quiet
/// about a plaintext file still sitting on disk.
fn offer_to_delete(path: &std::path::Path) -> Result<()> {
    let display = path.display();
    if !prompt::is_tty() {
        eprintln!("Kept {display}");
        return Ok(());
    }
    if prompt::confirm(&format!("Delete {display}?"))? {
        std::fs::remove_file(path).map_err(|source| Error::ReadFile {
            path: display.to_string(),
            source,
        })?;
        eprintln!("Deleted {display}");
    } else {
        eprintln!("Kept {display}");
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("kcv: {e}");
        std::process::exit(e.exit_code());
    }
}
