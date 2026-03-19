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
    assert_eq!(panels.len(), 4, "default config should have 4 panels");

    // Each panel has required fields and known type
    let known_types = ["cpu", "memory", "network", "temps"];
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
