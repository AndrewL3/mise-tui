mod action;
mod app;
mod component;
mod config;
mod data;
mod event;
mod layout;
mod registry;
mod theme;
mod widgets;

use std::io::{self, stdout};

use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    install_hooks()?;

    // Load and validate config before entering TUI mode
    let config = config::Config::load()?;
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

    let mut terminal = init_terminal()?;
    let app_result = app::App::new(config)?.run(&mut terminal).await;
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
