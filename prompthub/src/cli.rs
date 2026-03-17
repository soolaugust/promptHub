use clap::{Parser, Subcommand};
use std::path::PathBuf;
use crate::output::OutputFormat;

/// PromptHub — Layered prompt management system
#[derive(Parser, Debug)]
#[command(name = "prompthub", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new PromptHub project (creates Promptfile template)
    Init {
        /// Directory to initialize (default: current directory)
        #[arg(default_value = ".")]
        dir: PathBuf,
    },

    /// Build a Promptfile and output the merged prompt
    Build {
        /// Path to Promptfile (default: ./Promptfile)
        #[arg(default_value = "Promptfile")]
        promptfile: PathBuf,

        /// Override a variable (can be specified multiple times)
        #[arg(long, value_name = "NAME=VALUE", action = clap::ArgAction::Append)]
        var: Vec<String>,

        /// Output format
        #[arg(short, long, value_enum, default_value = "text")]
        output: OutputFormat,

        /// Show warnings
        #[arg(long)]
        warn: bool,
    },

    /// Layer management commands
    #[command(subcommand)]
    Layer(LayerCommands),

    /// Pull a layer from a remote source
    Pull {
        /// Layer reference (e.g. base/code-reviewer:v1.0)
        layer: String,
    },

    /// Search for available layers
    Search {
        /// Search keyword
        keyword: String,
    },

    /// Compare the build output of two Promptfiles
    Diff {
        /// First Promptfile
        first: PathBuf,
        /// Second Promptfile
        second: PathBuf,
    },

    /// Show version history of a layer
    History {
        /// Layer name (e.g. base/code-reviewer)
        layer: String,
    },

    /// Log in to a private registry and store the auth token
    Login {
        /// Registry URL (e.g. https://registry.mycompany.internal)
        registry_url: String,
        /// Use a pre-existing token directly (non-interactive, for CI/AI agents)
        #[arg(long)]
        token: Option<String>,
    },

    /// Log out from a private registry (removes stored token)
    Logout {
        /// Registry URL
        registry_url: String,
    },

    /// Push a layer to a private registry
    Push {
        /// Layer reference with version (e.g. base/my-expert:v1.0)
        layer: String,
        /// Registry source name (default: uses default source)
        #[arg(long)]
        source: Option<String>,
    },

    /// Export local Claude skills or existing layers as PromptHub layer format
    Export {
        /// What to export: "skills" (~/.claude/skills/) or "layers" (./layers/)
        source: ExportSource,
        /// Optional: specific name to export (e.g. "humanizer")
        name: Option<String>,
        /// Output directory (default: ./layers)
        #[arg(long, default_value = "layers")]
        output: String,
        /// Skip similarity analysis after export (default: auto-analyze on full export)
        #[arg(long, default_value_t = false)]
        no_analyze: bool,
        /// Automatically split common chunks into core layers based on similarity analysis
        #[arg(long, default_value_t = false)]
        refactor: bool,
        /// Skip confirmation prompt when used with --refactor
        #[arg(long, default_value_t = false)]
        yes: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ExportSource {
    /// Export Claude Code skills from ~/.claude/skills/
    Skills,
    /// Standardize / fix existing layers in ./layers/
    Layers,
}

#[derive(Subcommand, Debug)]
pub enum LayerCommands {
    /// Create a new layer with template files
    New {
        /// Layer name (e.g. base/my-reviewer or just my-reviewer)
        name: String,
        /// Output directory (default: ./layers)
        #[arg(long, default_value = "layers")]
        dir: PathBuf,
    },

    /// List all locally available layers
    List {
        /// Show only layers in a specific namespace
        #[arg(long)]
        namespace: Option<String>,
    },

    /// Show details of a layer
    Inspect {
        /// Layer name (e.g. base/code-reviewer)
        name: String,
    },

    /// Validate a layer's format
    Validate {
        /// Layer name or path to layer directory
        name: String,
    },
}
