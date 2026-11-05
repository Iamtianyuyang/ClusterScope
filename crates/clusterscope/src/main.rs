use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "clusterscope",
    version,
    about = "ClusterScope cluster monitoring: one binary, server and agent roles"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the central Server (Web, REST, WebSocket, gRPC).
    Server {
        /// Config file; default /etc/clusterscope/server.yaml.
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,
    },
    /// Run the GPU node Agent.
    Agent {
        #[command(flatten)]
        options: agent::Cli,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Server { config } => server::run(config).await,
        Command::Agent { options } => agent::run(options).await,
    }
}
