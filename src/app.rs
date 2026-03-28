use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::action::Direction;
use crate::component::Component;
use crate::config::{Config, KeybindsConfig, NormalizedConfig};
use crate::data::{
    DataUpdate, spawn_hyprland_task, spawn_packages_task, spawn_services_task, spawn_sysinfo_task,
};
use crate::editor::{EditAction, EditMode};
use crate::event::{Event, EventHandler};
use crate::input::{InputMode, KeyDispatcher};
use crate::layout::LayoutEngine;
use crate::profile::ProfileManager;
use crate::registry;
use crate::theme::{Theme, ThemeConfig};

struct ExternalTaskHandle {
    #[allow(dead_code)]
    instance_id: String,
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

pub struct App {
    should_quit: bool,
    components: HashMap<String, Box<dyn Component>>,
    layout: LayoutEngine,
    theme: Theme,
    focus: Option<(usize, usize)>,
    tick_rate_ms: u64,
    config_path: PathBuf,
    cancel: CancellationToken,
    sysinfo_cancel: CancellationToken,
    sysinfo_handle: Option<tokio::task::JoinHandle<()>>,
    notification: Option<(String, std::time::Instant)>,
    notification_duration: std::time::Duration,
    config_tx: Option<mpsc::Sender<()>>,
    error_states: HashMap<String, String>,
    sysinfo_retry_count: u32,
    sysinfo_max_retries: u32,
    sysinfo_restart_at: Option<tokio::time::Instant>,
    sysinfo_dead: bool,
    sysinfo_restarting: bool,
    sysinfo_disconnected: bool,
    show_help: bool,
    interact_mode: bool,
    undersized_panels: HashSet<String>,
    external_handles: Vec<ExternalTaskHandle>,
    external_disconnected: bool,
    hyprland_handle: Option<ExternalTaskHandle>,
    hyprland_disconnected: bool,
    profile_manager: ProfileManager,
    widget_configs: HashMap<String, toml::Value>,
    theme_config: ThemeConfig,
    keybinds_config: KeybindsConfig,
    dispatcher: KeyDispatcher,
    edit_mode: Option<EditMode>,
}

impl App {
    pub fn new(config: Config, config_path: PathBuf) -> Result<Self> {
        let normalized = config.normalize()?;
        let active_profile = normalized.active_profile_config();

        let theme = Theme::from_config(&normalized.theme)?;
        let layout = LayoutEngine::from_config(active_profile)?;

        let mut components: HashMap<String, Box<dyn Component>> = HashMap::new();

        for panel in &active_profile.panels {
            let instance_id = panel.instance_id().to_string();
            let widget_config = normalized.widgets.get(&instance_id).cloned();

            let descriptor = registry::get_descriptor(&panel.widget_type)
                .expect("unknown widget type should have been caught by validation");

            let component = (descriptor.constructor)(
                instance_id.clone(),
                panel.widget_type.clone(),
                widget_config,
            )?;
            components.insert(instance_id, component);
        }

        // Set initial focus to first occupied cell in reading order
        let focus = layout.occupied_cells().first().copied();

        let tick_rate_ms = normalized.general.tick_rate;

        let cancel = CancellationToken::new();
        let sysinfo_cancel = cancel.child_token();

        let profile_manager = ProfileManager::new(
            normalized.profiles.clone(),
            &normalized.default_profile,
        )?;

        let dispatcher = KeyDispatcher::new(&normalized.keybinds)?;

        Ok(Self {
            should_quit: false,
            components,
            layout,
            theme,
            focus,
            tick_rate_ms,
            config_path,
            cancel,
            sysinfo_cancel,
            sysinfo_handle: None,
            notification: None,
            notification_duration: std::time::Duration::from_secs(5),
            config_tx: None,
            error_states: HashMap::new(),
            sysinfo_retry_count: 0,
            sysinfo_max_retries: 5,
            sysinfo_restart_at: None,
            sysinfo_dead: false,
            sysinfo_restarting: false,
            sysinfo_disconnected: false,
            show_help: false,
            interact_mode: false,
            undersized_panels: HashSet::new(),
            external_handles: Vec::new(),
            external_disconnected: false,
            hyprland_handle: None,
            hyprland_disconnected: false,
            profile_manager,
            widget_configs: normalized.widgets.clone(),
            theme_config: normalized.theme.clone(),
            keybinds_config: normalized.keybinds.clone(),
            dispatcher,
            edit_mode: None,
        })
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        let mut events = EventHandler::new(std::time::Duration::from_millis(self.tick_rate_ms));

        // Spawn sysinfo polling task.
        // The sysinfo task owns the only Sender — when it dies, the channel
        // closes and data_rx.recv() returns None, triggering supervision.
        let (data_tx, mut data_rx) = mpsc::channel::<DataUpdate>(32);
        self.sysinfo_handle = Some(spawn_sysinfo_task(data_tx, self.sysinfo_cancel.clone()));

        // Config file watcher
        let (config_tx, mut config_rx) = mpsc::channel::<()>(4);
        self.config_tx = Some(config_tx.clone());
        let watcher_ready =
            spawn_config_watcher(self.config_path.clone(), config_tx, self.cancel.clone());
        match watcher_ready {
            Some((_handle, ready_rx)) => {
                // Check watcher startup result asynchronously — if it fails,
                // we'll catch it when the oneshot resolves in the first tick.
                if let Ok(false) = ready_rx.await {
                    self.set_notification(
                        "Config watcher failed to start (use 'r' to reload)".to_string(),
                    );
                }
            }
            None => {
                self.set_notification(
                    "Config watcher failed to start (use 'r' to reload)".to_string(),
                );
            }
        }

        // Spawn external tasks (packages, services) with separate channel
        let (external_tx, mut external_rx) = mpsc::channel::<DataUpdate>(16);
        self.spawn_external_tasks(&external_tx);
        drop(external_tx); // only cloned senders remain — channel closes when all tasks finish

        // Spawn Hyprland task if workspaces widget exists
        let (hyprland_tx, mut hyprland_rx) = mpsc::channel::<DataUpdate>(16);
        let has_workspaces = self
            .components
            .values()
            .any(|c| c.widget_type() == "workspaces");
        if has_workspaces {
            let hyprland_cancel = self.cancel.child_token();
            let handle = spawn_hyprland_task(hyprland_tx.clone(), hyprland_cancel.clone());
            self.hyprland_handle = Some(ExternalTaskHandle {
                instance_id: "workspaces".to_string(),
                cancel: hyprland_cancel,
                handle,
            });
        }
        drop(hyprland_tx);

        let loop_result: Result<()> = async {
            while !self.should_quit {
                tokio::select! {
                    event = events.next() => {
                        let event = event?;
                        match event {
                            Event::Tick => {
                                let mut actions = Vec::new();

                                // Drain any remaining data updates before drawing
                                while let Ok(update) = data_rx.try_recv() {
                                    let ids: Vec<String> = self.components.keys().cloned().collect();
                                    for id in &ids {
                                        if let Some(component) = self.components.get_mut(id) {
                                            match component.handle_data(&update) {
                                                Ok(action) => {
                                                    if update.matches_widget_type(component.widget_type()) {
                                                        self.error_states.remove(id);
                                                    }
                                                    if let Some(a) = action { actions.push(a); }
                                                }
                                                Err(e) => { self.error_states.insert(id.clone(), e.to_string()); }
                                            }
                                        }
                                    }
                                }

                                for component in self.components.values_mut() {
                                    if let Some(action) = component.update()? {
                                        actions.push(action);
                                    }
                                }
                                for action in actions {
                                    self.handle_action(action)?;
                                }
                                self.draw(terminal)?;
                            }
                            Event::Key(key) => {
                                let was_help = self.show_help;
                                self.handle_key(key)?;
                                if self.show_help != was_help {
                                    self.draw(terminal)?;
                                }
                            }
                            Event::Resize(..) => {
                                self.draw(terminal)?;
                            }
                        }
                    }
                    result = data_rx.recv(), if !self.sysinfo_dead && !self.sysinfo_disconnected => {
                        match result {
                            Some(update) => {
                                self.sysinfo_retry_count = 0;
                                let mut actions = Vec::new();
                                let ids: Vec<String> = self.components.keys().cloned().collect();
                                for id in &ids {
                                    if let Some(component) = self.components.get_mut(id) {
                                        match component.handle_data(&update) {
                                            Ok(action) => {
                                                if update.matches_widget_type(component.widget_type()) {
                                                    self.error_states.remove(id);
                                                }
                                                if let Some(a) = action { actions.push(a); }
                                            }
                                            Err(e) => { self.error_states.insert(id.clone(), e.to_string()); }
                                        }
                                    }
                                }
                                for action in actions {
                                    self.handle_action(action)?;
                                }
                            }
                            None => {
                                // Stop polling this closed channel until
                                // the restart timer installs a fresh one.
                                self.sysinfo_disconnected = true;

                                if self.sysinfo_restarting {
                                    // Expected during reload — do nothing
                                } else if self.sysinfo_retry_count < self.sysinfo_max_retries {
                                    self.sysinfo_retry_count += 1;
                                    let backoff_secs = (1u64 << (self.sysinfo_retry_count - 1)).min(30);
                                    self.set_notification(format!(
                                        "Data source disconnected, restarting in {backoff_secs}s..."
                                    ));
                                    self.sysinfo_restart_at = Some(
                                        tokio::time::Instant::now() + Duration::from_secs(backoff_secs)
                                    );
                                } else {
                                    self.sysinfo_dead = true;
                                    self.set_notification(
                                        "Data source failed after 5 retries. Restart the app.".to_string()
                                    );
                                }
                            }
                        }
                    }
                    // Sysinfo restart timer
                    _ = async {
                        match self.sysinfo_restart_at {
                            Some(deadline) => tokio::time::sleep_until(deadline).await,
                            None => std::future::pending::<()>().await,
                        }
                    }, if self.sysinfo_restart_at.is_some() => {
                        self.sysinfo_restart_at = None;
                        let new_cancel = self.cancel.child_token();
                        self.sysinfo_cancel = new_cancel.clone();
                        let (new_tx, new_rx) = mpsc::channel::<DataUpdate>(32);
                        self.sysinfo_handle = Some(spawn_sysinfo_task(new_tx, new_cancel));
                        data_rx = new_rx;
                        self.sysinfo_disconnected = false;
                        self.set_notification("Data source reconnected".to_string());
                    }
                    _ = config_rx.recv() => {
                        self.reload_config(&mut data_rx, &mut external_rx, &mut hyprland_rx);
                    }
                    result = external_rx.recv(), if !self.external_disconnected => {
                        match result {
                            Some(update) => {
                                let ids: Vec<String> = self.components.keys().cloned().collect();
                                for id in &ids {
                                    if let Some(component) = self.components.get_mut(id) {
                                        match component.handle_data(&update) {
                                            Ok(action) => {
                                                if update.matches_widget_type(component.widget_type()) {
                                                    self.error_states.remove(id);
                                                }
                                                if let Some(a) = action { self.handle_action(a)?; }
                                            }
                                            Err(e) => { self.error_states.insert(id.clone(), e.to_string()); }
                                        }
                                    }
                                }
                            }
                            None => {
                                self.external_disconnected = true;
                            }
                        }
                    }
                    result = hyprland_rx.recv(), if !self.hyprland_disconnected => {
                        match result {
                            Some(update) => {
                                let ids: Vec<String> = self.components.keys().cloned().collect();
                                for id in &ids {
                                    if let Some(component) = self.components.get_mut(id) {
                                        match component.handle_data(&update) {
                                            Ok(action) => {
                                                if update.matches_widget_type(component.widget_type()) {
                                                    self.error_states.remove(id);
                                                }
                                                if let Some(a) = action { self.handle_action(a)?; }
                                            }
                                            Err(e) => { self.error_states.insert(id.clone(), e.to_string()); }
                                        }
                                    }
                                }
                            }
                            None => {
                                self.hyprland_disconnected = true;
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        .await;

        // Shutdown external tasks
        for handle in self.external_handles.drain(..) {
            handle.cancel.cancel();
            handle.handle.abort();
        }

        // Shutdown Hyprland task
        if let Some(handle) = self.hyprland_handle.take() {
            handle.cancel.cancel();
            handle.handle.abort();
        }

        // Always shutdown sysinfo task, even on error
        self.cancel.cancel();
        if let Some(handle) = self.sysinfo_handle.take() {
            let abort = handle.abort_handle();
            if tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .is_err()
            {
                abort.abort();
            }
        }

        loop_result
    }

    fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Result<()> {
        let mode = if self.show_help {
            InputMode::HelpOverlay
        } else if self.edit_mode.is_some() {
            InputMode::Edit
        } else if self.interact_mode {
            InputMode::Interact
        } else {
            InputMode::Normal
        };

        if let Some(action) = self.dispatcher.resolve(key, mode) {
            self.handle_action(action)?;
        } else if (mode == InputMode::Interact || mode == InputMode::Normal)
            && let Some(focus) = self.focus
            && let Some(id) = self.layout.instance_at(focus.0, focus.1)
            && let Some(component) = self.components.get_mut(id)
            && let Some(action) = component.handle_event(&crate::event::Event::Key(key))?
        {
            self.handle_action(action)?;
        }
        Ok(())
    }

    fn handle_action(&mut self, action: crate::action::Action) -> Result<()> {
        use crate::action::Action;
        match action {
            Action::Quit => self.should_quit = true,
            Action::Notify(msg) => self.set_notification(msg),
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::FocusNext => self.focus_next(),
            Action::FocusPrev => self.focus_prev(),
            Action::FocusDirection(dir) => self.focus_direction(dir),
            Action::ReloadConfig => {
                if let Some(tx) = &self.config_tx {
                    let _ = tx.try_send(());
                }
            }
            Action::EnterInteract => {
                if self.edit_mode.is_some() {
                    return Ok(());
                }
                if let Some(focus) = self.focus
                    && let Some(id) = self.layout.instance_at(focus.0, focus.1)
                    && !self.undersized_panels.contains(id)
                {
                    self.interact_mode = true;
                    if let Some(component) = self.components.get_mut(id) {
                        component.notify_interact(true);
                    }
                }
            }
            Action::ExitInteract => {
                self.interact_mode = false;
                if let Some(focus) = self.focus
                    && let Some(id) = self.layout.instance_at(focus.0, focus.1)
                    && let Some(component) = self.components.get_mut(id)
                {
                    component.notify_interact(false);
                }
            }
            Action::SwitchProfile(direction) => {
                if self.edit_mode.is_some() || self.interact_mode {
                    return Ok(());
                }
                let profile = match direction {
                    crate::action::ProfileDirection::Next => self.profile_manager.next().clone(),
                    crate::action::ProfileDirection::Prev => self.profile_manager.prev().clone(),
                };
                match self.apply_profile_switch(&profile) {
                    Ok(()) => {
                        self.set_notification(format!(
                            "Switched to profile: {}",
                            self.profile_manager.active_name()
                        ));
                    }
                    Err(e) => {
                        self.set_notification(format!("Profile switch failed: {e}"));
                    }
                }
            }
            Action::EnterEditMode => {
                if self.interact_mode || self.edit_mode.is_some() {
                    return Ok(());
                }
                let layout = self.profile_manager.active_layout().clone();
                self.edit_mode = Some(EditMode::enter(layout));
                self.interact_mode = false;
            }
            Action::EditKey(key) => {
                if let Some(ref mut editor) = self.edit_mode {
                    let edit_action = editor.handle_key(key);
                    self.handle_edit_action(edit_action)?;
                }
            }
            Action::ExitEditMode => {
                self.edit_mode = None;
            }
            Action::SaveLayout => {
                // Handled via EditAction::Save in handle_edit_action
            }
            Action::Tick | Action::Resize(..) => {}
        }
        Ok(())
    }

    fn set_notification(&mut self, msg: String) {
        self.notification = Some((msg, std::time::Instant::now()));
    }

    fn apply_profile_switch(&mut self, profile: &crate::config::ProfileConfig) -> Result<()> {
        let new_layout = LayoutEngine::from_config(profile)?;
        let mut new_components: HashMap<String, Box<dyn Component>> = HashMap::new();
        for panel in &profile.panels {
            let instance_id = panel.instance_id().to_string();
            let widget_config = self.widget_configs.get(&instance_id).cloned();
            let descriptor = match registry::get_descriptor(&panel.widget_type) {
                Some(d) => d,
                None => continue,
            };
            let component = (descriptor.constructor)(
                instance_id.clone(),
                panel.widget_type.clone(),
                widget_config,
            )?;
            new_components.insert(instance_id, component);
        }
        self.components = new_components;
        self.layout = new_layout;
        self.focus = self.layout.occupied_cells().first().copied();
        self.interact_mode = false;
        self.error_states.clear();
        Ok(())
    }

    fn handle_edit_action(&mut self, action: EditAction) -> Result<()> {
        match action {
            EditAction::LayoutChanged { layout, added_widget, removed_widget } => {
                match LayoutEngine::from_config(&layout) {
                    Ok(new_layout) => {
                        if let Some((ref id, ref wtype)) = added_widget
                            && let Some(descriptor) = registry::get_descriptor(wtype)
                        {
                            let config = self.widget_configs.get(id).cloned();
                            if let Ok(component) = (descriptor.constructor)(
                                id.clone(), wtype.clone(), config,
                            ) {
                                self.components.insert(id.clone(), component);
                            }
                        }
                        if let Some(ref id) = removed_widget {
                            self.components.remove(id);
                        }
                        self.layout = new_layout;
                        self.focus = self.layout.occupied_cells().first().copied();
                    }
                    Err(_) => {
                        self.set_notification("Invalid layout change".to_string());
                    }
                }
            }
            EditAction::Save => {
                if let Some(ref editor) = self.edit_mode {
                    let working = editor.working_layout().clone();
                    let active_name = self.profile_manager.active_name().to_string();
                    self.profile_manager.update_profile(&active_name, working);
                    let save_config = NormalizedConfig {
                        general: crate::config::GeneralConfig {
                            tick_rate: self.tick_rate_ms,
                            default_profile: Some(self.profile_manager.active_name().to_string()),
                        },
                        profiles: self.profile_manager.profiles_map().clone(),
                        default_profile: self.profile_manager.active_name().to_string(),
                        theme: self.theme_config.clone(),
                        keybinds: self.keybinds_config.clone(),
                        widgets: self.widget_configs.clone(),
                    };
                    match self.profile_manager.save_to_file(&save_config, &self.config_path) {
                        Ok(()) => self.set_notification("Layout saved".to_string()),
                        Err(e) => self.set_notification(format!("Save failed: {e}")),
                    }
                }
            }
            EditAction::Exit => {
                self.edit_mode = None;
                // Revert to saved layout
                let profile = self.profile_manager.active_layout().clone();
                if let Ok(new_layout) = LayoutEngine::from_config(&profile) {
                    let mut new_components: HashMap<String, Box<dyn Component>> = HashMap::new();
                    for panel in &profile.panels {
                        let id = panel.instance_id().to_string();
                        let wconfig = self.widget_configs.get(&id).cloned();
                        if let Some(desc) = registry::get_descriptor(&panel.widget_type)
                            && let Ok(c) = (desc.constructor)(id.clone(), panel.widget_type.clone(), wconfig)
                        {
                            new_components.insert(id, c);
                        }
                    }
                    self.components = new_components;
                    self.layout = new_layout;
                    self.focus = self.layout.occupied_cells().first().copied();
                }
            }
            EditAction::None => {}
        }
        Ok(())
    }

    fn reload_config(
        &mut self,
        data_rx: &mut mpsc::Receiver<DataUpdate>,
        external_rx: &mut mpsc::Receiver<DataUpdate>,
        hyprland_rx: &mut mpsc::Receiver<DataUpdate>,
    ) {
        // 1. Load and validate
        let new_config = match crate::config::Config::load(Some(&self.config_path)) {
            Ok(c) => c,
            Err(e) => {
                self.set_notification(format!("Config reload failed: {e}"));
                return;
            }
        };

        let normalized = match new_config.normalize() {
            Ok(n) => n,
            Err(e) => {
                self.set_notification(format!("Config reload failed: {e}"));
                return;
            }
        };

        let validation = normalized.validate(registry::is_known_type);
        if validation.has_errors() {
            let msg = validation
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            self.set_notification(format!("Config reload failed: {msg}"));
            return;
        }

        let active_profile = normalized.active_profile_config();

        // 2. Build new state
        let new_theme = match Theme::from_config(&normalized.theme) {
            Ok(t) => t,
            Err(e) => {
                self.set_notification(format!("Config reload failed: {e}"));
                return;
            }
        };

        let new_layout = match LayoutEngine::from_config(active_profile) {
            Ok(l) => l,
            Err(e) => {
                self.set_notification(format!("Config reload failed: {e}"));
                return;
            }
        };

        let mut new_components: HashMap<String, Box<dyn Component>> = HashMap::new();

        for panel in &active_profile.panels {
            let instance_id = panel.instance_id().to_string();
            let widget_config = normalized.widgets.get(&instance_id).cloned();

            let descriptor = match registry::get_descriptor(&panel.widget_type) {
                Some(d) => d,
                None => {
                    self.set_notification(format!(
                        "Config reload failed: unknown widget type '{}'",
                        panel.widget_type
                    ));
                    return;
                }
            };

            let mut component = match (descriptor.constructor)(
                instance_id.clone(),
                panel.widget_type.clone(),
                widget_config,
            ) {
                Ok(c) => c,
                Err(e) => {
                    self.set_notification(format!("Config reload failed: {e}"));
                    return;
                }
            };

            // Transfer state from old widget with same ID and same widget type
            if let Some(old_component) = self.components.get(&instance_id)
                && old_component.widget_type() == component.widget_type()
            {
                component.transfer_state(old_component.as_ref());
            }

            new_components.insert(instance_id, component);
        }

        // Rebuild ProfileManager and KeyDispatcher
        let new_profile_manager = match ProfileManager::new(
            normalized.profiles.clone(),
            &normalized.default_profile,
        ) {
            Ok(pm) => pm,
            Err(e) => {
                self.set_notification(format!("Config reload failed: {e}"));
                return;
            }
        };

        let new_dispatcher = match KeyDispatcher::new(&normalized.keybinds) {
            Ok(d) => d,
            Err(e) => {
                self.set_notification(format!("Config reload failed: {e}"));
                return;
            }
        };

        // 3. Swap
        let tick_rate_changed = normalized.general.tick_rate != self.tick_rate_ms;
        self.components = new_components;
        self.layout = new_layout;
        self.theme = new_theme;
        self.error_states.clear();
        self.interact_mode = false;
        self.edit_mode = None;
        self.profile_manager = new_profile_manager;
        self.dispatcher = new_dispatcher;
        self.widget_configs = normalized.widgets.clone();
        self.theme_config = normalized.theme.clone();
        self.keybinds_config = normalized.keybinds.clone();

        // Recompute focus
        let occupied = self.layout.occupied_cells();
        if let Some(current) = self.focus {
            if let Some(anchor) = self.layout.anchor_for(current.0, current.1) {
                self.focus = Some(anchor);
            } else {
                self.focus = occupied.first().copied();
            }
        } else {
            self.focus = occupied.first().copied();
        }

        // 4. Restart sysinfo task
        self.sysinfo_restarting = true;
        self.sysinfo_cancel.cancel();

        if let Some(handle) = self.sysinfo_handle.take() {
            handle.abort();
        }

        let new_sysinfo_cancel = self.cancel.child_token();
        self.sysinfo_cancel = new_sysinfo_cancel.clone();

        let (new_data_tx, new_data_rx) = mpsc::channel::<DataUpdate>(32);
        self.sysinfo_handle = Some(spawn_sysinfo_task(new_data_tx, new_sysinfo_cancel));
        *data_rx = new_data_rx;
        self.sysinfo_restarting = false;
        self.sysinfo_disconnected = false;

        // Cancel and clear external tasks
        for handle in self.external_handles.drain(..) {
            handle.cancel.cancel();
            handle.handle.abort();
        }

        // Spawn fresh external tasks
        let (new_external_tx, new_external_rx) = mpsc::channel::<DataUpdate>(16);
        self.spawn_external_tasks(&new_external_tx);
        drop(new_external_tx);
        *external_rx = new_external_rx;
        self.external_disconnected = false;

        // Cancel and restart Hyprland task
        if let Some(handle) = self.hyprland_handle.take() {
            handle.cancel.cancel();
            handle.handle.abort();
        }

        let (new_hyprland_tx, new_hyprland_rx) = mpsc::channel::<DataUpdate>(16);
        let has_workspaces = self
            .components
            .values()
            .any(|c| c.widget_type() == "workspaces");
        if has_workspaces {
            let hyprland_cancel = self.cancel.child_token();
            let handle = spawn_hyprland_task(new_hyprland_tx.clone(), hyprland_cancel.clone());
            self.hyprland_handle = Some(ExternalTaskHandle {
                instance_id: "workspaces".to_string(),
                cancel: hyprland_cancel,
                handle,
            });
        }
        drop(new_hyprland_tx);
        *hyprland_rx = new_hyprland_rx;
        self.hyprland_disconnected = false;

        if tick_rate_changed {
            self.set_notification(
                "Config reloaded (tick_rate change requires restart)".to_string(),
            );
        } else {
            self.set_notification("Config reloaded".to_string());
        }
    }

    fn spawn_external_tasks(&mut self, external_tx: &mpsc::Sender<DataUpdate>) {
        for (id, component) in &self.components {
            let task_cancel = self.cancel.child_token();
            match component.widget_type() {
                "packages" => {
                    let widget = component
                        .as_any()
                        .downcast_ref::<crate::widgets::PackagesWidget>()
                        .expect("packages widget type mismatch");
                    let handle = spawn_packages_task(
                        id.clone(),
                        Duration::from_secs(widget.config.interval),
                        Duration::from_secs(widget.config.timeout),
                        external_tx.clone(),
                        task_cancel.clone(),
                    );
                    self.external_handles.push(ExternalTaskHandle {
                        instance_id: id.clone(),
                        cancel: task_cancel,
                        handle,
                    });
                }
                "services" => {
                    let widget = component
                        .as_any()
                        .downcast_ref::<crate::widgets::ServicesWidget>()
                        .expect("services widget type mismatch");
                    let handle = spawn_services_task(
                        id.clone(),
                        widget.config.scope.clone(),
                        widget.config.services.clone(),
                        Duration::from_secs(widget.config.interval),
                        Duration::from_secs(widget.config.timeout),
                        external_tx.clone(),
                        task_cancel.clone(),
                    );
                    self.external_handles.push(ExternalTaskHandle {
                        instance_id: id.clone(),
                        cancel: task_cancel,
                        handle,
                    });
                }
                _ => {}
            }
        }
    }

    fn focus_next(&mut self) {
        let cells = self.layout.occupied_cells();
        if cells.is_empty() {
            return;
        }

        self.focus = Some(match self.focus {
            Some(current) => {
                if let Some(pos) = cells.iter().position(|c| *c == current) {
                    cells[(pos + 1) % cells.len()]
                } else {
                    cells[0]
                }
            }
            None => cells[0],
        });
    }

    fn focus_prev(&mut self) {
        let cells = self.layout.occupied_cells();
        if cells.is_empty() {
            return;
        }

        self.focus = Some(match self.focus {
            Some(current) => {
                if let Some(pos) = cells.iter().position(|c| *c == current) {
                    if pos == 0 {
                        cells[cells.len() - 1]
                    } else {
                        cells[pos - 1]
                    }
                } else {
                    cells[0]
                }
            }
            None => cells[0],
        });
    }

    fn focus_direction(&mut self, direction: Direction) {
        let current = match self.focus {
            Some(pos) => pos,
            None => return,
        };

        let (rows, cols) = self.layout.grid_dimensions();

        let target = match direction {
            Direction::Up => (0..current.0)
                .rev()
                .find(|&r| self.layout.instance_at(r, current.1).is_some())
                .and_then(|r| self.layout.anchor_for(r, current.1)),
            Direction::Down => ((current.0 + 1)..rows)
                .find(|&r| self.layout.instance_at(r, current.1).is_some())
                .and_then(|r| self.layout.anchor_for(r, current.1)),
            Direction::Left => (0..current.1)
                .rev()
                .find(|&c| self.layout.instance_at(current.0, c).is_some())
                .and_then(|c| self.layout.anchor_for(current.0, c)),
            Direction::Right => ((current.1 + 1)..cols)
                .find(|&c| self.layout.instance_at(current.0, c).is_some())
                .and_then(|c| self.layout.anchor_for(current.0, c)),
        };

        if let Some(t) = target
            && t != current
        {
            self.focus = Some(t);
        }
    }

    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        // Expire stale notifications before rendering
        if let Some((_, created)) = &self.notification
            && created.elapsed() >= self.notification_duration
        {
            self.notification = None;
        }

        terminal.draw(|frame| {
            let area = frame.area();
            let (header_rect, _grid_rect, footer_rect) = self.layout.split_chrome(area);

            // Render header
            if let Some(header_area) = header_rect {
                let header_style = Style::new()
                    .fg(self.theme.header_fg)
                    .bg(self.theme.header_bg);
                let header_text = if self.edit_mode.is_some() {
                    format!(" mise-tui — {} [EDIT MODE] ", self.profile_manager.active_name())
                } else {
                    format!(" mise-tui — {} ", self.profile_manager.active_name())
                };
                let header = Paragraph::new(Line::from(header_text).style(header_style))
                    .style(header_style);
                frame.render_widget(header, header_area);
            }

            // Render grid cells
            self.undersized_panels.clear();
            let panel_rects = self.layout.resolve_rects(area);
            let all_rects = self.layout.resolve_all_rects(area);
            let (rows, cols) = self.layout.grid_dimensions();

            // 1. Draw empty borders for unoccupied cells
            for row in 0..rows {
                for col in 0..cols {
                    if self.layout.instance_at(row, col).is_none()
                        && let Some(&cell_rect) = all_rects.get(&(row, col))
                    {
                        let block = Block::bordered().border_style(self.theme.border);
                        frame.render_widget(block, cell_rect);
                    }
                }
            }

            // 2. Draw panels at anchor rects
            for (&(row, col), &cell_rect) in &panel_rects {
                let is_focused = self.focus == Some((row, col));
                let border_style = if let Some(ref editor) = self.edit_mode {
                    let (cr, cc) = editor.cursor();
                    if row == cr && col == cc {
                        self.theme.border_focused
                    } else {
                        self.theme.border_edit
                    }
                } else if is_focused && self.interact_mode {
                    self.theme.border_interact
                } else if is_focused {
                    self.theme.border_focused
                } else {
                    self.theme.border
                };

                let instance_id = self.layout.instance_at(row, col);

                let block = if let Some(id) = instance_id {
                    let name = self
                        .components
                        .get(id)
                        .map(|c| c.name().to_string())
                        .unwrap_or_default();
                    Block::bordered()
                        .title(format!(" {} ", name))
                        .title_style(self.theme.title)
                        .border_style(border_style)
                } else {
                    Block::bordered().border_style(border_style)
                };

                let inner = block.inner(cell_rect);
                frame.render_widget(block, cell_rect);

                // Render component content in the inner area
                if let Some(id) = instance_id
                    && let Some(component) = self.components.get_mut(id)
                {
                    if let Some(error_msg) = self.error_states.get(id) {
                        let error_style = Style::new().fg(self.theme.error_fg);
                        let error_text = Paragraph::new(Line::from(ratatui::text::Span::styled(
                            format!("Error: {error_msg}"),
                            error_style,
                        )));
                        frame.render_widget(error_text, inner);
                    } else {
                        let (min_w, min_h) = component.min_size();
                        if inner.width < min_w || inner.height < min_h {
                            self.undersized_panels.insert(id.to_string());
                            let placeholder =
                                Paragraph::new(Line::from(component.name().to_string()))
                                    .alignment(ratatui::layout::Alignment::Center);
                            frame.render_widget(placeholder, inner);
                        } else {
                            component.draw(frame, inner, &self.theme);
                        }
                    }
                }
            }

            // Render footer
            if let Some(footer_area) = footer_rect {
                let footer_style = Style::new()
                    .fg(self.theme.header_fg)
                    .bg(self.theme.header_bg);

                let text = if let Some((ref msg, _)) = self.notification {
                    format!(" {} ", msg)
                } else if let Some(ref editor) = self.edit_mode {
                    let (cr, cc) = editor.cursor();
                    let dirty = if editor.is_dirty() { " *" } else { "" };
                    let pending = match editor.pending_op() {
                        Some(crate::editor::PendingOp::Moving { .. }) => " | MOVING: arrows+Enter",
                        Some(crate::editor::PendingOp::Swapping { .. }) => " | SWAPPING: arrows+Enter",
                        Some(crate::editor::PendingOp::Adding { .. }) => " | ADDING: Up/Down+Enter",
                        Some(crate::editor::PendingOp::ConfirmingDelete { .. }) => " | DELETE? y/n",
                        Some(crate::editor::PendingOp::ConfirmingExit) => " | Unsaved! Esc again to discard",
                        None => "",
                    };
                    format!(
                        " [EDIT] ({},{}) | a:add d:del m:move x:swap s:save Esc:exit{}{} ",
                        cr, cc, dirty, pending
                    )
                } else if self.interact_mode {
                    " [INTERACT] j/k: scroll | s/S: sort | x: signal | Esc: exit ".to_string()
                } else {
                    " q: quit | Tab/Shift+Tab: cycle focus | Arrow keys: navigate | ?: help "
                        .to_string()
                };

                let footer =
                    Paragraph::new(Line::from(text).style(footer_style)).style(footer_style);
                frame.render_widget(footer, footer_area);
            }

            // Help overlay
            if self.show_help {
                let help_text = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        " Navigation",
                        Style::new()
                            .fg(self.theme.header_fg)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("   Tab / Shift+Tab    ", self.theme.value),
                        Span::styled("Cycle focus", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   Arrow keys         ", self.theme.value),
                        Span::styled("Directional focus", self.theme.label),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        " General",
                        Style::new()
                            .fg(self.theme.header_fg)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("   q / Ctrl+C         ", self.theme.value),
                        Span::styled("Quit", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   r                  ", self.theme.value),
                        Span::styled("Reload config", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   ?                  ", self.theme.value),
                        Span::styled("Toggle this help", self.theme.label),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        " Interact Mode",
                        Style::new()
                            .fg(self.theme.header_fg)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("   Enter              ", self.theme.value),
                        Span::styled("Enter interact mode", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   Escape             ", self.theme.value),
                        Span::styled("Exit interact mode", self.theme.label),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        " Profiles",
                        Style::new()
                            .fg(self.theme.header_fg)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("   Ctrl+]             ", self.theme.value),
                        Span::styled("Next profile", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   Ctrl+[             ", self.theme.value),
                        Span::styled("Previous profile", self.theme.label),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        " Edit Mode",
                        Style::new()
                            .fg(self.theme.header_fg)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("   e                  ", self.theme.value),
                        Span::styled("Enter edit mode", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   a / d / m / x      ", self.theme.value),
                        Span::styled("Add / Delete / Move / Swap", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   + / - / > / <      ", self.theme.value),
                        Span::styled("Resize col/row span", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   C / c / R / r      ", self.theme.value),
                        Span::styled("Add/remove columns/rows", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   s                  ", self.theme.value),
                        Span::styled("Save layout", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   u                  ", self.theme.value),
                        Span::styled("Undo last change", self.theme.label),
                    ]),
                    Line::from(vec![
                        Span::styled("   Escape             ", self.theme.value),
                        Span::styled("Exit edit mode", self.theme.label),
                    ]),
                ];

                let help_block = Block::bordered()
                    .title(" Keybinds ")
                    .title_style(self.theme.title)
                    .border_style(self.theme.border_focused)
                    .style(Style::new().bg(ratatui::style::Color::Black));

                let help_paragraph = Paragraph::new(help_text).block(help_block);
                frame.render_widget(Clear, area);
                frame.render_widget(help_paragraph, area);
            }
        })?;

        // Post-draw: exit interact mode if focused widget became undersized
        if self.interact_mode
            && let Some(focus) = self.focus
            && let Some(id) = self.layout.instance_at(focus.0, focus.1)
            && self.undersized_panels.contains(id)
        {
            self.interact_mode = false;
            if let Some(component) = self.components.get_mut(id) {
                component.notify_interact(false);
            }
        }

        Ok(())
    }
}

/// Spawn a file watcher task. Returns `(handle, ready_rx)` where `ready_rx`
/// resolves to `true` if the watcher started successfully, `false` otherwise.
fn spawn_config_watcher(
    config_path: PathBuf,
    config_tx: mpsc::Sender<()>,
    cancel: CancellationToken,
) -> Option<(
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Receiver<bool>,
)> {
    let watch_dir = config_path.parent()?.to_path_buf();
    let file_name = config_path.file_name()?.to_os_string();

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(async move {
        use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};

        let (notify_tx, mut notify_rx) = mpsc::channel(4);

        let mut debouncer = match new_debouncer(
            Duration::from_millis(500),
            move |events: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                if let Ok(events) = events {
                    for event in events {
                        if event.kind == DebouncedEventKind::Any
                            && event.path.file_name() == Some(&file_name)
                        {
                            let _ = notify_tx.blocking_send(());
                            return;
                        }
                    }
                }
            },
        ) {
            Ok(d) => d,
            Err(_) => {
                let _ = ready_tx.send(false);
                return;
            }
        };

        if debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
            .is_err()
        {
            let _ = ready_tx.send(false);
            return;
        }

        let _ = ready_tx.send(true);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                Some(()) = notify_rx.recv() => {
                    let _ = config_tx.try_send(());
                }
            }
        }
    });

    Some((handle, ready_rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_config_path_stored() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let app = App::new(config, path.clone()).unwrap();
        assert_eq!(app.config_path, path);
    }

    #[test]
    fn handle_action_enter_interact() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let mut app = App::new(config, path).unwrap();
        assert!(!app.interact_mode);
        app.handle_action(crate::action::Action::EnterInteract).unwrap();
        assert!(app.interact_mode);
    }

    #[test]
    fn handle_action_exit_interact() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let mut app = App::new(config, path).unwrap();
        app.interact_mode = true;
        app.handle_action(crate::action::Action::ExitInteract).unwrap();
        assert!(!app.interact_mode);
    }

    #[test]
    fn profile_name_shown_in_app() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let app = App::new(config, path).unwrap();
        assert_eq!(app.profile_manager.active_name(), "default");
    }

    #[test]
    fn handle_action_toggle_help() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let mut app = App::new(config, path).unwrap();
        assert!(!app.show_help);
        app.handle_action(crate::action::Action::ToggleHelp).unwrap();
        assert!(app.show_help);
        app.handle_action(crate::action::Action::ToggleHelp).unwrap();
        assert!(!app.show_help);
    }

    #[test]
    fn handle_action_enter_edit_mode() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let mut app = App::new(config, path).unwrap();
        assert!(app.edit_mode.is_none());
        app.handle_action(crate::action::Action::EnterEditMode).unwrap();
        assert!(app.edit_mode.is_some());
    }

    #[test]
    fn handle_action_enter_edit_mode_blocked_in_interact() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let mut app = App::new(config, path).unwrap();
        app.interact_mode = true;
        app.handle_action(crate::action::Action::EnterEditMode).unwrap();
        assert!(app.edit_mode.is_none());
    }

    #[test]
    fn handle_action_switch_profile_blocked_in_edit() {
        let config = crate::config::Config::parse(include_str!("../config/default.toml")).unwrap();
        let path = PathBuf::from("config/default.toml");
        let mut app = App::new(config, path).unwrap();
        app.edit_mode = Some(crate::editor::EditMode::enter(
            app.profile_manager.active_layout().clone(),
        ));
        // Switch profile should be blocked
        app.handle_action(crate::action::Action::SwitchProfile(
            crate::action::ProfileDirection::Next,
        )).unwrap();
        // Profile should not have changed (only one profile in default config)
        assert_eq!(app.profile_manager.active_name(), "default");
    }
}
