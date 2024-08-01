use std::path::PathBuf;

use clap::*;

#[derive(Parser, Clone, Default)]
pub struct SharedOpts {
    #[clap(short, long,action = clap::ArgAction::SetTrue)]
    pub debug: Option<bool>,
    #[clap(short, long)]
    config: Option<PathBuf>,
}

#[derive(Parser, Clone, Default)]
/// Run the platform
pub struct Run {
    #[clap(flatten)]
    sharedopts: SharedOpts,
}
#[derive(Parser, Clone)]
/// Show the parsed configuration
pub struct ShowConfig {
    #[clap(flatten)]
    pub sharedopts: SharedOpts,

    #[clap(short, long)]
    pub json: bool,
}

#[derive(Subcommand, Clone)]
pub enum Actions {
    #[clap(name = "run")]
    Run(Run),
    #[clap(name = "show-config")]
    ShowConfig(ShowConfig),
}

#[derive(Parser, Clone)]
pub struct CliOpts {
    #[command(subcommand)]
    pub action: Actions,
}

impl CliOpts {
    pub fn config(&self) -> Option<PathBuf> {
        match &self.action {
            Actions::Run(run) => run.sharedopts.config.clone(),
            Actions::ShowConfig(run) => run.sharedopts.config.clone(),
        }
    }

    pub fn debug(&self) -> bool {
        match &self.action {
            Actions::Run(run) => run.sharedopts.debug.unwrap_or(false),
            Actions::ShowConfig(run) => run.sharedopts.debug.unwrap_or(false),
        }
    }
}
