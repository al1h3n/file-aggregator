#![windows_subsystem = "windows"]

//! Entry point: `file-aggregator` with no arguments (or `gui`) opens the window;
//! `add` and `interactive` run in the terminal.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use file_aggregator::aggregator::aggregate_files;
use file_aggregator::gui;
use file_aggregator::security::{validate_path, write_private};

#[derive(Parser)]
#[command(name = "file-aggregator")]
#[command(about = "Aggregate files into a single prompt format", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add files via command line arguments
    Add {
        /// Files to aggregate
        #[arg(required = true)]
        files: Vec<PathBuf>,

        /// Output file path (optional, will copy to clipboard if not provided)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Interactive file selection using gum
    Interactive,
    /// Launch GUI application, optionally with files already in the list
    Gui {
        /// Files to start with
        files: Vec<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Add { files, output }) => handle_cli_add(files, output),
        Some(Commands::Interactive) => handle_interactive(),
        Some(Commands::Gui { files }) => gui::run_gui(files),
        None => gui::run_gui(Vec::new()),
    }
}

fn handle_cli_add(files: Vec<PathBuf>, output: Option<PathBuf>) -> Result<()> {
    let paths = files.iter().map(|p| validate_path(p)).collect::<Result<Vec<_>>>()?;
    let content = aggregate_files(&paths)?;
    output_result(&content, output)
}

fn handle_interactive() -> Result<()> {
    // Check if gum is available
    let gum_check = std::process::Command::new("gum").arg("--version").output();

    if gum_check.is_err() {
        anyhow::bail!("gum is not installed. Install from https://github.com/charmbracelet/gum");
    }

    println!("Select files (press space to select, enter to confirm):");

    let output = std::process::Command::new("gum").arg("file").arg("--all").output().context("Failed to run gum file picker")?;

    if !output.status.success() {
        anyhow::bail!("File selection cancelled");
    }

    let selected = String::from_utf8_lossy(&output.stdout);
    let files: Vec<PathBuf> = selected.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect();

    if files.is_empty() {
        println!("No files selected");
        return Ok(());
    }

    // Ask for output location
    let output_choice =
        std::process::Command::new("gum").arg("choose").arg("Copy to clipboard").arg("Save to file").output().context("Failed to run gum choose")?;

    let choice = String::from_utf8_lossy(&output_choice.stdout);
    let output_path = if choice.trim() == "Save to file" {
        let path_output = std::process::Command::new("gum").arg("input").arg("--placeholder").arg("output.md").output().context("Failed to get output path")?;

        let path_str = String::from_utf8_lossy(&path_output.stdout);
        Some(PathBuf::from(path_str.trim()))
    } else {
        None
    };

    handle_cli_add(files, output_path)
}

/// Saves or copies the aggregated text. It is already sanitized by the aggregator.
fn output_result(content: &str, output_path: Option<PathBuf>) -> Result<()> {
    if let Some(path) = output_path {
        write_private(&path, content)?;
        println!("Saved to: {}", path.display());
    } else {
        // Copy to clipboard (size limit enforced)
        const MAX_CLIPBOARD_SIZE: usize = 10 * 1024 * 1024; // 10MB

        if content.len() > MAX_CLIPBOARD_SIZE {
            anyhow::bail!("Content too large for clipboard ({} bytes). Save to file instead.", content.len());
        }

        let mut clipboard = arboard::Clipboard::new().context("Failed to access clipboard")?;

        clipboard.set_text(content).context("Failed to copy to clipboard")?;

        println!("Copied to clipboard ({} bytes)", content.len());
    }

    Ok(())
}
