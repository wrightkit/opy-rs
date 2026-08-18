//! The authoritative structured command model for `opy-cli`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// One command model drives argv parsing, generated help, and completion.
#[derive(Debug, Parser)]
#[command(
    name = "opy-cli",
    disable_version_flag = true,
    disable_help_subcommand = true,
    subcommand_precedence_over_arg = true,
    about = "Workshop-independent OPY frontend tooling",
    after_help = EXIT_CODES
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,

    /// Print the same version identity as the `version` command.
    #[arg(short = 'V', long, global = true, action = clap::ArgAction::SetTrue)]
    pub(crate) version: bool,

    /// Presentation renderer; `auto` detects terminals, CI, and GitHub Actions.
    #[arg(long, global = true, value_enum, default_value_t = RendererArg::Auto)]
    pub(crate) renderer: RendererArg,

    /// ANSI color policy.
    #[arg(long, global = true, value_enum, default_value_t = ColorArg::Auto)]
    pub(crate) color: ColorArg,
}

pub(crate) const EXIT_CODES: &str = "EXIT CODES:
    0  clean or successful operation
    1  source diagnostics
    2  usage or I/O error";

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Parse, preprocess, and resolve; print diagnostics to stderr.
    Check(CheckArgs),
    /// Print the resolved program model as JSON.
    Inspect(FileArgs),
    /// Print the compatibility support matrix or a filtered slice as JSON.
    Support(SupportArgs),
    /// Generate static shell completion from this command model.
    Completion(CompletionArgs),
    /// Show the top-level help.
    Help,
    /// Print crate and frontend protocol identities.
    Version,
}

#[derive(Debug, Args)]
pub(crate) struct CheckArgs {
    #[command(flatten)]
    pub(crate) file: FileArgs,

    /// Output format; JSON contains only the check result and diagnostics.
    #[arg(long, value_enum, default_value_t = OutputFormatArg::Text)]
    pub(crate) format: OutputFormatArg,
}

#[derive(Debug, Args)]
pub(crate) struct FileArgs {
    /// Main OPY source file.
    #[arg(value_name = "MAIN.OPY")]
    pub(crate) main: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct SupportArgs {
    /// Explicitly request the existing JSON output (output is JSON by default).
    #[arg(long)]
    pub(crate) json: bool,

    /// Filter by category or feature id.
    #[arg(value_name = "CATEGORY|FEATURE-ID")]
    pub(crate) filter: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct CompletionArgs {
    /// Shell to generate completion for.
    #[arg(value_enum, value_name = "SHELL")]
    pub(crate) shell: ShellArg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum RendererArg {
    Auto,
    Terminal,
    Plain,
    #[value(name = "github-actions", alias = "github")]
    GithubActions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ColorArg {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormatArg {
    Text,
    #[value(alias = "machine")]
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ShellArg {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell", alias = "pwsh")]
    PowerShell,
}
