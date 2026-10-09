use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct MicordockerCli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub enum Commands {
    Run(RunArgs)
}

#[derive(Parser, Debug)]
pub struct RunArgs {
    pub reference: String,

    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long)]
    pub restrict_syscalls: bool,

    #[arg(short, long)]
    pub envs: Vec<String>,

    #[arg(long, default_value = "missing")]
    pub pull: PullStrategy
}

#[derive(ValueEnum, Debug, Clone)]
pub enum PullStrategy {
    Always,
    Missing,
    Never
}
