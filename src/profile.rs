use color_eyre::Result;
use color_eyre::eyre::eyre;
use indexmap::IndexMap;

use crate::config::ProfileConfig;

/// Manages layout profiles: tracks which profile is active and supports
/// cycling / switching between them.
pub struct ProfileManager {
    profiles: IndexMap<String, ProfileConfig>,
    active: String,
}

impl ProfileManager {
    /// Create a new ProfileManager with the given profiles and default.
    ///
    /// Returns an error if `default` is not a key in `profiles`.
    pub fn new(profiles: IndexMap<String, ProfileConfig>, default: &str) -> Result<Self> {
        if !profiles.contains_key(default) {
            return Err(eyre!("Default profile '{}' not found", default));
        }
        Ok(Self {
            profiles,
            active: default.to_string(),
        })
    }

    /// Name of the currently active profile.
    pub fn active_name(&self) -> &str {
        &self.active
    }

    /// Layout config for the currently active profile.
    pub fn active_layout(&self) -> &ProfileConfig {
        &self.profiles[&self.active]
    }

    /// List all profile names in insertion order.
    pub fn list(&self) -> Vec<&str> {
        self.profiles.keys().map(|s| s.as_str()).collect()
    }

    /// Switch to the named profile. Returns the new active layout config,
    /// or an error if the profile does not exist.
    pub fn switch(&mut self, name: &str) -> Result<&ProfileConfig> {
        if !self.profiles.contains_key(name) {
            return Err(eyre!("Profile '{}' not found", name));
        }
        self.active = name.to_string();
        Ok(&self.profiles[&self.active])
    }

    /// Cycle to the next profile in insertion order, wrapping around.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> &ProfileConfig {
        let keys: Vec<&String> = self.profiles.keys().collect();
        let current_idx = keys
            .iter()
            .position(|k| *k == &self.active)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % keys.len();
        self.active = keys[next_idx].clone();
        &self.profiles[&self.active]
    }

    /// Cycle to the previous profile in insertion order, wrapping around.
    pub fn prev(&mut self) -> &ProfileConfig {
        let keys: Vec<&String> = self.profiles.keys().collect();
        let current_idx = keys
            .iter()
            .position(|k| *k == &self.active)
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            keys.len() - 1
        } else {
            current_idx - 1
        };
        self.active = keys[prev_idx].clone();
        &self.profiles[&self.active]
    }

    /// Insert or replace a profile layout.
    pub fn update_profile(&mut self, name: &str, layout: ProfileConfig) {
        self.profiles.insert(name.to_string(), layout);
    }

    /// Number of profiles.
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// Reference to the underlying profiles map.
    pub fn profiles_map(&self) -> &IndexMap<String, ProfileConfig> {
        &self.profiles
    }

    /// Serialize the config and write it to disk, creating a `.bak` backup first.
    pub fn save_to_file(
        &self,
        config: &crate::config::NormalizedConfig,
        path: &std::path::Path,
    ) -> Result<()> {
        let saveable = config.to_saveable();
        let toml_string = toml::to_string_pretty(&saveable)
            .map_err(|e| eyre!("Failed to serialize config: {}", e))?;
        // Create backup
        if path.exists() {
            let backup = path.with_extension("toml.bak");
            std::fs::copy(path, &backup)
                .map_err(|e| eyre!("Failed to create backup: {}", e))?;
        }
        std::fs::write(path, toml_string)
            .map_err(|e| eyre!("Failed to write config: {}", e))?;
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PanelConfig;

    fn make_profile(columns: usize, rows: usize) -> ProfileConfig {
        ProfileConfig {
            columns,
            rows,
            column_widths: None,
            row_heights: None,
            header: true,
            footer: true,
            panels: vec![PanelConfig {
                row: 0,
                col: 0,
                widget_type: "cpu".to_string(),
                id: None,
                col_span: 1,
                row_span: 1,
            }],
        }
    }

    fn make_profiles() -> IndexMap<String, ProfileConfig> {
        let mut map = IndexMap::new();
        map.insert("compact".to_string(), make_profile(2, 1));
        map.insert("full".to_string(), make_profile(3, 2));
        map.insert("minimal".to_string(), make_profile(1, 1));
        map
    }

    #[test]
    fn test_new_with_valid_default() {
        let profiles = make_profiles();
        let mgr = ProfileManager::new(profiles, "compact").unwrap();
        assert_eq!(mgr.active_name(), "compact");
        assert_eq!(mgr.active_layout().columns, 2);
    }

    #[test]
    fn test_new_with_invalid_default() {
        let profiles = make_profiles();
        assert!(ProfileManager::new(profiles, "nonexistent").is_err());
    }

    #[test]
    fn test_list_returns_all_profiles() {
        let profiles = make_profiles();
        let mgr = ProfileManager::new(profiles, "compact").unwrap();
        let list = mgr.list();
        assert_eq!(list.len(), 3);
        assert_eq!(list, vec!["compact", "full", "minimal"]);
    }

    #[test]
    fn test_switch_to_valid_profile() {
        let profiles = make_profiles();
        let mut mgr = ProfileManager::new(profiles, "compact").unwrap();
        let layout = mgr.switch("full").unwrap();
        assert_eq!(layout.columns, 3);
        assert_eq!(mgr.active_name(), "full");
    }

    #[test]
    fn test_switch_to_invalid_profile() {
        let profiles = make_profiles();
        let mut mgr = ProfileManager::new(profiles, "compact").unwrap();
        assert!(mgr.switch("nonexistent").is_err());
        // Active should remain unchanged
        assert_eq!(mgr.active_name(), "compact");
    }

    #[test]
    fn test_next_cycles_forward() {
        let profiles = make_profiles();
        let mut mgr = ProfileManager::new(profiles, "compact").unwrap();

        let layout = mgr.next();
        assert_eq!(layout.columns, 3); // full
        assert_eq!(mgr.active_name(), "full");

        let layout = mgr.next();
        assert_eq!(layout.columns, 1); // minimal
        assert_eq!(mgr.active_name(), "minimal");

        let layout = mgr.next();
        assert_eq!(layout.columns, 2); // wraps to compact
        assert_eq!(mgr.active_name(), "compact");
    }

    #[test]
    fn test_prev_cycles_backward() {
        let profiles = make_profiles();
        let mut mgr = ProfileManager::new(profiles, "compact").unwrap();

        let layout = mgr.prev();
        assert_eq!(layout.columns, 1); // wraps to minimal
        assert_eq!(mgr.active_name(), "minimal");

        let layout = mgr.prev();
        assert_eq!(layout.columns, 3); // full
        assert_eq!(mgr.active_name(), "full");

        let layout = mgr.prev();
        assert_eq!(layout.columns, 2); // back to compact
        assert_eq!(mgr.active_name(), "compact");
    }

    #[test]
    fn test_update_profile() {
        let profiles = make_profiles();
        let mut mgr = ProfileManager::new(profiles, "compact").unwrap();

        let new_layout = make_profile(4, 3);
        mgr.update_profile("compact", new_layout);

        assert_eq!(mgr.active_layout().columns, 4);
        assert_eq!(mgr.active_layout().rows, 3);
    }

    #[test]
    fn test_single_profile_next_stays() {
        let mut map = IndexMap::new();
        map.insert("only".to_string(), make_profile(2, 2));
        let mut mgr = ProfileManager::new(map, "only").unwrap();

        mgr.next();
        assert_eq!(mgr.active_name(), "only");
        assert_eq!(mgr.active_layout().columns, 2);

        mgr.prev();
        assert_eq!(mgr.active_name(), "only");
        assert_eq!(mgr.active_layout().columns, 2);
    }

    #[test]
    fn test_profile_count() {
        let profiles = make_profiles();
        let mgr = ProfileManager::new(profiles, "compact").unwrap();
        assert_eq!(mgr.profile_count(), 3);
    }

    #[test]
    fn test_profiles_map() {
        let profiles = make_profiles();
        let mgr = ProfileManager::new(profiles, "compact").unwrap();
        let map = mgr.profiles_map();
        assert!(map.contains_key("compact"));
        assert!(map.contains_key("full"));
        assert!(map.contains_key("minimal"));
    }
}
