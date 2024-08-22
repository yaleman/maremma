//! Main app CLI-related things

use std::path::PathBuf;

use clap::*;

use crate::prelude::ServiceType;
use crate::DEFAULT_CONFIG_FILE;

#[derive(Parser, Clone, Default, Debug)]
/// Shared configuration options
pub struct SharedOpts {
    #[clap(short, long,action = clap::ArgAction::SetTrue)]
    /// Enable debug logging
    pub debug: Option<bool>,
    #[clap(long,action = clap::ArgAction::SetTrue)]
    /// Enable database debug logging because it's SUPER noisy
    pub db_debug: Option<bool>,

    #[clap(short, long, help=format!("Path to the configuration file. Defaults to {}", crate::DEFAULT_CONFIG_FILE), default_value=crate::DEFAULT_CONFIG_FILE)]
    /// Defaults to [crate::DEFAULT_CONFIG_FILE]
    pub config: PathBuf,
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
    /// Shared options
    pub sharedopts: SharedOpts,
}

#[derive(Parser, Clone, Debug)]
/// Run a single check manually and exit
pub struct OneShotCmd {
    #[clap(flatten)]
    /// Shared options
    pub sharedopts: SharedOpts,
    /// The check to run
    pub check: ServiceType,
    /// Hostname to target
    pub hostname: String,
    /// Extra configuration, parsed as JSON
    pub service_config: String,

    /// Show the config options for the service
    #[clap(long)]
    pub show_config: bool,
}

/// Sub commands
#[derive(Subcommand, Clone)]
pub enum Actions {
    #[clap(name = "run")]
    /// Run the server
    Run(Run),
    #[clap(name = "show-config")]
    /// Show the system configuration
    ShowConfig(ShowConfig),
    #[clap(name = "export-config-schema")]
    /// Export a JSON schema for the config file
    ExportConfigSchema,
    #[clap(name = "oneshot")]
    /// Run a single check manually and exit
    OneShot(OneShotCmd),
}

#[derive(Parser, Clone)]
/// Maremma, protecting the herd.
pub struct CliOpts {
    /// Subcommands
    #[command(subcommand)]
    pub action: Actions,
}

impl CliOpts {
    /// Gets the config path
    pub fn config(&self) -> PathBuf {
        match &self.action {
            Actions::Run(run) => run.sharedopts.config.clone(),
            Actions::ShowConfig(run) => run.sharedopts.config.clone(),
            Actions::OneShot(run) => run.sharedopts.config.clone(),
            Actions::ExportConfigSchema => PathBuf::from(DEFAULT_CONFIG_FILE),
        }
    }

    /// Gets the debug field
    pub fn debug(&self) -> bool {
        match &self.action {
            Actions::Run(run) => run.sharedopts.debug.unwrap_or(false),
            Actions::ShowConfig(run) => run.sharedopts.debug.unwrap_or(false),
            Actions::OneShot(run) => run.sharedopts.debug.unwrap_or(false),
            Actions::ExportConfigSchema => false,
        }
    }
    /// Gets the db_debug field
    pub fn db_debug(&self) -> bool {
        match &self.action {
            Actions::Run(run) => run.sharedopts.db_debug.unwrap_or(false),
            Actions::ShowConfig(run) => run.sharedopts.db_debug.unwrap_or(false),
            Actions::OneShot(run) => run.sharedopts.db_debug.unwrap_or(false),
            Actions::ExportConfigSchema => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_cliopts() {
        let test_list = vec![
            ("maremma run --debug", true),
            ("maremma run", false),
            ("maremma show-config --debug", true),
            ("maremma show-config", false),
            ("maremma export-config-schema", false),
        ];

        for (args, debug) in test_list {
            let args = args.split_whitespace().collect::<Vec<&str>>();
            let opts = CliOpts::parse_from(args);

            assert_eq!(opts.debug(), debug);
        }

        let test_list = vec![
            (
                "maremma run --config /tmp/config.toml",
                PathBuf::from("/tmp/config.toml"),
            ),
            (
                "maremma show-config --config /tmp/config.toml",
                PathBuf::from("/tmp/config.toml"),
            ),
            ("maremma run", PathBuf::from(crate::DEFAULT_CONFIG_FILE)),
            (
                "maremma show-config",
                PathBuf::from(crate::DEFAULT_CONFIG_FILE),
            ),
            (
                "maremma export-config-schema",
                PathBuf::from(crate::DEFAULT_CONFIG_FILE),
            ),
        ];

        for (args, expected_config) in test_list {
            let args = args.split_whitespace().collect::<Vec<&str>>();
            let opts = CliOpts::parse_from(args);

            assert_eq!(opts.config(), expected_config);
        }
    }

    // TODO: work out how to run the export subcommand, capture the result and confirm it's doing what it says

    #[test]
    fn test_db_debug() {
        let test_list = vec![
            ("maremma run --db-debug", true),
            ("maremma run", false),
            ("maremma show-config --db-debug", true),
            ("maremma show-config", false),
            ("maremma export-config-schema", false),
        ];

        for (args, db_debug) in test_list {
            let args = args.split_whitespace().collect::<Vec<&str>>();
            let opts = CliOpts::parse_from(args);

            assert_eq!(opts.db_debug(), db_debug);
        }
    }
}
