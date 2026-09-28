//! Connected-component labelling of the free space between edges.
//!
//! Backs "container detection": the edge map is closed to seal one-pixel gaps
//! in a border, then every pocket of non-edge pixels becomes a labelled region
//! whose bounding box is the container under the cursor.

use crate::edges::EdgeMap;

/// 3x3 structuring element, matching the OpenCV `MORPH_CLOSE` this replaces.
const CLOSE_KERNEL_RADIUS: usize = 1;

/// A region that covers essentially the whole screen is the desktop backdrop,
/// not a UI container, so it is reported as unavailable.
const WHOLE_SCREEN_AREA_RATIO: f64 = 0.98;

/// Bounding box and pixel count of one labelled region, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RegionStats {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub area: usize,
}

/// Labelled free-space regions for one monitor's edge map.
pub struct RegionMap {
    width: u32,
    height: u32,
    /// Label per pixel; `0` means "barrier" (an edge pixel), regions start at 1.
    labels: Vec<u32>,
    /// Indexed by label; entry `0` is an unused placeholder for the barrier label.
    stats: Vec<RegionStats>,
}

impl RegionMap {
    /// Labels the free space of `edges`.
    pub fn build(edges: &EdgeMap) -> Self {
        let (w, h) = (edges.width() as usize, edges.height() as usize);
        if w == 0 || h == 0 {
            return Self {
                width: edges.width(),
                height: edges.height(),
                labels: Vec::new(),
                stats: vec![RegionStats::default()],
            };
        }

        let barriers = morphological_close(edges);
        let mut labels = vec![0u32; w * h];
        let mut stats = vec![RegionStats::default()];
        let mut stack: Vec<usize> = Vec::new();

        for seed in 0..w * h {
            if barriers[seed] || labels[seed] != 0 {
                continue;
            }

            let label = stats.len() as u32;
            let (mut min_x, mut max_x) = (seed % w, seed % w);
            let (mut min_y, mut max_y) = (seed / w, seed / w);
            let mut area = 0usize;

            labels[seed] = label;
            stack.push(seed);

            // Flood fill with 4-connectivity, mirroring the
            // `connectedComponentsWithStats(..., connectivity=4)` this replaces.
            while let Some(idx) = stack.pop() {
                let x = idx % w;
                let y = idx / w;
                area += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);

                let mut visit = |n: usize, stack: &mut Vec<usize>| {
                    if !barriers[n] && labels[n] == 0 {
                        labels[n] = label;
                        stack.push(n);
                    }
                };
                if x > 0 {
                    visit(idx - 1, &mut stack);
                }
                if x + 1 < w {
                    visit(idx + 1, &mut stack);
                }
                if y > 0 {
                    visit(idx - w, &mut stack);
                }
                if y + 1 < h {
                    visit(idx + w, &mut stack);
                }
            }

            stats.push(RegionStats {
                x: min_x as u32,
                y: min_y as u32,
                width: (max_x - min_x + 1) as u32,
                height: (max_y - min_y + 1) as u32,
                area,
            });
        }

        Self {
            width: edges.width(),
            height: edges.height(),
            labels,
            stats,
        }
    }

    /// Number of labelled regions.
    #[cfg(test)]
    pub fn region_count(&self) -> usize {
        self.stats.len().saturating_sub(1)
    }

    /// Label at `(x, y)`, or `0` for a barrier pixel / out-of-bounds read.
    fn label_at(&self, x: u32, y: u32) -> u32 {
        if x >= self.width || y >= self.height || self.labels.is_empty() {
            return 0;
        }
        self.labels[y as usize * self.width as usize + x as usize]
    }

    /// Finds the region under `(x, y)`, searching outward when the cursor sits
    /// exactly on a barrier pixel.
    ///
    /// Ties at equal distance resolve to the smaller region, which keeps the
    /// selection on the tight inner container rather than its parent.
    fn label_near(&self, x: u32, y: u32, max_radius: u32) -> u32 {
        let direct = self.label_at(x, y);
        if direct > 0 {
            return direct;
        }

        for radius in 1..=max_radius {
            let min_x = x.saturating_sub(radius);
            let max_x = x.saturating_add(radius).min(self.width.saturating_sub(1));
            let min_y = y.saturating_sub(radius);
            let max_y = y.saturating_add(radius).min(self.height.saturating_sub(1));

            let mut best_label = 0u32;
            let mut best_dist2 = u64::MAX;
            let mut best_area = usize::MAX;

            for ny in min_y..=max_y {
                for nx in min_x..=max_x {
                    let label = self.label_at(nx, ny);
                    if label == 0 {
                        continue;
                    }
                    let dx = nx.abs_diff(x) as u64;
                    let dy = ny.abs_diff(y) as u64;
                    let dist2 = dx * dx + dy * dy;
                    let area = self.stats[label as usize].area;
                    if dist2 < best_dist2 || (dist2 == best_dist2 && area < best_area) {
                        best_dist2 = dist2;
                        best_area = area;
                        best_label = label;
                    }
                }
            }

            if best_label > 0 {
                return best_label;
            }
        }

        0
    }

    /// Returns the bounding box of the container under `(x, y)` in device pixels.
    ///
    /// `None` means "no meaningful container here": either a barrier with no
    /// nearby region, a degenerate one-pixel sliver, or the full-screen backdrop.
    pub fn container_at(&self, x: u32, y: u32) -> Option<RegionStats> {
        let label = self.label_near(x, y, 3);
        if label == 0 {
            return None;
        }
        let stats = *self.stats.get(label as usize)?;
        if stats.width <= 1 || stats.height <= 1 {
            return None;
        }
        let screen_area = f64::from(self.width) * f64::from(self.height);
        if stats.area as f64 >= screen_area * WHOLE_SCREEN_AREA_RATIO {
            return None;
        }
        Some(stats)
    }
}

/// Dilate-then-erode with a 3x3 kernel, sealing single-pixel breaks in borders
/// so a leaky container does not merge with the whole desktop.
fn morphological_close(edges: &EdgeMap) -> Vec<bool> {
    let (w, h) = (edges.width() as usize, edges.height() as usize);
    let dilated = morph_pass(edges.as_slice(), w, h, true);
    morph_pass(&dilated, w, h, false)
}

/// One square-kernel morphology pass. `dilate` selects OR-of-neighbourhood,
/// otherwise AND-of-neighbourhood (erosion). Out-of-bounds neighbours are
/// treated as set, so erosion does not eat the image border.
///
/// A square structuring element is separable, so this runs a horizontal pass
/// then a vertical one: six neighbour reads per pixel rather than nine, over
/// buffers the size of the whole framebuffer.
fn morph_pass(src: &[bool], w: usize, h: usize, dilate: bool) -> Vec<bool> {
    let outside = !dilate;
    let combine = |a: bool, b: bool| if dilate { a | b } else { a & b };
    let r = CLOSE_KERNEL_RADIUS;

    let mut horizontal = vec![false; w * h];
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let mut acc = src[row + x];
            for offset in 1..=r {
                let left = if x >= offset {
                    src[row + x - offset]
                } else {
                    outside
                };
                let right = if x + offset < w {
                    src[row + x + offset]
                } else {
                    outside
                };
                acc = combine(combine(acc, left), right);
            }
            horizontal[row + x] = acc;
        }
    }

    let mut out = vec![false; w * h];
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let mut acc = horizontal[row + x];
            for offset in 1..=r {
                let up = if y >= offset {
                    horizontal[row - offset * w + x]
                } else {
                    outside
                };
                let down = if y + offset < h {
                    horizontal[row + offset * w + x]
                } else {
                    outside
                };
                acc = combine(combine(acc, up), down);
            }
            out[row + x] = acc;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 24;
    const H: u32 = 18;

    /// Draws a 1px rectangle border on a 24x18 map.
    ///
    /// The interior is deliberately larger than the 3x3 closing kernel: a
    /// container only a couple of pixels across is legitimately swallowed by
    /// the close, and real UI containers never are.
    fn boxed_map(gap: Option<(u32, u32)>) -> EdgeMap {
        let (left, top, right, bottom) = (4u32, 3u32, 19u32, 14u32);
        let w = W as usize;

        let mut data = vec![false; w * H as usize];
        let mut set = |x: u32, y: u32, v: bool| data[y as usize * w + x as usize] = v;
        for x in left..=right {
            set(x, top, true);
            set(x, bottom, true);
        }
        for y in top..=bottom {
            set(left, y, true);
            set(right, y, true);
        }
        if let Some((gx, gy)) = gap {
            set(gx, gy, false);
        }
        EdgeMap::new(W, H, data).expect("valid map")
    }

    #[test]
    fn labels_a_boxed_interior_as_its_own_region() {
        let regions = RegionMap::build(&boxed_map(None));

        // Outside backdrop plus the box interior.
        assert!(
            regions.region_count() >= 2,
            "got {}",
            regions.region_count()
        );

        let inner = regions
            .container_at(12, 8)
            .expect("interior is a container");
        assert_eq!(
            inner,
            RegionStats {
                x: 5,
                y: 4,
                width: 14,
                height: 10,
                area: 140,
            }
        );
    }

    #[test]
    fn closing_seals_a_one_pixel_gap_in_a_border() {
        // Punch a hole in the right wall. Without the morphological close the
        // interior would flood out into the backdrop and report the whole screen.
        let leaky = RegionMap::build(&boxed_map(Some((19, 8))));
        let inner = leaky.container_at(12, 8).expect("interior stays enclosed");

        let intact = RegionMap::build(&boxed_map(None));
        let reference = intact.container_at(12, 8).expect("interior is a container");
        assert_eq!(inner, reference, "one-pixel gap leaked through the close");
    }

    #[test]
    fn a_wide_gap_is_not_sealed_and_the_interior_merges_outward() {
        // Closing repairs hairline breaks, not real openings: a four-pixel
        // doorway must still connect the interior to the backdrop.
        let mut data = boxed_map(None).as_slice().to_vec();
        for y in 7..=10usize {
            data[y * W as usize + 19] = false;
        }
        let open = EdgeMap::new(W, H, data).expect("valid map");

        let regions = RegionMap::build(&open);
        let inner = regions.container_at(12, 8).expect("region exists");
        assert!(
            inner.width > 14,
            "interior should have merged outward, got {inner:?}"
        );
    }

    #[test]
    fn full_screen_backdrop_is_not_reported_as_a_container() {
        let empty = EdgeMap::from_ascii(&["....", "....", "....", "...."]);
        let regions = RegionMap::build(&empty);
        assert_eq!(regions.region_count(), 1);
        assert!(regions.container_at(1, 1).is_none());
    }

    #[test]
    fn cursor_on_a_barrier_snaps_to_the_nearest_region() {
        let regions = RegionMap::build(&boxed_map(None));
        // (12, 3) sits exactly on the top border, which belongs to no region.
        // It resolves outward, preferring the smaller of the two equidistant
        // neighbours so the tight container wins over the desktop backdrop.
        let resolved = regions
            .container_at(12, 3)
            .expect("barrier resolves outward");
        assert_eq!(resolved.y, 4, "expected the box interior, got {resolved:?}");
    }

    #[test]
    fn empty_edge_map_is_handled_without_panicking() {
        let regions = RegionMap::build(&EdgeMap::blank(0, 0));
        assert_eq!(regions.region_count(), 0);
        assert!(regions.container_at(0, 0).is_none());
    }

    #[test]
    fn extreme_coordinates_do_not_overflow_the_outward_search() {
        // The outward search adds the radius to the cursor position; an
        // unclamped u32 coordinate would wrap it.
        let regions = RegionMap::build(&boxed_map(None));
        assert!(regions.container_at(u32::MAX, u32::MAX).is_none());
        assert!(regions.container_at(u32::MAX, 8).is_none());
        assert!(regions.container_at(8, u32::MAX).is_none());
    }
}
