use std::path::PathBuf;

use clap::*;

use crate::DEFAULT_CONFIG_FILE;

#[derive(Parser, Clone, Default)]
pub struct SharedOpts {
    #[clap(short, long,action = clap::ArgAction::SetTrue)]
    pub debug: Option<bool>,
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
    pub sharedopts: SharedOpts,
    // #[clap(short, long)]
    // pub json: bool,
}

#[derive(Subcommand, Clone)]
pub enum Actions {
    #[clap(name = "run")]
    Run(Run),
    #[clap(name = "show-config")]
    ShowConfig(ShowConfig),
    #[clap(name = "export-config-schema")]
    /// Export a JSON schema for the config file
    ExportConfigSchema,
}

#[derive(Parser, Clone)]
pub struct CliOpts {
    #[command(subcommand)]
    pub action: Actions,
}

impl CliOpts {
    pub fn config(&self) -> PathBuf {
        match &self.action {
            Actions::Run(run) => run.sharedopts.config.clone(),
            Actions::ShowConfig(run) => run.sharedopts.config.clone(),
            Actions::ExportConfigSchema => PathBuf::from(DEFAULT_CONFIG_FILE),
        }
    }

    pub fn debug(&self) -> bool {
        match &self.action {
            Actions::Run(run) => run.sharedopts.debug.unwrap_or(false),
            Actions::ShowConfig(run) => run.sharedopts.debug.unwrap_or(false),
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
        ];

        for (args, expected_config) in test_list {
            let args = args.split_whitespace().collect::<Vec<&str>>();
            let opts = CliOpts::parse_from(args);

            assert_eq!(opts.config(), expected_config);
        }
    }

    // TOOD: work out how to run the export subcommand, capture the result and confirm it's doing what it says
}
