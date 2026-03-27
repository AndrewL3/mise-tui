use std::collections::HashMap;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::config::LayoutConfig;

// ─── LayoutEngine ─────────────────────────────────────────────────────────────

pub struct LayoutEngine {
    rows: usize,
    cols: usize,
    header: bool,
    footer: bool,
    occupancy: HashMap<(usize, usize), String>, // (row, col) -> instance_id
    anchors: HashMap<String, (usize, usize)>,   // instance_id -> anchor cell
    spans: HashMap<String, (usize, usize)>,     // instance_id -> (row_span, col_span)
    row_constraints: Vec<Constraint>,
    col_constraints: Vec<Constraint>,
}

impl LayoutEngine {
    /// Build the layout engine from a `LayoutConfig`.
    ///
    /// Resolves percentage constraints for rows and columns. When a sizing
    /// array is omitted, cells are split equally (last item absorbs remainder:
    /// `100 - pct * (n-1)`).
    pub fn from_config(config: &LayoutConfig) -> Result<Self> {
        let rows = config.rows;
        let cols = config.columns;

        let mut occupancy: HashMap<(usize, usize), String> = HashMap::new();
        let mut anchors: HashMap<String, (usize, usize)> = HashMap::new();
        let mut spans: HashMap<String, (usize, usize)> = HashMap::new();

        for panel in &config.panels {
            let id = panel.instance_id().to_string();
            let row_span = panel.row_span;
            let col_span = panel.col_span;

            anchors.insert(id.clone(), (panel.row, panel.col));
            spans.insert(id.clone(), (row_span, col_span));

            for r in panel.row..(panel.row + row_span).min(rows) {
                for c in panel.col..(panel.col + col_span).min(cols) {
                    occupancy.insert((r, c), id.clone());
                }
            }
        }

        let row_constraints = resolve_constraints(rows, config.row_heights.as_deref())?;
        let col_constraints = resolve_constraints(cols, config.column_widths.as_deref())?;

        Ok(Self {
            rows,
            cols,
            header: config.header,
            footer: config.footer,
            occupancy,
            anchors,
            spans,
            row_constraints,
            col_constraints,
        })
    }

    /// Split the area into (header, grid, footer) regions.
    ///
    /// Header and footer each take exactly 1 row. If neither is enabled the
    /// entire area is the grid.
    pub fn split_chrome(&self, area: Rect) -> (Option<Rect>, Rect, Option<Rect>) {
        match (self.header, self.footer) {
            (false, false) => (None, area, None),
            (true, false) => {
                let areas =
                    Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).split(area);
                (Some(areas[0]), areas[1], None)
            }
            (false, true) => {
                let areas =
                    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(area);
                (None, areas[0], Some(areas[1]))
            }
            (true, true) => {
                let areas = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                ])
                .split(area);
                (Some(areas[0]), areas[1], Some(areas[2]))
            }
        }
    }

    /// Resolve the terminal area into a map of `(row, col)` -> `Rect` for
    /// every panel, keyed by its anchor cell. Spanning panels produce a single
    /// merged `Rect` covering all their constituent cells.
    pub fn resolve_rects(&self, area: Rect) -> HashMap<(usize, usize), Rect> {
        let all_cells = self.resolve_all_rects(area);

        let mut result = HashMap::new();

        for (id, &(anchor_row, anchor_col)) in &self.anchors {
            let &(row_span, col_span) = self.spans.get(id).unwrap();

            let mut min_x = u16::MAX;
            let mut min_y = u16::MAX;
            let mut max_x: u16 = 0;
            let mut max_y: u16 = 0;

            for r in anchor_row..(anchor_row + row_span).min(self.rows) {
                for c in anchor_col..(anchor_col + col_span).min(self.cols) {
                    if let Some(cell) = all_cells.get(&(r, c)) {
                        min_x = min_x.min(cell.x);
                        min_y = min_y.min(cell.y);
                        max_x = max_x.max(cell.x + cell.width);
                        max_y = max_y.max(cell.y + cell.height);
                    }
                }
            }

            if min_x < max_x && min_y < max_y {
                result.insert(
                    (anchor_row, anchor_col),
                    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y),
                );
            }
        }

        result
    }

    /// Resolve the terminal area into a map of `(row, col)` -> `Rect` for
    /// every cell in the grid, including unoccupied ones.
    pub fn resolve_all_rects(&self, area: Rect) -> HashMap<(usize, usize), Rect> {
        let (_header, grid_area, _footer) = self.split_chrome(area);

        let row_areas = Layout::vertical(&self.row_constraints).split(grid_area);

        let mut result = HashMap::new();

        for row in 0..self.rows {
            let row_rect = row_areas[row];
            let col_areas = Layout::horizontal(&self.col_constraints).split(row_rect);

            for col in 0..self.cols {
                result.insert((row, col), col_areas[col]);
            }
        }

        result
    }

    /// Return the instance ID of the widget at `(row, col)`, if any.
    pub fn instance_at(&self, row: usize, col: usize) -> Option<&str> {
        self.occupancy.get(&(row, col)).map(|s| s.as_str())
    }

    /// Return all occupied cells, sorted by `(row, col)`.
    /// Returns anchor cells only (not secondary cells of spanning panels).
    pub fn occupied_cells(&self) -> Vec<(usize, usize)> {
        let mut cells: Vec<(usize, usize)> = self.anchors.values().copied().collect();
        cells.sort();
        cells
    }

    /// Given any occupied cell, return the anchor cell (top-left) of the panel
    /// that occupies it. Returns `None` for unoccupied cells.
    pub fn anchor_for(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        let id = self.occupancy.get(&(row, col))?;
        self.anchors.get(id).copied()
    }

    /// Return the grid dimensions as `(rows, cols)`.
    pub fn grid_dimensions(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    pub fn has_header(&self) -> bool {
        self.header
    }

    pub fn has_footer(&self) -> bool {
        self.footer
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a slice of percentage strings (e.g. `["70%", "30%"]`) into ratatui
/// `Constraint::Percentage` values, or build equal-split constraints when
/// `sizes` is `None`.
///
/// When constructing equal splits, the last item absorbs any remainder so that
/// the constraints always sum to 100.
fn resolve_constraints(n: usize, sizes: Option<&[String]>) -> Result<Vec<Constraint>> {
    if n == 0 {
        return Ok(vec![]);
    }

    match sizes {
        Some(pcts) => {
            let mut constraints = Vec::with_capacity(pcts.len());
            for s in pcts {
                let trimmed = s.trim();
                let num_str = trimmed
                    .strip_suffix('%')
                    .ok_or_else(|| eyre!("invalid percentage value '{s}'"))?;
                let pct: u16 = num_str
                    .parse()
                    .map_err(|_| eyre!("invalid percentage value '{s}'"))?;
                constraints.push(Constraint::Percentage(pct));
            }
            Ok(constraints)
        }
        None => {
            // Equal split: each cell gets floor(100/n)%; last one absorbs remainder
            let base = 100u16 / n as u16;
            let remainder = 100u16 - base * (n as u16 - 1);
            let mut constraints: Vec<Constraint> =
                (0..n - 1).map(|_| Constraint::Percentage(base)).collect();
            constraints.push(Constraint::Percentage(remainder));
            Ok(constraints)
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PanelConfig;

    fn make_config(rows: usize, cols: usize, panels: Vec<PanelConfig>) -> LayoutConfig {
        LayoutConfig {
            rows,
            columns: cols,
            column_widths: None,
            row_heights: None,
            header: false,
            footer: false,
            panels,
        }
    }

    fn panel(row: usize, col: usize, widget_type: &str) -> PanelConfig {
        PanelConfig {
            row,
            col,
            widget_type: widget_type.to_string(),
            id: None,
            col_span: 1,
            row_span: 1,
        }
    }

    fn spanning_panel(
        row: usize,
        col: usize,
        widget_type: &str,
        row_span: usize,
        col_span: usize,
    ) -> PanelConfig {
        PanelConfig {
            row,
            col,
            widget_type: widget_type.to_string(),
            id: None,
            col_span,
            row_span,
        }
    }

    #[test]
    fn occupancy_map_populated() {
        let config = make_config(2, 2, vec![panel(0, 0, "cpu"), panel(1, 1, "memory")]);
        let engine = LayoutEngine::from_config(&config).unwrap();
        assert_eq!(engine.instance_at(0, 0), Some("cpu"));
        assert_eq!(engine.instance_at(1, 1), Some("memory"));
        assert_eq!(engine.instance_at(0, 1), None);
        assert_eq!(engine.instance_at(1, 0), None);
    }

    #[test]
    fn occupied_cells_sorted() {
        let config = make_config(
            2,
            2,
            vec![
                panel(1, 1, "memory"),
                panel(0, 0, "cpu"),
                panel(0, 1, "network"),
            ],
        );
        let engine = LayoutEngine::from_config(&config).unwrap();
        assert_eq!(engine.occupied_cells(), vec![(0, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn grid_dimensions() {
        let config = make_config(3, 4, vec![]);
        let engine = LayoutEngine::from_config(&config).unwrap();
        assert_eq!(engine.grid_dimensions(), (3, 4));
    }

    #[test]
    fn equal_split_2x2() {
        let config = make_config(
            2,
            2,
            vec![
                panel(0, 0, "cpu"),
                panel(0, 1, "memory"),
                panel(1, 0, "network"),
                panel(1, 1, "temps"),
            ],
        );
        let engine = LayoutEngine::from_config(&config).unwrap();
        let rects = engine.resolve_rects(Rect::new(0, 0, 100, 50));
        assert_eq!(rects.len(), 4);
        for rect in rects.values() {
            assert!(rect.width > 0);
            assert!(rect.height > 0);
        }
        let tl = rects[&(0, 0)];
        assert_eq!(tl.x, 0);
        assert_eq!(tl.y, 0);
    }

    #[test]
    fn header_footer_reduce_grid_area() {
        let mut config = make_config(1, 1, vec![panel(0, 0, "cpu")]);
        config.header = true;
        config.footer = true;
        let engine = LayoutEngine::from_config(&config).unwrap();
        let rects = engine.resolve_rects(Rect::new(0, 0, 80, 24));
        let cell = rects[&(0, 0)];
        assert_eq!(cell.height, 22);
    }

    #[test]
    fn explicit_percentages() {
        let config = LayoutConfig {
            rows: 2,
            columns: 1,
            column_widths: None,
            row_heights: Some(vec!["70%".to_string(), "30%".to_string()]),
            header: false,
            footer: false,
            panels: vec![panel(0, 0, "cpu"), panel(1, 0, "memory")],
        };
        let engine = LayoutEngine::from_config(&config).unwrap();
        let rects = engine.resolve_rects(Rect::new(0, 0, 80, 100));
        let top = rects[&(0, 0)];
        let bottom = rects[&(1, 0)];
        assert_eq!(top.height, 70);
        assert_eq!(bottom.height, 30);
    }

    #[test]
    fn split_chrome_header_and_footer() {
        let mut config = make_config(1, 1, vec![panel(0, 0, "cpu")]);
        config.header = true;
        config.footer = true;
        let engine = LayoutEngine::from_config(&config).unwrap();
        let (header, grid, footer) = engine.split_chrome(Rect::new(0, 0, 80, 24));
        assert!(header.is_some());
        assert_eq!(header.unwrap().height, 1);
        assert!(footer.is_some());
        assert_eq!(footer.unwrap().height, 1);
        assert_eq!(grid.height, 22);
    }

    #[test]
    fn spanning_occupancy_fills_all_cells() {
        let config = make_config(
            2,
            3,
            vec![spanning_panel(0, 0, "cpu", 2, 2), panel(0, 2, "memory")],
        );
        let engine = LayoutEngine::from_config(&config).unwrap();
        assert_eq!(engine.instance_at(0, 0), Some("cpu"));
        assert_eq!(engine.instance_at(0, 1), Some("cpu"));
        assert_eq!(engine.instance_at(1, 0), Some("cpu"));
        assert_eq!(engine.instance_at(1, 1), Some("cpu"));
        assert_eq!(engine.instance_at(0, 2), Some("memory"));
        assert_eq!(engine.instance_at(1, 2), None);
    }

    #[test]
    fn occupied_cells_returns_anchors_only() {
        let config = make_config(
            2,
            3,
            vec![spanning_panel(0, 0, "cpu", 2, 2), panel(0, 2, "memory")],
        );
        let engine = LayoutEngine::from_config(&config).unwrap();
        assert_eq!(engine.occupied_cells(), vec![(0, 0), (0, 2)]);
    }

    #[test]
    fn anchor_for_returns_anchor_cell() {
        let config = make_config(
            2,
            3,
            vec![spanning_panel(0, 0, "cpu", 2, 2), panel(0, 2, "memory")],
        );
        let engine = LayoutEngine::from_config(&config).unwrap();
        assert_eq!(engine.anchor_for(0, 0), Some((0, 0)));
        assert_eq!(engine.anchor_for(1, 1), Some((0, 0)));
        assert_eq!(engine.anchor_for(0, 2), Some((0, 2)));
        assert_eq!(engine.anchor_for(1, 2), None);
    }

    #[test]
    fn resolve_rects_merges_spanning_cells() {
        let config = make_config(
            2,
            2,
            vec![
                spanning_panel(0, 0, "cpu", 1, 2),
                panel(1, 0, "memory"),
                panel(1, 1, "network"),
            ],
        );
        let engine = LayoutEngine::from_config(&config).unwrap();
        let rects = engine.resolve_rects(Rect::new(0, 0, 100, 50));
        assert_eq!(rects.len(), 3);
        let cpu_rect = rects[&(0, 0)];
        let mem_rect = rects[&(1, 0)];
        assert_eq!(cpu_rect.width, 100);
        assert!(cpu_rect.width > mem_rect.width);
    }

    #[test]
    fn resolve_rects_2x2_span() {
        let config = make_config(2, 2, vec![spanning_panel(0, 0, "cpu", 2, 2)]);
        let engine = LayoutEngine::from_config(&config).unwrap();
        let rects = engine.resolve_rects(Rect::new(0, 0, 100, 50));
        assert_eq!(rects.len(), 1);
        let rect = rects[&(0, 0)];
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 0);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 50);
    }
}
