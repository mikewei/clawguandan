mod cli;
mod server_runtime;

use clap::Parser;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = cli::Cli::parse();
    let err = match cli.command {
        cli::Top::Server { cmd } => {
            let cmd = cmd.unwrap_or(cli::ServerCmd::Status);
            match cmd {
                cli::ServerCmd::Serve { ip, port } => {
                    let resolved_port = match cli::resolve_port(port) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("error: {e}");
                            std::process::exit(1);
                        }
                    };
                    let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
                    rt.block_on(server_runtime::serve(ip, resolved_port))
                }
                cli::ServerCmd::Start { no_auto_use } => {
                    let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
                    rt.block_on(cli::server_start(!no_auto_use))
                }
                cli::ServerCmd::Restart { no_auto_use } => {
                    let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
                    rt.block_on(cli::server_restart(!no_auto_use))
                }
                other => cli::run_from_top(cli::Top::Server { cmd: Some(other) }),
            }
        }
        other => cli::run_from_top(other),
    };

    if let Err(e) = err {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
