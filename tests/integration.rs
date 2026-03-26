/// Smoke test: parse embedded default config, verify structure and validity.
#[test]
fn default_config_end_to_end() {
    let default_toml = include_str!("../config/default.toml");

    let value: toml::Value =
        toml::from_str(default_toml).expect("default.toml should be valid TOML");

    let table = value.as_table().expect("root should be a table");

    // Required sections
    assert!(table.contains_key("general"), "missing [general]");
    assert!(table.contains_key("layout"), "missing [layout]");
    assert!(table.contains_key("theme"), "missing [theme]");

    // Layout structure
    let layout = table["layout"].as_table().unwrap();
    assert!(layout.contains_key("columns"));
    assert!(layout.contains_key("rows"));
    assert!(layout.contains_key("panels"));

    let panels = layout["panels"].as_array().unwrap();
    assert_eq!(panels.len(), 6, "default config should have 6 panels");

    // Each panel has required fields and known type
    let known_types = [
        "cpu",
        "memory",
        "network",
        "temps",
        "disk",
        "processes",
        "packages",
        "services",
    ];
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
