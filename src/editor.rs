use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{PanelConfig, ProfileConfig};
use crate::registry;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum EditAction {
    LayoutChanged {
        layout: ProfileConfig,
        added_widget: Option<(String, String)>, // (instance_id, widget_type)
        removed_widget: Option<String>,          // instance_id
    },
    Save,
    Exit,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingOp {
    Moving { from: (usize, usize) },
    Swapping { from: (usize, usize) },
    Adding { selected_index: usize, types: Vec<String> },
    ConfirmingDelete { widget_type: String },
    ConfirmingExit,
}

#[derive(Debug, Clone)]
struct UndoSnapshot {
    layout: ProfileConfig,
    #[allow(dead_code)]
    added_widget_config: Option<String>,
}

// ─── EditMode ────────────────────────────────────────────────────────────────

pub struct EditMode {
    working_layout: ProfileConfig,
    cursor: (usize, usize),
    pending_op: Option<PendingOp>,
    undo_state: Option<UndoSnapshot>,
    dirty: bool,
}

impl EditMode {
    pub fn enter(layout: ProfileConfig) -> Self {
        Self {
            working_layout: layout,
            cursor: (0, 0),
            pending_op: None,
            undo_state: None,
            dirty: false,
        }
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn pending_op(&self) -> Option<&PendingOp> {
        self.pending_op.as_ref()
    }

    pub fn working_layout(&self) -> &ProfileConfig {
        &self.working_layout
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EditAction {
        if self.pending_op.is_some() {
            return self.handle_pending_key(key);
        }

        match key.code {
            // Cursor movement
            KeyCode::Up => {
                if self.cursor.0 > 0 {
                    self.cursor.0 -= 1;
                }
                EditAction::None
            }
            KeyCode::Down => {
                if self.cursor.0 + 1 < self.working_layout.rows {
                    self.cursor.0 += 1;
                }
                EditAction::None
            }
            KeyCode::Left => {
                if self.cursor.1 > 0 {
                    self.cursor.1 -= 1;
                }
                EditAction::None
            }
            KeyCode::Right => {
                if self.cursor.1 + 1 < self.working_layout.columns {
                    self.cursor.1 += 1;
                }
                EditAction::None
            }

            // Escape: exit (with dirty check)
            KeyCode::Esc => {
                if self.dirty {
                    self.pending_op = Some(PendingOp::ConfirmingExit);
                    EditAction::None
                } else {
                    EditAction::Exit
                }
            }

            // Save
            KeyCode::Char('s') => {
                if self.dirty {
                    self.dirty = false;
                    EditAction::Save
                } else {
                    EditAction::None
                }
            }

            // Add widget
            KeyCode::Char('a') => {
                self.start_add()
            }

            // Delete widget
            KeyCode::Char('d') => {
                self.start_delete()
            }

            // Move widget
            KeyCode::Char('m') => {
                self.start_move()
            }

            // Swap widgets
            KeyCode::Char('x') => {
                self.start_swap()
            }

            // Resize col_span / row_span
            KeyCode::Char('+') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    // On most terminals, '+' already has SHIFT. We use a heuristic:
                    // if SHIFT is the ONLY modifier, treat as col_span grow.
                    // For explicit row_span grow, we use '>' instead.
                    self.resize_col_span(1)
                } else {
                    self.resize_col_span(1)
                }
            }
            KeyCode::Char('-') => {
                self.resize_col_span(-1)
            }
            KeyCode::Char('>') => {
                self.resize_row_span(1)
            }
            KeyCode::Char('<') => {
                self.resize_row_span(-1)
            }

            // Grid dimensions: uppercase = add, lowercase = remove
            KeyCode::Char('C') => {
                self.add_column()
            }
            KeyCode::Char('c') => {
                self.remove_last_column()
            }
            KeyCode::Char('R') => {
                self.add_row()
            }
            KeyCode::Char('r') => {
                self.remove_last_row()
            }

            // Undo
            KeyCode::Char('u') => {
                self.undo()
            }

            _ => EditAction::None,
        }
    }

    // ─── Pending operation dispatch ──────────────────────────────────────────

    fn handle_pending_key(&mut self, key: KeyEvent) -> EditAction {
        let op = self.pending_op.clone().unwrap();
        match op {
            PendingOp::Adding { selected_index, ref types } => {
                self.handle_adding_key(key, selected_index, types.clone())
            }
            PendingOp::ConfirmingDelete { ref widget_type } => {
                self.handle_confirming_delete_key(key, widget_type.clone())
            }
            PendingOp::Moving { from } => {
                self.handle_moving_key(key, from)
            }
            PendingOp::Swapping { from } => {
                self.handle_swapping_key(key, from)
            }
            PendingOp::ConfirmingExit => {
                self.handle_confirming_exit_key(key)
            }
        }
    }

    fn handle_adding_key(
        &mut self,
        key: KeyEvent,
        selected_index: usize,
        types: Vec<String>,
    ) -> EditAction {
        match key.code {
            KeyCode::Up => {
                let new_index = if selected_index > 0 {
                    selected_index - 1
                } else {
                    selected_index
                };
                self.pending_op = Some(PendingOp::Adding {
                    selected_index: new_index,
                    types,
                });
                EditAction::None
            }
            KeyCode::Down => {
                let new_index = if selected_index + 1 < types.len() {
                    selected_index + 1
                } else {
                    selected_index
                };
                self.pending_op = Some(PendingOp::Adding {
                    selected_index: new_index,
                    types,
                });
                EditAction::None
            }
            KeyCode::Enter => {
                let widget_type = types[selected_index].clone();
                let instance_id = self.generate_instance_id(&widget_type);
                self.save_undo(None);
                self.working_layout.panels.push(PanelConfig {
                    row: self.cursor.0,
                    col: self.cursor.1,
                    widget_type: widget_type.clone(),
                    id: Some(instance_id.clone()),
                    col_span: 1,
                    row_span: 1,
                });
                self.dirty = true;
                self.pending_op = None;
                EditAction::LayoutChanged {
                    layout: self.working_layout.clone(),
                    added_widget: Some((instance_id, widget_type)),
                    removed_widget: None,
                }
            }
            KeyCode::Esc => {
                self.pending_op = None;
                EditAction::None
            }
            _ => EditAction::None,
        }
    }

    fn handle_confirming_delete_key(
        &mut self,
        key: KeyEvent,
        widget_type: String,
    ) -> EditAction {
        match key.code {
            KeyCode::Char('y') => {
                let (row, col) = self.cursor;
                if let Some(idx) = self.panel_index_at(row, col) {
                    let panel = &self.working_layout.panels[idx];
                    let instance_id = panel.id.clone().unwrap_or_else(|| panel.widget_type.clone());
                    self.save_undo(None);
                    self.working_layout.panels.remove(idx);
                    self.dirty = true;
                    self.pending_op = None;
                    let _ = widget_type; // consumed
                    EditAction::LayoutChanged {
                        layout: self.working_layout.clone(),
                        added_widget: None,
                        removed_widget: Some(instance_id),
                    }
                } else {
                    self.pending_op = None;
                    EditAction::None
                }
            }
            _ => {
                self.pending_op = None;
                EditAction::None
            }
        }
    }

    fn handle_moving_key(&mut self, key: KeyEvent, from: (usize, usize)) -> EditAction {
        match key.code {
            KeyCode::Up => {
                if self.cursor.0 > 0 {
                    self.cursor.0 -= 1;
                }
                EditAction::None
            }
            KeyCode::Down => {
                if self.cursor.0 + 1 < self.working_layout.rows {
                    self.cursor.0 += 1;
                }
                EditAction::None
            }
            KeyCode::Left => {
                if self.cursor.1 > 0 {
                    self.cursor.1 -= 1;
                }
                EditAction::None
            }
            KeyCode::Right => {
                if self.cursor.1 + 1 < self.working_layout.columns {
                    self.cursor.1 += 1;
                }
                EditAction::None
            }
            KeyCode::Enter => {
                let target = self.cursor;
                if target == from {
                    // No-op: same cell
                    self.pending_op = None;
                    return EditAction::None;
                }
                if !self.is_cell_empty(target.0, target.1) {
                    // Target must be empty for move
                    return EditAction::None;
                }
                if let Some(idx) = self.panel_index_at(from.0, from.1) {
                    self.save_undo(None);
                    self.working_layout.panels[idx].row = target.0;
                    self.working_layout.panels[idx].col = target.1;
                    self.dirty = true;
                    self.pending_op = None;
                    EditAction::LayoutChanged {
                        layout: self.working_layout.clone(),
                        added_widget: None,
                        removed_widget: None,
                    }
                } else {
                    self.pending_op = None;
                    EditAction::None
                }
            }
            KeyCode::Esc => {
                self.pending_op = None;
                EditAction::None
            }
            _ => EditAction::None,
        }
    }

    fn handle_swapping_key(&mut self, key: KeyEvent, from: (usize, usize)) -> EditAction {
        match key.code {
            KeyCode::Up => {
                if self.cursor.0 > 0 {
                    self.cursor.0 -= 1;
                }
                EditAction::None
            }
            KeyCode::Down => {
                if self.cursor.0 + 1 < self.working_layout.rows {
                    self.cursor.0 += 1;
                }
                EditAction::None
            }
            KeyCode::Left => {
                if self.cursor.1 > 0 {
                    self.cursor.1 -= 1;
                }
                EditAction::None
            }
            KeyCode::Right => {
                if self.cursor.1 + 1 < self.working_layout.columns {
                    self.cursor.1 += 1;
                }
                EditAction::None
            }
            KeyCode::Enter => {
                let target = self.cursor;
                if target == from {
                    self.pending_op = None;
                    return EditAction::None;
                }
                // Target must be occupied for swap
                let from_idx = self.panel_index_at(from.0, from.1);
                let target_idx = self.panel_index_at(target.0, target.1);
                if let (Some(fi), Some(ti)) = (from_idx, target_idx) {
                    self.save_undo(None);
                    // Swap row/col of both panels
                    let from_row = self.working_layout.panels[fi].row;
                    let from_col = self.working_layout.panels[fi].col;
                    let target_row = self.working_layout.panels[ti].row;
                    let target_col = self.working_layout.panels[ti].col;
                    self.working_layout.panels[fi].row = target_row;
                    self.working_layout.panels[fi].col = target_col;
                    self.working_layout.panels[ti].row = from_row;
                    self.working_layout.panels[ti].col = from_col;
                    self.dirty = true;
                    self.pending_op = None;
                    EditAction::LayoutChanged {
                        layout: self.working_layout.clone(),
                        added_widget: None,
                        removed_widget: None,
                    }
                } else {
                    // Target not occupied, can't swap
                    EditAction::None
                }
            }
            KeyCode::Esc => {
                self.pending_op = None;
                EditAction::None
            }
            _ => EditAction::None,
        }
    }

    fn handle_confirming_exit_key(&mut self, key: KeyEvent) -> EditAction {
        match key.code {
            KeyCode::Esc => {
                // Second Escape while confirming exit → actually exit
                self.pending_op = None;
                EditAction::Exit
            }
            _ => {
                // Any other key cancels the exit confirmation
                self.pending_op = None;
                EditAction::None
            }
        }
    }

    // ─── Operations ──────────────────────────────────────────────────────────

    fn start_add(&mut self) -> EditAction {
        let (row, col) = self.cursor;
        if !self.is_cell_empty(row, col) {
            return EditAction::None;
        }
        let types: Vec<String> = registry::known_types()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        self.pending_op = Some(PendingOp::Adding {
            selected_index: 0,
            types,
        });
        EditAction::None
    }

    fn start_delete(&mut self) -> EditAction {
        let (row, col) = self.cursor;
        if let Some(panel) = self.panel_at(row, col) {
            let widget_type = panel.widget_type.clone();
            self.pending_op = Some(PendingOp::ConfirmingDelete { widget_type });
        }
        EditAction::None
    }

    fn start_move(&mut self) -> EditAction {
        let (row, col) = self.cursor;
        if self.panel_at(row, col).is_some() {
            self.pending_op = Some(PendingOp::Moving { from: (row, col) });
        }
        EditAction::None
    }

    fn start_swap(&mut self) -> EditAction {
        let (row, col) = self.cursor;
        if self.panel_at(row, col).is_some() {
            self.pending_op = Some(PendingOp::Swapping { from: (row, col) });
        }
        EditAction::None
    }

    fn resize_col_span(&mut self, delta: i32) -> EditAction {
        let (row, col) = self.cursor;
        if let Some(idx) = self.panel_index_at(row, col) {
            let panel = &self.working_layout.panels[idx];
            let new_span = panel.col_span as i32 + delta;
            if new_span < 1 {
                return EditAction::None;
            }
            let new_span = new_span as usize;

            // Check bounds
            if panel.col + new_span > self.working_layout.columns {
                return EditAction::None;
            }

            // If growing, check that target cells are empty (excluding this panel)
            if delta > 0 {
                for c in (panel.col + panel.col_span)..(panel.col + new_span) {
                    for r in panel.row..(panel.row + panel.row_span) {
                        if !self.is_cell_empty_excluding(r, c, idx) {
                            return EditAction::None;
                        }
                    }
                }
            }

            self.save_undo(None);
            self.working_layout.panels[idx].col_span = new_span;
            self.dirty = true;
            EditAction::LayoutChanged {
                layout: self.working_layout.clone(),
                added_widget: None,
                removed_widget: None,
            }
        } else {
            EditAction::None
        }
    }

    fn resize_row_span(&mut self, delta: i32) -> EditAction {
        let (row, col) = self.cursor;
        if let Some(idx) = self.panel_index_at(row, col) {
            let panel = &self.working_layout.panels[idx];
            let new_span = panel.row_span as i32 + delta;
            if new_span < 1 {
                return EditAction::None;
            }
            let new_span = new_span as usize;

            // Check bounds
            if panel.row + new_span > self.working_layout.rows {
                return EditAction::None;
            }

            // If growing, check that target cells are empty (excluding this panel)
            if delta > 0 {
                for r in (panel.row + panel.row_span)..(panel.row + new_span) {
                    for c in panel.col..(panel.col + panel.col_span) {
                        if !self.is_cell_empty_excluding(r, c, idx) {
                            return EditAction::None;
                        }
                    }
                }
            }

            self.save_undo(None);
            self.working_layout.panels[idx].row_span = new_span;
            self.dirty = true;
            EditAction::LayoutChanged {
                layout: self.working_layout.clone(),
                added_widget: None,
                removed_widget: None,
            }
        } else {
            EditAction::None
        }
    }

    fn add_column(&mut self) -> EditAction {
        self.save_undo(None);
        self.working_layout.columns += 1;
        self.dirty = true;
        EditAction::LayoutChanged {
            layout: self.working_layout.clone(),
            added_widget: None,
            removed_widget: None,
        }
    }

    fn add_row(&mut self) -> EditAction {
        self.save_undo(None);
        self.working_layout.rows += 1;
        self.dirty = true;
        EditAction::LayoutChanged {
            layout: self.working_layout.clone(),
            added_widget: None,
            removed_widget: None,
        }
    }

    fn remove_last_column(&mut self) -> EditAction {
        if self.working_layout.columns <= 1 {
            return EditAction::None;
        }
        let last_col = self.working_layout.columns - 1;
        // Check no panel occupies the last column
        for panel in &self.working_layout.panels {
            if panel.col + panel.col_span > last_col {
                return EditAction::None;
            }
        }
        self.save_undo(None);
        self.working_layout.columns -= 1;
        // Clamp cursor
        if self.cursor.1 >= self.working_layout.columns {
            self.cursor.1 = self.working_layout.columns - 1;
        }
        self.dirty = true;
        EditAction::LayoutChanged {
            layout: self.working_layout.clone(),
            added_widget: None,
            removed_widget: None,
        }
    }

    fn remove_last_row(&mut self) -> EditAction {
        if self.working_layout.rows <= 1 {
            return EditAction::None;
        }
        let last_row = self.working_layout.rows - 1;
        // Check no panel occupies the last row
        for panel in &self.working_layout.panels {
            if panel.row + panel.row_span > last_row {
                return EditAction::None;
            }
        }
        self.save_undo(None);
        self.working_layout.rows -= 1;
        // Clamp cursor
        if self.cursor.0 >= self.working_layout.rows {
            self.cursor.0 = self.working_layout.rows - 1;
        }
        self.dirty = true;
        EditAction::LayoutChanged {
            layout: self.working_layout.clone(),
            added_widget: None,
            removed_widget: None,
        }
    }

    fn undo(&mut self) -> EditAction {
        if let Some(snapshot) = self.undo_state.take() {
            self.working_layout = snapshot.layout;
            self.dirty = true;
            EditAction::LayoutChanged {
                layout: self.working_layout.clone(),
                added_widget: None,
                removed_widget: None,
            }
        } else {
            EditAction::None
        }
    }

    // ─── Helpers ─────────────────────────────────────────────────────────────

    fn panel_at(&self, row: usize, col: usize) -> Option<&PanelConfig> {
        self.working_layout.panels.iter().find(|p| {
            row >= p.row
                && row < p.row + p.row_span
                && col >= p.col
                && col < p.col + p.col_span
        })
    }

    fn panel_index_at(&self, row: usize, col: usize) -> Option<usize> {
        self.working_layout.panels.iter().position(|p| {
            row >= p.row
                && row < p.row + p.row_span
                && col >= p.col
                && col < p.col + p.col_span
        })
    }

    fn is_cell_empty(&self, row: usize, col: usize) -> bool {
        self.panel_at(row, col).is_none()
    }

    fn is_cell_empty_excluding(&self, row: usize, col: usize, exclude_idx: usize) -> bool {
        self.working_layout
            .panels
            .iter()
            .enumerate()
            .all(|(i, p)| {
                i == exclude_idx
                    || !(row >= p.row
                        && row < p.row + p.row_span
                        && col >= p.col
                        && col < p.col + p.col_span)
            })
    }

    fn save_undo(&mut self, added_widget_config: Option<String>) {
        self.undo_state = Some(UndoSnapshot {
            layout: self.working_layout.clone(),
            added_widget_config,
        });
    }

    fn generate_instance_id(&self, widget_type: &str) -> String {
        // Collect existing instance IDs
        let existing: Vec<String> = self
            .working_layout
            .panels
            .iter()
            .map(|p| p.id.clone().unwrap_or_else(|| p.widget_type.clone()))
            .collect();

        // If the base type name isn't taken, use it
        if !existing.contains(&widget_type.to_string()) {
            return widget_type.to_string();
        }

        // Otherwise try type_2, type_3, ...
        let mut counter = 2;
        loop {
            let candidate = format!("{}_{}", widget_type, counter);
            if !existing.contains(&candidate) {
                return candidate;
            }
            counter += 1;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_layout() -> ProfileConfig {
        ProfileConfig {
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
                PanelConfig {
                    row: 1,
                    col: 0,
                    widget_type: "network".into(),
                    id: None,
                    col_span: 1,
                    row_span: 1,
                },
            ],
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_shift(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    // ── Task 8: Core tests ───────────────────────────────────────────────────

    #[test]
    fn test_enter_sets_initial_state() {
        let layout = make_layout();
        let edit = EditMode::enter(layout.clone());
        assert_eq!(edit.cursor(), (0, 0));
        assert!(!edit.is_dirty());
        assert!(edit.pending_op().is_none());
        assert_eq!(edit.working_layout().columns, 3);
        assert_eq!(edit.working_layout().rows, 2);
        assert_eq!(edit.working_layout().panels.len(), 3);
    }

    #[test]
    fn test_cursor_movement() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Right
        edit.handle_key(key(KeyCode::Right));
        assert_eq!(edit.cursor(), (0, 1));

        // Down
        edit.handle_key(key(KeyCode::Down));
        assert_eq!(edit.cursor(), (1, 1));

        // Left
        edit.handle_key(key(KeyCode::Left));
        assert_eq!(edit.cursor(), (1, 0));

        // Up
        edit.handle_key(key(KeyCode::Up));
        assert_eq!(edit.cursor(), (0, 0));
    }

    #[test]
    fn test_cursor_clamps_to_grid() {
        let layout = make_layout(); // 3 cols, 2 rows
        let mut edit = EditMode::enter(layout);

        // Try to go left from (0,0) — should stay
        edit.handle_key(key(KeyCode::Left));
        assert_eq!(edit.cursor(), (0, 0));

        // Try to go up from (0,0) — should stay
        edit.handle_key(key(KeyCode::Up));
        assert_eq!(edit.cursor(), (0, 0));

        // Move to bottom-right corner
        edit.handle_key(key(KeyCode::Right));
        edit.handle_key(key(KeyCode::Right));
        edit.handle_key(key(KeyCode::Down));
        assert_eq!(edit.cursor(), (1, 2));

        // Try to go right beyond grid — should stay
        edit.handle_key(key(KeyCode::Right));
        assert_eq!(edit.cursor(), (1, 2));

        // Try to go down beyond grid — should stay
        edit.handle_key(key(KeyCode::Down));
        assert_eq!(edit.cursor(), (1, 2));
    }

    #[test]
    fn test_exit_clean() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key(KeyCode::Esc));
        assert_eq!(action, EditAction::Exit);
    }

    #[test]
    fn test_exit_dirty_first_escape_warns() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Make dirty by adding a column
        edit.handle_key(key_shift(KeyCode::Char('C')));
        assert!(edit.is_dirty());

        // First Escape sets ConfirmingExit
        let action = edit.handle_key(key(KeyCode::Esc));
        assert_eq!(action, EditAction::None);
        assert_eq!(
            edit.pending_op(),
            Some(&PendingOp::ConfirmingExit)
        );
    }

    #[test]
    fn test_exit_dirty_second_escape_exits() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Make dirty
        edit.handle_key(key_shift(KeyCode::Char('C')));

        // First Escape → ConfirmingExit
        edit.handle_key(key(KeyCode::Esc));
        assert!(edit.pending_op().is_some());

        // Second Escape → Exit
        let action = edit.handle_key(key(KeyCode::Esc));
        assert_eq!(action, EditAction::Exit);
    }

    #[test]
    fn test_save_returns_save_action() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Save when clean does nothing
        let action = edit.handle_key(key(KeyCode::Char('s')));
        assert_eq!(action, EditAction::None);

        // Make dirty
        edit.handle_key(key_shift(KeyCode::Char('C')));
        assert!(edit.is_dirty());

        // Save when dirty returns Save and clears dirty
        let action = edit.handle_key(key(KeyCode::Char('s')));
        assert_eq!(action, EditAction::Save);
        assert!(!edit.is_dirty());
    }

    // ── Task 9: Panel operation tests ────────────────────────────────────────

    #[test]
    fn test_add_widget_to_empty_cell() {
        let layout = make_layout(); // (0,2) is empty
        let mut edit = EditMode::enter(layout);

        // Move cursor to (0, 2)
        edit.handle_key(key(KeyCode::Right));
        edit.handle_key(key(KeyCode::Right));
        assert_eq!(edit.cursor(), (0, 2));

        // Start add
        edit.handle_key(key(KeyCode::Char('a')));
        assert!(matches!(
            edit.pending_op(),
            Some(PendingOp::Adding { .. })
        ));

        // The types list is sorted; "cpu" is at index 0
        // Navigate to select "cpu" (already at index 0)
        let action = edit.handle_key(key(KeyCode::Enter));

        match action {
            EditAction::LayoutChanged {
                ref added_widget,
                ref removed_widget,
                ..
            } => {
                // "cpu" already exists, so new one should be "cpu_2"
                assert_eq!(
                    added_widget,
                    &Some(("cpu_2".to_string(), "cpu".to_string()))
                );
                assert_eq!(removed_widget, &None);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }

        assert!(edit.is_dirty());
        assert_eq!(edit.working_layout().panels.len(), 4);
    }

    #[test]
    fn test_add_widget_to_occupied_cell_does_nothing() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Cursor is at (0,0) which has "cpu"
        let action = edit.handle_key(key(KeyCode::Char('a')));
        assert_eq!(action, EditAction::None);
        assert!(edit.pending_op().is_none());
    }

    #[test]
    fn test_add_widget_cancel_with_escape() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Move to empty cell (0,2)
        edit.handle_key(key(KeyCode::Right));
        edit.handle_key(key(KeyCode::Right));

        // Start add
        edit.handle_key(key(KeyCode::Char('a')));
        assert!(edit.pending_op().is_some());

        // Cancel
        edit.handle_key(key(KeyCode::Esc));
        assert!(edit.pending_op().is_none());
        assert_eq!(edit.working_layout().panels.len(), 3);
    }

    #[test]
    fn test_delete_widget() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Cursor at (0,0) has "cpu"
        edit.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(
            edit.pending_op(),
            Some(PendingOp::ConfirmingDelete { .. })
        ));

        // Confirm with 'y'
        let action = edit.handle_key(key(KeyCode::Char('y')));
        match action {
            EditAction::LayoutChanged {
                ref removed_widget, ..
            } => {
                assert_eq!(removed_widget, &Some("cpu".to_string()));
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }

        assert!(edit.is_dirty());
        assert_eq!(edit.working_layout().panels.len(), 2);
    }

    #[test]
    fn test_delete_empty_cell_does_nothing() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Move to empty cell (0,2)
        edit.handle_key(key(KeyCode::Right));
        edit.handle_key(key(KeyCode::Right));

        let action = edit.handle_key(key(KeyCode::Char('d')));
        assert_eq!(action, EditAction::None);
        assert!(edit.pending_op().is_none());
    }

    #[test]
    fn test_delete_cancel_with_n() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Start delete on (0,0)
        edit.handle_key(key(KeyCode::Char('d')));
        assert!(edit.pending_op().is_some());

        // Cancel with 'n' (any non-y key cancels)
        let action = edit.handle_key(key(KeyCode::Char('n')));
        assert_eq!(action, EditAction::None);
        assert!(edit.pending_op().is_none());
        assert_eq!(edit.working_layout().panels.len(), 3);
    }

    #[test]
    fn test_move_widget() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Start move from (0,0) which has "cpu"
        edit.handle_key(key(KeyCode::Char('m')));
        assert!(matches!(
            edit.pending_op(),
            Some(PendingOp::Moving { from: (0, 0) })
        ));

        // Move cursor to empty cell (0,2)
        edit.handle_key(key(KeyCode::Right));
        edit.handle_key(key(KeyCode::Right));

        // Confirm move
        let action = edit.handle_key(key(KeyCode::Enter));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                // Find the cpu panel — it should now be at (0,2)
                let cpu = layout
                    .panels
                    .iter()
                    .find(|p| p.widget_type == "cpu")
                    .unwrap();
                assert_eq!(cpu.row, 0);
                assert_eq!(cpu.col, 2);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
        assert!(edit.is_dirty());
    }

    #[test]
    fn test_swap_widgets() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // Start swap from (0,0) which has "cpu"
        edit.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(
            edit.pending_op(),
            Some(PendingOp::Swapping { from: (0, 0) })
        ));

        // Move cursor to (0,1) which has "memory"
        edit.handle_key(key(KeyCode::Right));

        // Confirm swap
        let action = edit.handle_key(key(KeyCode::Enter));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                let cpu = layout
                    .panels
                    .iter()
                    .find(|p| p.widget_type == "cpu")
                    .unwrap();
                let mem = layout
                    .panels
                    .iter()
                    .find(|p| p.widget_type == "memory")
                    .unwrap();
                // cpu should now be at (0,1), memory at (0,0)
                assert_eq!(cpu.row, 0);
                assert_eq!(cpu.col, 1);
                assert_eq!(mem.row, 0);
                assert_eq!(mem.col, 0);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
        assert!(edit.is_dirty());
    }

    // ── Task 10: Resize, grid, and undo tests ───────────────────────────────

    #[test]
    fn test_resize_col_span_grow() {
        let mut layout = make_layout();
        // Remove memory at (0,1) to make room for cpu to grow
        layout.panels.retain(|p| p.widget_type != "memory");
        let mut edit = EditMode::enter(layout);

        // Cursor at (0,0), cpu has col_span=1
        let action = edit.handle_key(key(KeyCode::Char('+')));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                let cpu = layout
                    .panels
                    .iter()
                    .find(|p| p.widget_type == "cpu")
                    .unwrap();
                assert_eq!(cpu.col_span, 2);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
    }

    #[test]
    fn test_resize_col_span_shrink() {
        let mut layout = make_layout();
        // Give cpu col_span=2 and remove memory
        layout.panels[0].col_span = 2;
        layout.panels.retain(|p| p.widget_type != "memory");
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key(KeyCode::Char('-')));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                let cpu = layout
                    .panels
                    .iter()
                    .find(|p| p.widget_type == "cpu")
                    .unwrap();
                assert_eq!(cpu.col_span, 1);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
    }

    #[test]
    fn test_resize_cant_shrink_below_one() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        // cpu at (0,0) has col_span=1, can't shrink further
        let action = edit.handle_key(key(KeyCode::Char('-')));
        assert_eq!(action, EditAction::None);
    }

    #[test]
    fn test_resize_cant_grow_beyond_grid() {
        let layout = make_layout(); // 3 columns
        let mut edit = EditMode::enter(layout);

        // Move to (0,2) — but that's empty, move to (0,1) memory
        edit.handle_key(key(KeyCode::Right));
        // memory at (0,1) col_span=1. Grid has 3 columns.
        // Growing by 1 → col_span=2, reaching col 3 which is fine (0+2=2 < 3? No: 1+2=3 = columns)
        // But (0,2) is empty so it can grow there.
        // Let's grow twice: first should succeed, second should fail (1+3=4 > 3)
        let action = edit.handle_key(key(KeyCode::Char('+')));
        assert!(matches!(action, EditAction::LayoutChanged { .. }));

        // Now memory has col_span=2, occupying (0,1) and (0,2). Try to grow again.
        let action = edit.handle_key(key(KeyCode::Char('+')));
        assert_eq!(action, EditAction::None);
    }

    #[test]
    fn test_add_column() {
        let layout = make_layout(); // 3 columns
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key_shift(KeyCode::Char('C')));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                assert_eq!(layout.columns, 4);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
        assert!(edit.is_dirty());
    }

    #[test]
    fn test_add_row() {
        let layout = make_layout(); // 2 rows
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key_shift(KeyCode::Char('R')));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                assert_eq!(layout.rows, 3);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
        assert!(edit.is_dirty());
    }

    #[test]
    fn test_remove_empty_last_column() {
        let layout = make_layout(); // last column (col=2) is empty except nothing there
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key(KeyCode::Char('c')));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                assert_eq!(layout.columns, 2);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
    }

    #[test]
    fn test_remove_occupied_last_column_fails() {
        let mut layout = make_layout();
        // Add a panel at (0,2) so last column is occupied
        layout.panels.push(PanelConfig {
            row: 0,
            col: 2,
            widget_type: "disk".into(),
            id: None,
            col_span: 1,
            row_span: 1,
        });
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key(KeyCode::Char('c')));
        assert_eq!(action, EditAction::None);
        assert_eq!(edit.working_layout().columns, 3);
    }

    #[test]
    fn test_undo_reverses_last_action() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        let original_cols = edit.working_layout().columns;
        // Add a column
        edit.handle_key(key_shift(KeyCode::Char('C')));
        assert_eq!(edit.working_layout().columns, original_cols + 1);

        // Undo
        let action = edit.handle_key(key(KeyCode::Char('u')));
        match action {
            EditAction::LayoutChanged { ref layout, .. } => {
                assert_eq!(layout.columns, original_cols);
            }
            _ => panic!("expected LayoutChanged, got {:?}", action),
        }
    }

    #[test]
    fn test_undo_with_no_history_does_nothing() {
        let layout = make_layout();
        let mut edit = EditMode::enter(layout);

        let action = edit.handle_key(key(KeyCode::Char('u')));
        assert_eq!(action, EditAction::None);
    }
}
