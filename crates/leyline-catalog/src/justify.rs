//! The justified layout of the grid (ADR 0161 §3, §5): rows of one height,
//! photographs at their own width, each full row filled edge to edge.
//!
//! Pure geometry over the ratios [`crate::Catalog::grid_shapes`] returns.
//! It lives beside the query because the two are one contract — index *n*
//! of the shapes is index *n* of the grid — and because a client that holds
//! the whole layout is what lets the virtual window travel in pixels.

/// The frame the layout fills, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JustifyFrame {
    /// Width of the grid, the length every full row is scaled to.
    pub width: f32,
    /// Height of the image area of a row before it is scaled down to fit:
    /// the size slider's value (ADR 0161 §6).
    pub row_height: f32,
    /// Space between two cells, horizontally and between two rows.
    pub gap: f32,
    /// Height of the cell's foot under the image (ADR 0146 §2), added to
    /// every row without being scaled.
    pub foot: f32,
}

/// One row of the layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JustifiedRow {
    /// Grid index of the row's first photograph.
    pub first: usize,
    /// Top of the row.
    pub y: f32,
    /// Height of the row's image area; the cell is this plus the foot.
    pub image_height: f32,
}

/// The horizontal placement of one photograph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JustifiedCell {
    /// Left edge of the cell.
    pub x: f32,
    /// Width of the cell.
    pub width: f32,
}

/// The layout of a whole query: every row, every cell, and the total height
/// the scrollbar measures.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JustifiedLayout {
    /// Rows from top to bottom.
    pub rows: Vec<JustifiedRow>,
    /// One per photograph, in grid order.
    pub cells: Vec<JustifiedCell>,
    /// Height of the whole grid, from the top of the first row to the
    /// bottom of the last foot.
    pub height: f32,
}

impl JustifiedLayout {
    /// The row holding grid index `index`, or `None` past the end.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        if index >= self.cells.len() {
            return None;
        }
        // Rows are sorted by `first` and the first row starts at 0.
        Some(self.rows.partition_point(|row| row.first <= index) - 1)
    }

    /// The grid indices of row `row`.
    fn indices_of(&self, row: usize) -> std::ops::Range<usize> {
        let end = self
            .rows
            .get(row + 1)
            .map_or(self.cells.len(), |next| next.first);
        self.rows[row].first..end
    }
}

/// Breaks `aspects` (width over height, in grid order) into justified rows.
///
/// A row takes photographs until its natural width — each ratio times
/// `row_height`, plus the gaps — reaches the frame's width, then is scaled
/// down to fill it exactly; a single photograph wider than the frame takes
/// the row alone. The **last row is not stretched** (ADR 0161 §3): it keeps
/// `row_height` and stays left-aligned, so three photographs do not become
/// three giants.
pub fn justify(aspects: &[f32], frame: &JustifyFrame) -> JustifiedLayout {
    let mut layout = JustifiedLayout::default();
    if aspects.is_empty() || frame.width <= 0.0 || frame.row_height <= 0.0 {
        return layout;
    }
    layout.cells.reserve(aspects.len());

    let mut y = 0.0;
    let mut first = 0;
    while first < aspects.len() {
        let mut end = first;
        let mut ratio_sum = 0.0;
        let mut natural_width;
        loop {
            ratio_sum += aspects[end];
            end += 1;
            natural_width = ratio_sum * frame.row_height + frame.gap * (end - first - 1) as f32;
            if natural_width >= frame.width || end == aspects.len() {
                break;
            }
        }

        let full = natural_width >= frame.width;
        let gaps = frame.gap * (end - first - 1) as f32;
        let image_height = if full {
            // Never below a pixel, even in a frame narrower than its gaps.
            ((frame.width - gaps) / ratio_sum).max(1.0)
        } else {
            frame.row_height
        };

        let mut x = 0.0;
        for (offset, aspect) in aspects[first..end].iter().enumerate() {
            let last_of_full_row = full && first + offset + 1 == end;
            // The last cell of a full row ends on the frame's edge exactly,
            // so float rounding never leaves a ragged right margin.
            let width = if last_of_full_row {
                (frame.width - x).max(1.0)
            } else {
                aspect * image_height
            };
            layout.cells.push(JustifiedCell { x, width });
            x += width + frame.gap;
        }

        layout.rows.push(JustifiedRow {
            first,
            y,
            image_height,
        });
        layout.height = y + image_height + frame.foot;
        y = layout.height + frame.gap;
        first = end;
    }
    layout
}

/// The grid index `↑` (`direction < 0`) or `↓` (`direction > 0`) moves to
/// from `index` (ADR 0161 §5): in the row above or below, the photograph
/// whose horizontal centre is closest to the focused one's.
///
/// Stays on `index` at the top or bottom edge, and for an index the layout
/// does not hold.
pub fn step_vertical(layout: &JustifiedLayout, index: usize, direction: i32) -> usize {
    let Some(row) = layout.row_of(index) else {
        return index;
    };
    let target = match direction.signum() {
        -1 if row > 0 => row - 1,
        1 if row + 1 < layout.rows.len() => row + 1,
        _ => return index,
    };
    let centre = |cell: &JustifiedCell| cell.x + cell.width / 2.0;
    let focused = centre(&layout.cells[index]);
    layout
        .indices_of(target)
        .min_by(|&a, &b| {
            let distance = |i: usize| (centre(&layout.cells[i]) - focused).abs();
            distance(a).total_cmp(&distance(b))
        })
        .unwrap_or(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: JustifyFrame = JustifyFrame {
        width: 1000.0,
        row_height: 200.0,
        gap: 0.0,
        foot: 20.0,
    };

    fn widths(layout: &JustifiedLayout, row: usize) -> Vec<f32> {
        layout
            .indices_of(row)
            .map(|i| layout.cells[i].width)
            .collect()
    }

    #[test]
    fn nothing_to_lay_out_gives_an_empty_layout() {
        assert_eq!(justify(&[], &FRAME), JustifiedLayout::default());
        let zero_width = JustifyFrame {
            width: 0.0,
            ..FRAME
        };
        assert!(justify(&[1.5], &zero_width).cells.is_empty());
    }

    #[test]
    fn a_full_row_is_scaled_down_to_fill_the_width() {
        // 1.5 + 1.5 + 1.5 + 1.0 = 5.5 × 200 = 1100 ≥ 1000: four photographs,
        // scaled to 1000 / 5.5.
        let layout = justify(&[1.5, 1.5, 1.5, 1.0, 1.5], &FRAME);
        assert_eq!(layout.rows[0].first, 0);
        assert_eq!(layout.rows[1].first, 4);
        let height = 1000.0 / 5.5;
        assert!((layout.rows[0].image_height - height).abs() < 1e-3);
        let row: f32 = widths(&layout, 0).iter().sum();
        assert!((row - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn the_last_row_keeps_its_height_and_stays_left_aligned() {
        let layout = justify(&[1.5, 1.5, 0.75], &FRAME);
        assert_eq!(layout.rows.len(), 1);
        assert_eq!(layout.rows[0].image_height, 200.0);
        assert_eq!(widths(&layout, 0), vec![300.0, 300.0, 150.0]);
        assert_eq!(layout.height, 220.0);
    }

    #[test]
    fn a_photograph_wider_than_the_grid_takes_the_row_alone() {
        let layout = justify(
            &[3.0, 1.0],
            &JustifyFrame {
                width: 450.0,
                ..FRAME
            },
        );
        assert_eq!(layout.rows[1].first, 1);
        assert_eq!(layout.cells[0].width, 450.0);
        assert_eq!(layout.rows[0].image_height, 150.0);
    }

    #[test]
    fn gaps_and_feet_are_counted_between_cells_and_rows() {
        let frame = JustifyFrame { gap: 10.0, ..FRAME };
        // 3 × 1.7 × 200 + 2 × 10 = 1040 ≥ 1000.
        let layout = justify(&[1.7, 1.7, 1.7, 1.0], &frame);
        let row = &layout.rows[0];
        assert!((row.image_height - 980.0 / 5.1).abs() < 1e-3);
        assert_eq!(layout.cells[1].x, layout.cells[0].width + 10.0);
        let last = layout.cells[2];
        assert_eq!(last.x + last.width, 1000.0);
        assert_eq!(layout.rows[1].y, row.image_height + 20.0 + 10.0);
        assert_eq!(layout.height, layout.rows[1].y + 200.0 + 20.0);
    }

    #[test]
    fn row_of_finds_each_index_and_nothing_past_the_end() {
        let layout = justify(&[1.5, 1.5, 1.5, 1.0, 1.5, 1.5], &FRAME);
        assert_eq!(layout.row_of(0), Some(0));
        assert_eq!(layout.row_of(3), Some(0));
        assert_eq!(layout.row_of(4), Some(1));
        assert_eq!(layout.row_of(5), Some(1));
        assert_eq!(layout.row_of(6), None);
    }

    #[test]
    fn vertical_steps_land_on_the_closest_centre() {
        // Row 0: four cells of 1000 / 4 = 250 (ratios 1.25 × 4 = 5 → 1000).
        // Row 1: 0.5 (100 px, centre 50) and 3.0 (600 px, centre 400).
        let layout = justify(&[1.25, 1.25, 1.25, 1.25, 0.5, 3.0], &FRAME);
        assert_eq!(layout.rows[1].first, 4);
        assert_eq!(step_vertical(&layout, 0, 1), 4); // centre 125 → 50
        assert_eq!(step_vertical(&layout, 2, 1), 5); // centre 625 → 400
        assert_eq!(step_vertical(&layout, 5, -1), 1); // centre 400 → 375
        assert_eq!(step_vertical(&layout, 4, -1), 0); // centre 50 → 125
    }

    #[test]
    fn vertical_steps_stop_at_the_edges() {
        let layout = justify(&[1.25, 1.25, 1.25, 1.25, 1.0], &FRAME);
        assert_eq!(step_vertical(&layout, 1, -1), 1);
        assert_eq!(step_vertical(&layout, 4, 1), 4);
        assert_eq!(step_vertical(&layout, 9, 1), 9);
        assert_eq!(step_vertical(&layout, 1, 0), 1);
    }
}
