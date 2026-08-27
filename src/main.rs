use clap::{Parser, Subcommand};
use kcv::cmd;
use kcv::error::{Error, Result};
use kcv::store::KeychainStore;

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
    let environment = environment_name(cli.environment)?;
    let store = KeychainStore::open()?;

    match cli.command {
        Commands::Set { assignments } => {
            let count = cmd::set(&store, &environment, &assignments)?;
            let plural = if count == 1 { "" } else { "s" };
            eprintln!("Stored {count} variable{plural} in environment {environment:?}");
            Ok(())
        }
        // Returns only on failure.
        Commands::Exec { argv } => cmd::exec(&store, &environment, &argv).map(|_| ()),
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("kcv: {e}");
        std::process::exit(e.exit_code());
    }
}
