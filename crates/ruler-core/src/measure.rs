//! Measurement primitives over an edge map.
//!
//! Everything here works in *device* pixels on a single monitor's edge map.
//! Conversion to and from logical desktop coordinates is the caller's job, so
//! these stay pure and testable without a display.

use crate::edges::EdgeMap;

/// The four axis-aligned ray directions cast from the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    North,
    South,
    West,
    East,
}

impl Direction {
    /// Unit step in image coordinates (y grows downward).
    fn step(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::South => (0, 1),
            Direction::West => (-1, 0),
            Direction::East => (1, 0),
        }
    }
}

/// Distances from the cursor to the first edge in each direction, in device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rays {
    pub north: u32,
    pub south: u32,
    pub west: u32,
    pub east: u32,
}

impl Rays {
    /// Total horizontal span between the west and east edges. The UI reports
    /// spans in logical pixels instead, so this exists for the tests.
    #[cfg(test)]
    pub fn width(self) -> u32 {
        self.west + self.east
    }

    /// Total vertical span between the north and south edges.
    #[cfg(test)]
    pub fn height(self) -> u32 {
        self.north + self.south
    }
}

/// Casts a ray from `(x, y)` and returns the number of steps taken before
/// stopping on an edge pixel or leaving the map.
///
/// The starting pixel is never inspected, so a cursor resting on an edge still
/// measures the span around it rather than collapsing to zero.
pub fn trace_ray(edges: &EdgeMap, x: u32, y: u32, direction: Direction) -> u32 {
    if edges.is_empty() {
        return 0;
    }
    let (dx, dy) = direction.step();
    // Stepping in i64 so an out-of-range cursor coordinate cannot wrap into
    // the map instead of walking straight back out of it.
    let (mut cx, mut cy) = (i64::from(x), i64::from(y));
    let (w, h) = (i64::from(edges.width()), i64::from(edges.height()));
    let mut distance = 0u32;

    loop {
        let nx = cx + i64::from(dx);
        let ny = cy + i64::from(dy);
        if nx < 0 || ny < 0 || nx >= w || ny >= h {
            return distance;
        }
        distance += 1;
        if edges.at(nx as u32, ny as u32) {
            return distance;
        }
        cx = nx;
        cy = ny;
    }
}

/// Casts all four rays from `(x, y)`.
pub fn cast_rays(edges: &EdgeMap, x: u32, y: u32) -> Rays {
    Rays {
        north: trace_ray(edges, x, y, Direction::North),
        south: trace_ray(edges, x, y, Direction::South),
        west: trace_ray(edges, x, y, Direction::West),
        east: trace_ray(edges, x, y, Direction::East),
    }
}

/// Pulls `(x, y)` onto the nearest edge within `radius` device pixels.
///
/// Returns `None` when neither axis found an edge to snap to; otherwise the
/// snapped point, which may have moved on only one axis.
///
/// The axes snap independently: a point beside a vertical rule snaps in x while
/// keeping its y, which is what makes dragging a selection onto a UI border feel
/// predictable. `band` widens the perpendicular search so a near-miss on a
/// slightly diagonal edge still registers; pass the monitor scale factor.
pub fn snap_to_edge(edges: &EdgeMap, x: u32, y: u32, radius: u32, band: u32) -> Option<(u32, u32)> {
    if edges.is_empty() || radius == 0 {
        return None;
    }

    let max_x = edges.width() - 1;
    let max_y = edges.height() - 1;
    let x = x.min(max_x);
    let y = y.min(max_y);
    let band = band.max(1);

    let row_min = y.saturating_sub(band);
    let row_max = y.saturating_add(band).min(max_y);
    let col_min = x.saturating_sub(band);
    let col_max = x.saturating_add(band).min(max_x);

    let best_x = nearest(x, radius, max_x, |candidate| {
        edges.any_in_column(candidate, row_min, row_max)
    });
    let best_y = nearest(y, radius, max_y, |candidate| {
        edges.any_in_row(candidate, col_min, col_max)
    });

    if best_x.is_none() && best_y.is_none() {
        return None;
    }
    Some((best_x.unwrap_or(x), best_y.unwrap_or(y)))
}

/// Nearest index within `radius` of `origin` (capped at `max`) that `hit`
/// accepts, searching outward so the first match is the closest one.
///
/// Ties resolve to the lower index, which keeps snapping stable when an edge
/// sits equidistant on both sides.
fn nearest(origin: u32, radius: u32, max: u32, hit: impl Fn(u32) -> bool) -> Option<u32> {
    // No index lies farther from `origin` than this, so a caller passing a
    // huge radius searches exactly the same candidates instead of spinning
    // through billions of offsets that can never yield one.
    let radius = radius.min(origin.max(max.saturating_sub(origin)));
    (0..=radius).find_map(|offset| {
        let below = (offset <= origin).then(|| origin - offset);
        let above = (offset > 0 && origin.saturating_add(offset) <= max).then(|| origin + offset);
        below.into_iter().chain(above).find(|c| hit(*c))
    })
}

/// An inclusive rectangle in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl PixelRect {
    /// Builds a normalised rect from two corners, clamped to `(max_x, max_y)`.
    ///
    /// Corners are signed because a drag can run off the top-left of the
    /// monitor before being clamped back onto it.
    pub fn from_corners(x0: i32, y0: i32, x1: i32, y1: i32, max_x: u32, max_y: u32) -> Self {
        // Clamping in i64 so a max beyond i32::MAX cannot wrap the bound.
        let clamp = |v: i32, max: u32| i64::from(v).clamp(0, i64::from(max)) as u32;
        Self {
            left: clamp(x0.min(x1), max_x),
            top: clamp(y0.min(y1), max_y),
            right: clamp(x0.max(x1), max_x),
            bottom: clamp(y0.max(y1), max_y),
        }
    }

    /// Saturating: the fields are public, so an inverted rect reports a zero
    /// span rather than panicking on the underflow.
    pub fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// Rectangles smaller than this are left untouched by shrink-to-fit; there is
/// no meaningful content to tighten onto.
const MIN_SHRINK_SIZE: u32 = 5;

/// Tightens `rect` inward until each side rests on the outermost edge pixel it
/// contains, discarding surrounding whitespace.
///
/// `rect` is normalised and clamped to the map first. Its fields are public,
/// so an inverted or off-map rectangle is constructible, and scanning one
/// would walk billions of coordinates that cannot hold an edge and then
/// report bounds that are not on the monitor.
///
/// Returns the clamped rectangle when it is too small or holds no edges.
pub fn shrink_to_content(edges: &EdgeMap, rect: PixelRect) -> PixelRect {
    if edges.is_empty() {
        return rect;
    }
    let max_x = edges.width() - 1;
    let max_y = edges.height() - 1;
    let rect = PixelRect {
        left: rect.left.min(rect.right).min(max_x),
        top: rect.top.min(rect.bottom).min(max_y),
        right: rect.right.max(rect.left).min(max_x),
        bottom: rect.bottom.max(rect.top).min(max_y),
    };
    if rect.width() < MIN_SHRINK_SIZE || rect.height() < MIN_SHRINK_SIZE {
        return rect;
    }

    let top = (rect.top..=rect.bottom)
        .find(|row| edges.any_in_row(*row, rect.left, rect.right))
        .unwrap_or(rect.top);
    let bottom = (top..=rect.bottom)
        .rev()
        .find(|row| edges.any_in_row(*row, rect.left, rect.right))
        .unwrap_or(rect.bottom);
    let left = (rect.left..=rect.right)
        .find(|col| edges.any_in_column(*col, top, bottom))
        .unwrap_or(rect.left);
    let right = (left..=rect.right)
        .rev()
        .find(|col| edges.any_in_column(*col, top, bottom))
        .unwrap_or(rect.right);

    PixelRect {
        left,
        top,
        right,
        bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_stops_on_the_first_edge() {
        let edges = EdgeMap::from_ascii(&[
            "..........",
            "..........",
            "..#....#..",
            "..........",
            "..........",
        ]);
        assert_eq!(trace_ray(&edges, 5, 2, Direction::West), 3);
        assert_eq!(trace_ray(&edges, 5, 2, Direction::East), 2);
    }

    #[test]
    fn ray_runs_to_the_boundary_when_unobstructed() {
        let edges = EdgeMap::from_ascii(&["....", "....", "....", "...."]);
        assert_eq!(trace_ray(&edges, 1, 1, Direction::West), 1);
        assert_eq!(trace_ray(&edges, 1, 1, Direction::East), 2);
        assert_eq!(trace_ray(&edges, 1, 1, Direction::North), 1);
        assert_eq!(trace_ray(&edges, 1, 1, Direction::South), 2);
    }

    #[test]
    fn ray_ignores_an_edge_under_the_cursor_itself() {
        // Standing on an edge must still measure the surrounding span.
        let edges = EdgeMap::from_ascii(&["#..#"]);
        assert_eq!(trace_ray(&edges, 0, 0, Direction::East), 3);
    }

    #[test]
    fn casting_all_rays_reports_span_totals() {
        let edges = EdgeMap::from_ascii(&["..#....", ".......", "#.....#", ".......", "..#...."]);
        let rays = cast_rays(&edges, 2, 2);
        assert_eq!(rays.north, 2);
        assert_eq!(rays.south, 2);
        assert_eq!(rays.west, 2);
        assert_eq!(rays.east, 4);
        assert_eq!(rays.width(), 6);
        assert_eq!(rays.height(), 4);
    }

    #[test]
    fn snapping_pulls_each_axis_independently() {
        // A vertical rule at x == 4 and nothing horizontal nearby.
        let edges = EdgeMap::from_ascii(&[
            "....#.....",
            "....#.....",
            "....#.....",
            "....#.....",
            "....#.....",
        ]);
        let snap = snap_to_edge(&edges, 6, 2, 5, 1).expect("expected a snap");
        assert_eq!(snap.0, 4, "x should snap onto the rule");
        assert_eq!(snap.1, 2, "y has no edge to snap to and must stay put");
    }

    #[test]
    fn snapping_prefers_the_nearest_edge() {
        let edges = EdgeMap::from_ascii(&["#....#...."]);
        let snap = snap_to_edge(&edges, 4, 0, 5, 1).expect("expected a snap");
        assert_eq!(snap.0, 5);
    }

    #[test]
    fn snapping_is_a_no_op_outside_the_radius() {
        let edges = EdgeMap::from_ascii(&["#........."]);
        assert_eq!(snap_to_edge(&edges, 8, 0, 3, 1), None);

        // A zero radius disables snapping entirely.
        assert_eq!(snap_to_edge(&edges, 1, 0, 0, 1), None);
    }

    #[test]
    fn shrink_tightens_onto_content() {
        let edges = EdgeMap::from_ascii(&[
            "..........",
            "..........",
            "...####...",
            "...#..#...",
            "...####...",
            "..........",
            "..........",
        ]);
        let rect = PixelRect {
            left: 0,
            top: 0,
            right: 9,
            bottom: 6,
        };
        assert_eq!(
            shrink_to_content(&edges, rect),
            PixelRect {
                left: 3,
                top: 2,
                right: 6,
                bottom: 4
            }
        );
    }

    #[test]
    fn shrink_leaves_empty_or_tiny_rects_alone() {
        let blank = EdgeMap::from_ascii(&[
            "..........",
            "..........",
            "..........",
            "..........",
            "..........",
            "..........",
        ]);
        let rect = PixelRect {
            left: 0,
            top: 0,
            right: 9,
            bottom: 5,
        };
        assert_eq!(shrink_to_content(&blank, rect), rect);

        let tiny = PixelRect {
            left: 0,
            top: 0,
            right: 3,
            bottom: 3,
        };
        assert_eq!(shrink_to_content(&blank, tiny), tiny);
    }

    #[test]
    fn corner_construction_normalises_and_clamps() {
        let rect = PixelRect::from_corners(9, 8, 2, 1, 5, 5);
        assert_eq!(
            rect,
            PixelRect {
                left: 2,
                top: 1,
                right: 5,
                bottom: 5
            }
        );

        let negative = PixelRect::from_corners(-10, -10, 3, 3, 9, 9);
        assert_eq!(
            negative,
            PixelRect {
                left: 0,
                top: 0,
                right: 3,
                bottom: 3
            }
        );
    }

    #[test]
    fn extreme_coordinates_walk_out_rather_than_wrapping() {
        // An out-of-range cursor must leave the map immediately, not wrap a
        // signed step back onto real pixels.
        let edges = EdgeMap::from_ascii(&["#..#", "....", "#..#"]);
        for d in [
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            assert_eq!(trace_ray(&edges, u32::MAX, u32::MAX, d), 0, "{d:?}");
            assert_eq!(trace_ray(&edges, u32::MAX, 1, d), 0, "{d:?}");
        }

        // Snapping clamps the cursor back onto the map instead of overflowing
        // the outward search.
        assert!(snap_to_edge(&edges, u32::MAX, u32::MAX, 4, u32::MAX).is_some());

        // A max bound past i32::MAX must not wrap the corner clamp.
        let wide = PixelRect::from_corners(-5, -5, i32::MAX, i32::MAX, u32::MAX, u32::MAX);
        assert_eq!(wide.left, 0);
        assert_eq!(wide.right, i32::MAX as u32);
    }

    #[test]
    fn an_unbounded_snap_radius_searches_only_the_map() {
        // `radius` is a public `u32`, so the outward search must cap itself at
        // the map bounds rather than stepping through four billion offsets.
        let edges = EdgeMap::from_ascii(&["....#....."]);
        assert_eq!(snap_to_edge(&edges, 2, 0, u32::MAX, 1), Some((4, 0)));

        let blank = EdgeMap::from_ascii(&[".........."]);
        assert_eq!(snap_to_edge(&blank, 2, 0, u32::MAX, u32::MAX), None);
    }

    #[test]
    fn shrink_normalises_and_clamps_the_rect_it_is_given() {
        let edges = EdgeMap::from_ascii(&[
            "..........",
            "..........",
            "...####...",
            "...#..#...",
            "...####...",
            "..........",
            "..........",
        ]);
        let content = PixelRect {
            left: 3,
            top: 2,
            right: 6,
            bottom: 4,
        };

        // A rect running past the monitor is pulled back onto it before the
        // scan, instead of walking to u32::MAX.
        let huge = PixelRect {
            left: 0,
            top: 0,
            right: u32::MAX,
            bottom: u32::MAX,
        };
        assert_eq!(shrink_to_content(&edges, huge), content);

        // Corners in the wrong order normalise rather than underflowing.
        let inverted = PixelRect {
            left: 9,
            top: 6,
            right: 0,
            bottom: 0,
        };
        assert_eq!(inverted.width(), 0);
        assert_eq!(shrink_to_content(&edges, inverted), content);

        // With nothing to tighten onto, the result is still on the map.
        let blank = EdgeMap::from_ascii(&[".........."; 7]);
        assert_eq!(
            shrink_to_content(&blank, huge),
            PixelRect {
                left: 0,
                top: 0,
                right: 9,
                bottom: 6
            }
        );
    }
}
