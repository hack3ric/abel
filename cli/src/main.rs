mod server;

use clap::{Parser, Subcommand};
use server::{ServerArgs, HALF_NUM_CPUS};

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
  #[clap(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  /// Run Abel server.
  Server {
    #[clap(flatten)]
    args: ServerArgs,
  },
}

fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  match args.command {
    Command::Server { args } => tokio::runtime::Builder::new_multi_thread()
      .enable_all()
      .worker_threads(*HALF_NUM_CPUS)
      .build()
      .unwrap()
      .block_on(server::run(args)),
  }
}
