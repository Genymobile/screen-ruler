//! Measurement primitives over an edge map.
//!
//! Everything here works in *device* pixels on a single monitor's edge map.
//! Conversion to and from logical desktop coordinates is the caller's job, so
//! these stay pure and testable without a display.

use image::RgbaImage;

use crate::boundary::{self, Axis, Bounds};
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

/// The boundaries around `(x, y)`: where the nearest edge in each direction
/// really lies, in device px, or the image border when a ray finds none.
///
/// The width is `right - left` whichever pixel of each transition the edge
/// map marked; see [`crate::boundary`].
pub fn measure_around(edges: &EdgeMap, image: &RgbaImage, x: u32, y: u32) -> Bounds {
    if edges.is_empty() {
        return Bounds::default();
    }
    let (w, h) = (edges.width(), edges.height());
    let (x, y) = (x.min(w - 1), y.min(h - 1));
    let rays = cast_rays(edges, x, y);
    // Where a ray stopped on an edge pixel, read the boundary off the image,
    // approaching from the cursor. A ray that ran off the map ends at the
    // border.
    let hit = |(cx, cy): (u32, u32)| edges.at(cx, cy);
    let left = x - rays.west;
    let right = x + rays.east;
    let top = y - rays.north;
    let bottom = y + rays.south;
    Bounds {
        left: if rays.west > 0 && hit((left, y)) {
            boundary::approach(image, Axis::Horizontal, y, x, left)
        } else {
            0.0
        },
        right: if rays.east > 0 && hit((right, y)) {
            boundary::approach(image, Axis::Horizontal, y, x, right)
        } else {
            w as f32
        },
        top: if rays.north > 0 && hit((x, top)) {
            boundary::approach(image, Axis::Vertical, x, y, top)
        } else {
            0.0
        },
        bottom: if rays.south > 0 && hit((x, bottom)) {
            boundary::approach(image, Axis::Vertical, x, y, bottom)
        } else {
            h as f32
        },
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
/// nearly vertical or horizontal edge still registers; pass the monitor scale
/// factor.
///
/// An axis only snaps onto an edge that runs across it: x onto an edge pixel
/// in a vertical run of at least three, y onto one in a horizontal run. Steep
/// diagonals (2:1 and up) still have such runs; a 45° edge does not, and is
/// not snapped to — this is a ruler for axis-aligned UI.
/// Otherwise, near a corner, the band catches the *other* edge
/// and the axis snaps short of the one it is aiming for.
pub fn snap_to_edge(edges: &EdgeMap, x: u32, y: u32, radius: u32, band: u32) -> Option<(u32, u32)> {
    let (best_x, best_y) = snap_axes(edges, x, y, radius, band)?;
    Some((
        best_x.unwrap_or(x.min(edges.width() - 1)),
        best_y.unwrap_or(y.min(edges.height() - 1)),
    ))
}

/// The edge each axis snaps onto, `None` for an axis that does not; `None`
/// overall when neither does.
fn snap_axes(
    edges: &EdgeMap,
    x: u32,
    y: u32,
    radius: u32,
    band: u32,
) -> Option<(Option<u32>, Option<u32>)> {
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

    // Part of a run of at least three along the axis: Canny can mark both
    // pixels of a sharp step, so a horizontal edge is often two pixels thick,
    // and a pair stacked vertically does not make it a vertical edge.
    let run =
        |at: &dyn Fn(i64) -> bool| (at(-1) && at(1)) || (at(-2) && at(-1)) || (at(1) && at(2));
    let vertical = |cx: u32, cy: u32| {
        edges.at(cx, cy)
            && run(&|d| {
                let r = i64::from(cy) + d;
                (0..=i64::from(max_y)).contains(&r) && edges.at(cx, r as u32)
            })
    };
    let horizontal = |cx: u32, cy: u32| {
        edges.at(cx, cy)
            && run(&|d| {
                let c = i64::from(cx) + d;
                (0..=i64::from(max_x)).contains(&c) && edges.at(c as u32, cy)
            })
    };
    let best_x = nearest(x, radius, max_x, |candidate| {
        (row_min..=row_max).any(|row| vertical(candidate, row))
    });
    let best_y = nearest(y, radius, max_y, |candidate| {
        (col_min..=col_max).any(|col| horizontal(col, candidate))
    });

    if best_x.is_none() && best_y.is_none() {
        return None;
    }
    Some((best_x, best_y))
}

/// [`snap_to_edge`] onto boundaries: `(x, y)` is a device-pixel position, and
/// each axis that snaps lands where its edge really lies, approached from the
/// cursor along its row or column, the other keeping `(x, y)` exactly.
pub fn snap_to_boundary(
    edges: &EdgeMap,
    image: &RgbaImage,
    x: f32,
    y: f32,
    radius: u32,
    band: u32,
) -> Option<(f32, f32)> {
    if edges.is_empty() {
        return None;
    }
    let px = (x.max(0.0) as u32).min(edges.width() - 1);
    let py = (y.max(0.0) as u32).min(edges.height() - 1);
    let (sx, sy) = snap_axes(edges, px, py, radius, band)?;
    let snapped_x = sx.map(|sx| boundary::approach(image, Axis::Horizontal, py, px, sx));
    let snapped_y = sy.map(|sy| boundary::approach(image, Axis::Vertical, px, py, sy));
    Some((snapped_x.unwrap_or(x), snapped_y.unwrap_or(y)))
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

/// [`shrink_to_content`] onto boundaries: each side that tightened lands where
/// the content's outermost edge really lies, read off the image along a row or
/// column through that edge. A side with no edge on it stays where `rect` put
/// it, as a position (so an empty rect comes back as given).
pub fn shrink_to_bounds(edges: &EdgeMap, image: &RgbaImage, rect: PixelRect) -> Bounds {
    let r = shrink_to_content(edges, rect);
    let mut bounds = Bounds {
        left: r.left as f32,
        top: r.top as f32,
        right: r.right as f32,
        bottom: r.bottom as f32,
    };
    // Left untouched for being too small (or with no map): keep it so.
    if edges.is_empty() || r.width() < MIN_SHRINK_SIZE || r.height() < MIN_SHRINK_SIZE {
        return bounds;
    }
    // Each side closes in from where the rect had it, clamped onto the map.
    let (max_x, max_y) = (edges.width() - 1, edges.height() - 1);
    let outer = PixelRect {
        left: rect.left.min(rect.right).min(max_x),
        top: rect.top.min(rect.bottom).min(max_y),
        right: rect.right.max(rect.left).min(max_x),
        bottom: rect.bottom.max(rect.top).min(max_y),
    };
    // A line through each outermost edge pixel, to read its boundary along.
    let on_row = |row: u32| (r.left..=r.right).find(|&col| edges.at(col, row));
    let on_column = |col: u32| (r.top..=r.bottom).find(|&row| edges.at(col, row));
    if let Some(col) = on_row(r.top) {
        bounds.top = boundary::approach(image, Axis::Vertical, col, outer.top, r.top);
    }
    if let Some(col) = on_row(r.bottom) {
        bounds.bottom = boundary::approach(image, Axis::Vertical, col, outer.bottom, r.bottom);
    }
    if let Some(row) = on_column(r.left) {
        bounds.left = boundary::approach(image, Axis::Horizontal, row, outer.left, r.left);
    }
    if let Some(row) = on_column(r.right) {
        bounds.right = boundary::approach(image, Axis::Horizontal, row, outer.right, r.right);
    }
    bounds
}

/// The boundaries of a region found by the region map, given as its inclusive
/// pixel extent. The region's pixels stop short of the edge pixels around it,
/// so each side is read off the image across the region's middle, approaching
/// from inside: the inner side of a border, as the crosshair measures it from
/// within. Where there is no pixel beyond the region, its own extent stands.
pub fn region_bounds(image: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> Bounds {
    let (mid_x, mid_y) = (x + width / 2, y + height / 2);
    let (right, bottom) = (x + width, y + height);
    let (w, h) = (image.width(), image.height());
    let side = |axis, line, outside: Option<u32>, extent: u32, from: u32| {
        outside.map_or(extent as f32, |at| {
            boundary::approach(image, axis, line, from, at)
        })
    };
    Bounds {
        left: side(Axis::Horizontal, mid_y, x.checked_sub(1), x, mid_x),
        right: side(
            Axis::Horizontal,
            mid_y,
            (right < w).then_some(right),
            right,
            mid_x,
        ),
        top: side(Axis::Vertical, mid_x, y.checked_sub(1), y, mid_y),
        bottom: side(
            Axis::Vertical,
            mid_x,
            (bottom < h).then_some(bottom),
            bottom,
            mid_y,
        ),
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
        let edges = EdgeMap::from_ascii(&["#....#...."; 3]);
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
        let edges = EdgeMap::from_ascii(&["#..#", "#..#", "#..#"]);
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
        let edges = EdgeMap::from_ascii(&["....#....."; 3]);
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

    // ---- boundaries ---------------------------------------------------

    use crate::boundary::tests::{bordered, boxed};
    use crate::edges;

    fn canny(image: &RgbaImage) -> EdgeMap {
        let (low, high) = edges::sensitivity_to_thresholds(edges::DEFAULT_SENSITIVITY);
        edges::canny_color(image, low, high)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.05
    }

    #[test]
    fn a_box_measures_its_exact_size() {
        // Sharp, as on a 1x output: 200 px across is 200, not 199 or 201.
        let image = boxed(320, 320, 60.0, 60.0, 260.0, 260.0);
        let edges = canny(&image);
        let b = measure_around(&edges, &image, 160, 160);
        assert_eq!((b.width(), b.height()), (200.0, 200.0), "{b:?}");
        assert_eq!((b.left, b.top), (60.0, 60.0));
    }

    #[test]
    fn an_antialiased_box_measures_its_fractional_size() {
        // 137 x 89 logical at 1.5023x: 205.82 x 133.71 device px, edges
        // falling between pixels.
        let s = 1.502_343_8_f32;
        let (x0, y0) = (40.3_f32, 30.6_f32);
        let image = boxed(300, 220, x0, y0, x0 + 137.0 * s, y0 + 89.0 * s);
        let edges = canny(&image);
        let b = measure_around(&edges, &image, 140, 100);
        assert!(close(b.width(), 137.0 * s), "{b:?}");
        assert!(close(b.height(), 89.0 * s), "{b:?}");
        assert_eq!(
            ((b.width() / s).round(), (b.height() / s).round()),
            (137.0, 89.0)
        );
    }

    #[test]
    fn a_ray_with_no_edge_ends_at_the_border() {
        let image = boxed(50, 40, 100.0, 100.0, 101.0, 101.0);
        let edges = canny(&image);
        let b = measure_around(&edges, &image, 10, 10);
        assert_eq!(
            b,
            Bounds {
                left: 0.0,
                top: 0.0,
                right: 50.0,
                bottom: 40.0
            }
        );
    }

    #[test]
    fn a_thin_line_is_measured_to_its_near_side() {
        // A 1 px rule at x == 30: the span stops where the rule starts, as
        // seen from the cursor, whichever neighbour Canny marked.
        let image = RgbaImage::from_fn(60, 20, |x, _| {
            let v = if x == 30 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        });
        let edges = canny(&image);
        assert_eq!(measure_around(&edges, &image, 10, 10).right, 30.0);
        assert_eq!(measure_around(&edges, &image, 50, 10).left, 31.0);
    }

    #[test]
    fn a_bordered_box_measures_its_inside_whatever_the_colours() {
        // 40 x 40 with a 1 px border: 38 inside. Before, the reading depended
        // on the border and fill colours (34, 36 or 40).
        for (border, fill) in [(128, 255), (0, 225), (0, 200), (0, 255)] {
            let image = bordered(border, fill);
            let edges = canny(&image);
            let b = measure_around(&edges, &image, 40, 40);
            assert_eq!(
                b,
                Bounds {
                    left: 21.0,
                    top: 21.0,
                    right: 59.0,
                    bottom: 59.0
                },
                "{border}/{fill}"
            );
            // The container is the same inside; snapping from inside a
            // corner lands on it too.
            let r = region_bounds(&image, 21, 21, 38, 38);
            assert_eq!(r, b, "container {border}/{fill}");
            assert_eq!(
                snap_to_boundary(&edges, &image, 57.5, 57.5, 6, 1),
                Some((59.0, 59.0))
            );
            assert_eq!(
                snap_to_boundary(&edges, &image, 22.5, 22.5, 6, 1),
                Some((21.0, 21.0))
            );
            // From outside, snapping lands on the border's outer side.
            assert_eq!(
                snap_to_boundary(&edges, &image, 40.5, 16.5, 6, 1),
                Some((40.5, 20.0))
            );
        }
    }

    #[test]
    fn a_thin_border_at_a_fractional_scale_measures_its_inside() {
        // A 40 logical px box with a 1 logical px border at 1.5023x: 38
        // logical px inside, edges falling between device pixels.
        let s = 1.502_343_8_f32;
        let (x0, y0) = (20.3_f32, 20.7_f32);
        let image = crate::boundary::tests::bordered_fractional(
            100,
            x0,
            y0,
            x0 + 40.0 * s,
            y0 + 40.0 * s,
            s,
        );
        let edges = canny(&image);
        let b = measure_around(&edges, &image, 50, 50);
        // A border this thin never covers a whole pixel, so its full colour
        // is never seen and each side may land up to ~0.2 device px off; the
        // reading is still right to the logical pixel. (It was 37.28.)
        let near = |a: f32, b: f32| (a - b).abs() < 0.35;
        assert!(
            near(b.width(), 38.0 * s) && near(b.height(), 38.0 * s),
            "{b:?}"
        );
        assert_eq!(
            ((b.width() / s).round(), (b.height() / s).round()),
            (38.0, 38.0)
        );
    }

    #[test]
    fn shrinking_onto_text_stems_keeps_only_the_stems() {
        // Two 1 px stems at x = 30 and 34, rows 10..18: content is [30, 35).
        let image = RgbaImage::from_fn(60, 30, |x, y| {
            let v = if (x == 30 || x == 34) && (10..18).contains(&y) {
                0
            } else {
                255
            };
            image::Rgba([v, v, v, 255])
        });
        let edges = canny(&image);
        let rect = PixelRect {
            left: 20,
            top: 2,
            right: 45,
            bottom: 26,
        };
        let b = shrink_to_bounds(&edges, &image, rect);
        assert_eq!(
            b,
            Bounds {
                left: 30.0,
                top: 10.0,
                right: 35.0,
                bottom: 18.0
            }
        );
    }

    #[test]
    fn a_rect_too_small_to_shrink_is_left_as_given() {
        let image = boxed(100, 100, 10.0, 10.0, 90.0, 90.0);
        let edges = canny(&image);
        let tiny = PixelRect {
            left: 8,
            top: 8,
            right: 11,
            bottom: 11,
        };
        let b = shrink_to_bounds(&edges, &image, tiny);
        assert_eq!(
            b,
            Bounds {
                left: 8.0,
                top: 8.0,
                right: 11.0,
                bottom: 11.0
            }
        );
    }

    #[test]
    fn a_corner_snaps_onto_both_of_its_edges() {
        // Aiming just inside the bottom-right corner of a box must land on
        // the corner, not short of it on either axis.
        let image = boxed(320, 320, 60.0, 60.0, 260.0, 260.0);
        let edges = canny(&image);
        for band in [1, 2, 3] {
            let (x, y) = snap_to_boundary(&edges, &image, 257.5, 257.5, 15, band).unwrap();
            assert_eq!((x, y), (260.0, 260.0), "band {band}");
            let (x, y) = snap_to_boundary(&edges, &image, 62.5, 62.5, 15, band).unwrap();
            assert_eq!((x, y), (60.0, 60.0), "band {band}");
        }
    }

    #[test]
    fn a_corner_does_not_pull_the_cross_axis() {
        // Pixel-level: the bottom edge's run must not count as a vertical
        // edge for x, even when the band reaches it.
        let edges = EdgeMap::from_ascii(&[
            "......#...",
            "......#...",
            "......#...",
            "#######...",
            "..........",
        ]);
        // At (4, 2): x should snap to the vertical run at 6, y to row 3.
        assert_eq!(snap_to_edge(&edges, 4, 2, 4, 1), Some((6, 3)));
    }

    #[test]
    fn a_45_degree_edge_is_not_snapped_to() {
        let edges = EdgeMap::from_ascii(&[
            "#.........",
            ".#........",
            "..#.......",
            "...#......",
            "....#.....",
        ]);
        assert_eq!(snap_to_edge(&edges, 6, 2, 5, 2), None);
    }

    #[test]
    fn snapping_keeps_an_unsnapped_axis_exactly() {
        let image = boxed(100, 100, 40.0, 0.0, 100.0, 100.0);
        let edges = canny(&image);
        let (x, y) = snap_to_boundary(&edges, &image, 37.25, 50.75, 6, 1).unwrap();
        assert_eq!((x, y), (40.0, 50.75));
    }

    #[test]
    fn shrinking_lands_on_the_content_boundaries() {
        let s = 1.502_343_8_f32;
        let image = boxed(400, 300, 60.0, 50.0, 60.0 + 200.0 * s, 50.0 + 137.0 * s);
        let edges = canny(&image);
        let rect = PixelRect {
            left: 30,
            top: 20,
            right: 380,
            bottom: 290,
        };
        let b = shrink_to_bounds(&edges, &image, rect);
        assert!(close(b.left, 60.0) && close(b.top, 50.0), "{b:?}");
        assert!(
            close(b.width(), 200.0 * s) && close(b.height(), 137.0 * s),
            "{b:?}"
        );
    }

    #[test]
    fn shrinking_an_empty_rect_returns_it_as_given() {
        let image = boxed(100, 100, 500.0, 500.0, 501.0, 501.0);
        let edges = canny(&image);
        let rect = PixelRect {
            left: 10,
            top: 20,
            right: 60,
            bottom: 70,
        };
        let b = shrink_to_bounds(&edges, &image, rect);
        assert_eq!(
            b,
            Bounds {
                left: 10.0,
                top: 20.0,
                right: 60.0,
                bottom: 70.0
            }
        );
    }

    #[test]
    fn a_region_is_widened_to_its_boundaries() {
        // A box whose edge pixels sit inside it: the region between them is
        // narrower than the box, the boundaries are not.
        let image = boxed(200, 200, 50.0, 40.0, 150.0, 160.0);
        let b = region_bounds(&image, 51, 41, 98, 118);
        assert_eq!(
            b,
            Bounds {
                left: 50.0,
                top: 40.0,
                right: 150.0,
                bottom: 160.0
            }
        );
        // And when they sit outside it.
        let b = region_bounds(&image, 50, 40, 100, 120);
        assert_eq!(
            b,
            Bounds {
                left: 50.0,
                top: 40.0,
                right: 150.0,
                bottom: 160.0
            }
        );
    }
}
