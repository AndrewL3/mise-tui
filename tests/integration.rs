/// Smoke test: parse embedded default config, verify structure and validity.
#[test]
fn default_config_end_to_end() {
    let default_toml = include_str!("../config/default.toml");

    let value: toml::Value =
        toml::from_str(default_toml).expect("default.toml should be valid TOML");

    let table = value.as_table().expect("root should be a table");

    // Required sections
    assert!(table.contains_key("general"), "missing [general]");
    assert!(table.contains_key("theme"), "missing [theme]");

    // Must have profiles (new format) or layout (legacy)
    let has_profiles = table.contains_key("profiles");
    let has_layout = table.contains_key("layout");
    assert!(
        has_profiles || has_layout,
        "missing both [profiles] and [layout]"
    );

    // Known widget types
    let known_types = [
        "cpu",
        "memory",
        "network",
        "temps",
        "disk",
        "processes",
        "packages",
        "services",
        "workspaces",
    ];

    // Validate profiles if present
    if has_profiles {
        let profiles = table["profiles"].as_table().unwrap();
        let mut total_panels = 0;
        for (_name, profile) in profiles {
            let profile = profile.as_table().unwrap();
            assert!(profile.contains_key("columns"));
            assert!(profile.contains_key("rows"));
            assert!(profile.contains_key("panels"));

            let panels = profile["panels"].as_array().unwrap();
            total_panels += panels.len();
            for panel in panels {
                let panel = panel.as_table().unwrap();
                assert!(panel.contains_key("row"));
                assert!(panel.contains_key("col"));
                assert!(panel.contains_key("type"));
                let wtype = panel["type"].as_str().unwrap();
                assert!(
                    known_types.contains(&wtype),
                    "unknown widget type: {}",
                    wtype
                );
            }
        }
        assert_eq!(total_panels, 6, "default config should have 6 panels");
    }

    // Validate layout if present (legacy format)
    if has_layout {
        let layout = table["layout"].as_table().unwrap();
        assert!(layout.contains_key("columns"));
        assert!(layout.contains_key("rows"));
        assert!(layout.contains_key("panels"));

        let panels = layout["panels"].as_array().unwrap();
        for panel in panels {
            let panel = panel.as_table().unwrap();
            assert!(panel.contains_key("row"));
            assert!(panel.contains_key("col"));
            assert!(panel.contains_key("type"));
            let wtype = panel["type"].as_str().unwrap();
            assert!(
                known_types.contains(&wtype),
                "unknown widget type: {}",
                wtype
            );
        }
    }

    // Widget sections match panel types
    if let Some(widgets) = table.get("widgets") {
        let widgets = widgets.as_table().unwrap();
        for key in widgets.keys() {
            assert!(
                known_types.contains(&key.as_str()),
                "orphan widget section: {}",
                key
            );
        }
    }

    // Tick rate is reasonable
    let tick_rate = table["general"]["tick_rate"].as_integer().unwrap();
    assert!(
        tick_rate > 0 && tick_rate < 10000,
        "tick_rate should be reasonable"
    );
}

#[test]
fn packages_widget_component_compliance() {
    use mise_tui::component::Component;
    let w = mise_tui::widgets::PackagesWidget::new("packages".into(), None).unwrap();
    assert_eq!(w.id(), "packages");
    assert_eq!(w.widget_type(), "packages");
    assert!(!w.supports_interact());
}

#[test]
fn services_widget_component_compliance() {
    use mise_tui::component::Component;
    let config: toml::Value = toml::from_str(r#"services = ["sshd"]"#).unwrap();
    let w = mise_tui::widgets::ServicesWidget::new("services".into(), Some(config)).unwrap();
    assert_eq!(w.id(), "services");
    assert_eq!(w.widget_type(), "services");
    assert!(!w.supports_interact());
}

#[test]
fn config_with_external_widgets_validates() {
    let toml_str = r#"
[general]
tick_rate = 250

[layout]
columns = 2
rows = 1

[[layout.panels]]
row = 0
col = 0
type = "packages"

[[layout.panels]]
row = 0
col = 1
type = "services"

[theme]

[widgets.services]
services = ["sshd", "NetworkManager"]
"#;
    let config: mise_tui::config::Config = toml::from_str(toml_str).expect("should parse");
    let validation = config.validate(mise_tui::registry::is_known_type);
    assert!(
        !validation.has_errors(),
        "validation errors: {:?}",
        validation.errors
    );
}

// ─── M7 Integration Tests ───────────────────────────────────────────────────

#[test]
fn test_profiles_config_loads_and_validates() {
    let toml_str = r#"
[general]
tick_rate = 250
default_profile = "compact"

[profiles.compact]
columns = 2
rows = 1
[[profiles.compact.panels]]
row = 0
col = 0
type = "cpu"
[[profiles.compact.panels]]
row = 0
col = 1
type = "memory"

[profiles.full]
columns = 3
rows = 2
[[profiles.full.panels]]
row = 0
col = 0
type = "cpu"
[[profiles.full.panels]]
row = 0
col = 1
type = "memory"
[[profiles.full.panels]]
row = 0
col = 2
type = "network"
[[profiles.full.panels]]
row = 1
col = 0
type = "temps"
[[profiles.full.panels]]
row = 1
col = 1
type = "disk"
[[profiles.full.panels]]
row = 1
col = 2
type = "processes"

[theme]
"#;
    let config: mise_tui::config::Config = toml::from_str(toml_str).expect("should parse");
    let normalized = config.normalize().expect("should normalize");
    assert_eq!(normalized.default_profile(), "compact");
    assert_eq!(normalized.profiles().len(), 2);

    let validation = normalized.validate(mise_tui::registry::is_known_type);
    assert!(
        !validation.has_errors(),
        "validation errors: {:?}",
        validation.errors
    );
}

#[test]
fn test_legacy_config_still_works() {
    let toml_str = r#"
[general]
tick_rate = 250

[layout]
columns = 2
rows = 1

[[layout.panels]]
row = 0
col = 0
type = "cpu"

[[layout.panels]]
row = 0
col = 1
type = "memory"

[theme]
"#;
    let config: mise_tui::config::Config = toml::from_str(toml_str).expect("should parse");
    let normalized = config.normalize().expect("should normalize");
    assert_eq!(normalized.default_profile(), "default");
    assert_eq!(normalized.profiles().len(), 1);
    assert_eq!(normalized.active_profile_config().columns, 2);
    assert_eq!(normalized.active_profile_config().panels.len(), 2);

    let validation = normalized.validate(mise_tui::registry::is_known_type);
    assert!(
        !validation.has_errors(),
        "validation errors: {:?}",
        validation.errors
    );
}

#[test]
fn test_custom_keybinds_parse() {
    let toml_str = r#"
[general]
tick_rate = 250

[keybinds]
quit = "x"
reload = "alt+r"

[layout]
columns = 1
rows = 1
[[layout.panels]]
row = 0
col = 0
type = "cpu"

[theme]
"#;
    let config: mise_tui::config::Config = toml::from_str(toml_str).expect("should parse");
    let normalized = config.normalize().expect("should normalize");
    assert_eq!(normalized.keybinds.quit.as_deref(), Some("x"));
    assert_eq!(normalized.keybinds.reload.as_deref(), Some("alt+r"));

    // Verify dispatcher can be built with custom binds
    let dispatcher = mise_tui::input::KeyDispatcher::new(&normalized.keybinds);
    assert!(dispatcher.is_ok(), "dispatcher should build: {:?}", dispatcher.err());
}

#[test]
fn test_editor_full_flow() {
    use mise_tui::config::PanelConfig;
    use mise_tui::editor::{EditAction, EditMode};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let layout = mise_tui::config::ProfileConfig {
        columns: 3,
        rows: 2,
        column_widths: None,
        row_heights: None,
        header: true,
        footer: true,
        panels: vec![
            PanelConfig {
                row: 0,
                col: 0,
                widget_type: "cpu".into(),
                id: None,
                col_span: 1,
                row_span: 1,
            },
            PanelConfig {
                row: 0,
                col: 1,
                widget_type: "memory".into(),
                id: None,
                col_span: 1,
                row_span: 1,
            },
        ],
    };

    let mut editor = EditMode::enter(layout);
    assert!(!editor.is_dirty());
    assert_eq!(editor.cursor(), (0, 0));

    // Move to empty cell (0, 2) and add a widget
    editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(editor.cursor(), (0, 2));

    // Start add
    editor.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

    // Confirm add (first type in sorted list)
    let action = editor.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(action, EditAction::LayoutChanged { .. }));
    assert!(editor.is_dirty());
    assert_eq!(editor.working_layout().panels.len(), 3);

    // Save
    let action = editor.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(action, EditAction::Save);
    assert!(!editor.is_dirty());

    // Exit cleanly (not dirty)
    let action = editor.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(action, EditAction::Exit);
}

#[test]
fn test_config_round_trip_save_and_reload() {
    let toml_str = r#"
[general]
tick_rate = 250
default_profile = "default"

[profiles.default]
columns = 2
rows = 1
[[profiles.default.panels]]
row = 0
col = 0
type = "cpu"
[[profiles.default.panels]]
row = 0
col = 1
type = "memory"

[theme]

[widgets.cpu]
mode = "sparklines"
"#;

    let config: mise_tui::config::Config = toml::from_str(toml_str).expect("should parse");
    let normalized = config.normalize().expect("should normalize");

    // Create a ProfileManager and save to a temp file
    let pm = mise_tui::profile::ProfileManager::new(
        normalized.profiles.clone(),
        &normalized.default_profile,
    )
    .expect("should create profile manager");

    let dir = tempfile::tempdir().expect("should create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml_str).expect("should write initial config");

    pm.save_to_file(&normalized, &path).expect("should save");

    // Reload and verify
    let reloaded = mise_tui::config::Config::load(Some(&path)).expect("should reload");
    let renormalized = reloaded.normalize().expect("should normalize reloaded");

    assert_eq!(renormalized.default_profile(), "default");
    assert_eq!(renormalized.profiles().len(), 1);
    let profile = renormalized.active_profile_config();
    assert_eq!(profile.columns, 2);
    assert_eq!(profile.rows, 1);
    assert_eq!(profile.panels.len(), 2);

    // Check widget config preserved
    assert!(renormalized.widgets.contains_key("cpu"));

    // Check backup was created
    let backup = path.with_extension("toml.bak");
    assert!(backup.exists(), "backup file should exist");
}
