use std::io::{self, stdout};
use std::path::PathBuf;

use clap::Parser;
use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};

use mise_tui::app;
use mise_tui::config;
use mise_tui::registry;

#[derive(Parser)]
#[command(
    name = "mise-tui",
    version,
    about = "A configurable TUI system dashboard"
)]
struct Cli {
    /// Path to config file (default: ~/.config/mise-tui/config.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_hooks()?;

    let cli = Cli::parse();
    let config_override = cli.config.is_some();
    let config_path = match cli.config {
        Some(p) => p,
        None => config::config_path()?,
    };

    // Only create default config when no --config override was given.
    if !config_override && !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, config::DEFAULT_CONFIG)?;
    }

    let config = config::Config::load(Some(&config_path))?;
    let validation = config.validate(registry::is_known_type);
    for warning in &validation.warnings {
        eprintln!("config warning: {:?}", warning);
    }
    if validation.has_errors() {
        for err in &validation.errors {
            eprintln!("config error: {}", err);
        }
        std::process::exit(1);
    }

    // Construct app before entering raw mode so widget config errors
    // print to stderr without leaving the terminal in a broken state.
    let mut app = app::App::new(config, config_path)?;

    let mut terminal = init_terminal()?;
    let app_result = app.run(&mut terminal).await;
    restore_terminal()?;
    app_result
}

fn install_hooks() -> Result<()> {
    color_eyre::install()?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));
    Ok(())
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout()))?)
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}
