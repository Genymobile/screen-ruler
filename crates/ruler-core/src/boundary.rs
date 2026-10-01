//! Where exactly an edge lies, to a fraction of a pixel.
//!
//! The edge map says *roughly* where an edge is: Canny marks one pixel per
//! transition, and which side of it it picks depends on rounding in the
//! gradient (seen on real captures: the outer pixel on one side of a box, the
//! inner one on the other, sometimes both). Measuring between edge pixels is
//! therefore off by one in either direction.
//!
//! So the edge map only locates the edge, and the image decides where it is:
//! a boundary is a position *between* pixels, in device px. A sharp step from
//! pixel `k - 1` to pixel `k` lies at `k`, which makes a box covering pixels
//! `a..b` exactly `b - a` wide (the first pixel inside on the top/left, the
//! first pixel outside on the bottom/right). An antialiased step, as drawn on
//! fractionally scaled outputs, lies at its coverage point: a 137 px box at
//! 1.5x is 205.8 device px wide, not 205 or 206.

use image::RgbaImage;

/// The direction a line of pixels runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Along a row: positions are x.
    Horizontal,
    /// Along a column: positions are y.
    Vertical,
}

/// A rectangle given by its four boundaries, in device px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Bounds {
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    pub fn height(self) -> f32 {
        self.bottom - self.top
    }
}

/// How far the colour either side of an edge must differ, on its most
/// changing channel, for its position to be worth estimating.
const MIN_CONTRAST: i32 = 24;

/// Locates the boundary of the colour step around the edge pixel at `at` on
/// `line` (a row for [`Axis::Horizontal`], a column for [`Axis::Vertical`]).
///
/// The colours two pixels either side of `at` are taken as the two sides,
/// and the three pixels between them are read as coverage of the far side:
/// the boundary is where that much coverage puts it. `None` when there is no
/// clean step to read: the two sides are alike (a thin line), they are not
/// flat (text, or a second edge right behind this one), or the window runs
/// off the image. Callers then fall back to [`first_change`].
pub fn locate(image: &RgbaImage, axis: Axis, line: u32, at: u32) -> Option<f32> {
    let (len, other) = match axis {
        Axis::Horizontal => (image.width(), image.height()),
        Axis::Vertical => (image.height(), image.width()),
    };
    if line >= other || at < 2 || at + 2 >= len {
        return None;
    }
    let pixel = |k: u32| {
        let (x, y) = match axis {
            Axis::Horizontal => (k, line),
            Axis::Vertical => (line, k),
        };
        let p = image.get_pixel(x, y).0;
        [i32::from(p[0]), i32::from(p[1]), i32::from(p[2])]
    };

    let near = pixel(at - 2);
    let far = pixel(at + 2);
    let channel = (0..3)
        .max_by_key(|&c| (far[c] - near[c]).abs())
        .unwrap_or(0);
    let contrast = far[channel] - near[channel];
    if contrast.abs() < MIN_CONTRAST {
        return None;
    }

    // Both sides must be flat, or "coverage" is measuring something else.
    let flat_tolerance = contrast.abs() / 4;
    let flat = |a: [i32; 3], b: [i32; 3]| (0..3).all(|c| (a[c] - b[c]).abs() <= flat_tolerance);
    if at >= 3 && !flat(pixel(at - 3), near) {
        return None;
    }
    if at + 3 < len && !flat(pixel(at + 3), far) {
        return None;
    }

    // And the pixels between must lie between them: a line of a third
    // colour along the boundary (a dark border between white and a light
    // fill) is not partial coverage of either side.
    let (low, high) = (
        near[channel].min(far[channel]) - flat_tolerance,
        near[channel].max(far[channel]) + flat_tolerance,
    );
    if (at - 1..=at + 1).any(|k| !(low..=high).contains(&pixel(k)[channel])) {
        return None;
    }

    let coverage: f32 = (at - 1..=at + 1)
        .map(|k| {
            let t = (pixel(k)[channel] - near[channel]) as f32 / contrast as f32;
            t.clamp(0.0, 1.0)
        })
        .sum();
    // The three pixels span [at - 1, at + 2); the far side covers `coverage`
    // of that, ending at at + 2.
    Some((at + 2) as f32 - coverage)
}

/// The least a pixel must differ from the colour a scan started on, on any
/// channel, to count as the start of something else.
const CHANGE: i32 = 16;

/// Scans `line` from `from` towards `to` (either direction, both inclusive)
/// and returns the boundary in front of the first thing that differs from the
/// pixel at `from`: the near side of whatever the scan ran into. `None` when
/// nothing along the way differs.
///
/// This is where an edge lies when there is no clean step for [`locate`] to
/// read: a 1 px border, a text stem. Canny marks such features on their
/// neighbours, either side, so the edge pixel itself is no guide to which
/// side of it they start; the image is, seen from where the measurement
/// approaches.
///
/// "Differs" is judged against what the scan runs into, at half its
/// strongest difference, so noise on the way (a wallpaper, a dithered
/// gradient) does not stop it early. And the feature's first pixel may be
/// only partly covered, as on a fractionally scaled output: its difference
/// over the feature's peak just beyond it is read as coverage, which puts the
/// boundary inside that pixel rather than on its far side. (A feature under
/// two pixels thick never shows its full colour, so its peak understates it
/// and the boundary can land up to a fifth of a pixel off: still well within
/// half a logical px.)
pub fn first_change(image: &RgbaImage, axis: Axis, line: u32, from: u32, to: u32) -> Option<f32> {
    let (len, other) = match axis {
        Axis::Horizontal => (image.width(), image.height()),
        Axis::Vertical => (image.height(), image.width()),
    };
    if line >= other || from >= len {
        return None;
    }
    let to = to.min(len - 1);
    let pixel = |k: u32| match axis {
        Axis::Horizontal => image.get_pixel(k, line).0,
        Axis::Vertical => image.get_pixel(line, k).0,
    };
    let start = pixel(from);
    let difference = |k: u32| {
        let p = pixel(k);
        (0..3)
            .map(|c| (i32::from(p[c]) - i32::from(start[c])).abs())
            .max()
            .unwrap_or(0)
    };
    let forward = to >= from;
    let steps: Vec<u32> = if forward {
        (from + 1..=to).collect()
    } else {
        (to..from).rev().collect()
    };
    let strongest = steps.iter().map(|&k| difference(k)).max()?;
    let threshold = CHANGE.max(strongest / 2);
    let mut first = steps.iter().position(|&k| difference(k) > threshold)?;
    let peak = steps[first..]
        .iter()
        .take(3)
        .map(|&k| difference(k))
        .max()
        .unwrap_or(strongest);
    // A partly covered pixel in front of the one past the threshold belongs
    // to the feature too: it stands clear of the noise floor and of the
    // start colour by a real fraction of the peak.
    if first > 0 {
        let before = difference(steps[first - 1]);
        if before > CHANGE && before * 5 > peak {
            first -= 1;
        }
    }
    let k = steps[first];
    let coverage = (difference(k) as f32 / peak.max(1) as f32).clamp(0.0, 1.0);
    // Covered from its far side: going forward the feature fills the end
    // of pixel k, going backward its start.
    Some(if forward {
        k as f32 + 1.0 - coverage
    } else {
        k as f32 + coverage
    })
}

/// The boundary of the edge at `at` on `line`, approached from `from` (the
/// cursor, or the side of a rectangle closing in): [`locate`] where the step
/// is clean, otherwise the near side of the first thing the approach runs
/// into, otherwise the near side of the edge pixel itself.
pub fn approach(image: &RgbaImage, axis: Axis, line: u32, from: u32, at: u32) -> f32 {
    if let Some(boundary) = locate(image, axis, line, at) {
        return boundary;
    }
    let forward = || first_change(image, axis, line, from.max(at.saturating_sub(3)), at + 2);
    let backward = || first_change(image, axis, line, from.min(at + 3), at.saturating_sub(2));
    let found = match from.cmp(&at) {
        std::cmp::Ordering::Less => forward(),
        std::cmp::Ordering::Greater => backward(),
        // Starting on the edge pixel itself, there is no telling which way
        // the approach runs: take whichever change is nearer.
        std::cmp::Ordering::Equal => match (forward(), backward()) {
            (Some(f), Some(b)) => Some(if f - at as f32 <= (at + 1) as f32 - b {
                f
            } else {
                b
            }),
            (f, b) => f.or(b),
        },
    };
    found.unwrap_or(if from <= at {
        at as f32
    } else {
        (at + 1) as f32
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::Rgba;

    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
    const BLACK: Rgba<u8> = Rgba([0, 0, 0, 255]);

    /// `width` x `height` white, with a black box covering `[x0, x1) x [y0,
    /// y1)` in continuous coordinates, antialiased by area coverage, as a
    /// compositor draws a box whose edges fall between device pixels.
    pub(crate) fn boxed(width: u32, height: u32, x0: f32, y0: f32, x1: f32, y1: f32) -> RgbaImage {
        let overlap = |p: u32, a: f32, b: f32| ((p + 1) as f32).min(b) - (p as f32).max(a);
        RgbaImage::from_fn(width, height, |x, y| {
            let cover = overlap(x, x0, x1).max(0.0) * overlap(y, y0, y1).max(0.0);
            let v = (255.0 * (1.0 - cover)).round() as u8;
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn a_sharp_step_lies_between_its_two_pixels() {
        let image = boxed(40, 5, 10.0, 0.0, 30.0, 5.0);
        // Whichever of the two pixels Canny marked, the answer is the same.
        for at in [9, 10] {
            assert_eq!(locate(&image, Axis::Horizontal, 2, at), Some(10.0));
        }
        for at in [29, 30] {
            assert_eq!(locate(&image, Axis::Horizontal, 2, at), Some(30.0));
        }
    }

    #[test]
    fn an_antialiased_step_lies_at_its_coverage_point() {
        let image = boxed(40, 40, 10.3, 5.0, 25.8, 35.0);
        let left = locate(&image, Axis::Horizontal, 20, 10).unwrap();
        let right = locate(&image, Axis::Horizontal, 20, 25).unwrap();
        assert!((left - 10.3).abs() < 0.01, "{left}");
        assert!((right - 25.8).abs() < 0.01, "{right}");
        assert!((right - left - 15.5).abs() < 0.02);
    }

    #[test]
    fn columns_work_like_rows() {
        let image = boxed(5, 40, 0.0, 7.0, 5.0, 21.5);
        assert_eq!(locate(&image, Axis::Vertical, 2, 7), Some(7.0));
        let bottom = locate(&image, Axis::Vertical, 2, 21).unwrap();
        assert!((bottom - 21.5).abs() < 0.01, "{bottom}");
    }

    #[test]
    fn a_thin_line_has_no_step_to_read() {
        let image = RgbaImage::from_fn(20, 3, |x, _| if x == 10 { BLACK } else { WHITE });
        assert_eq!(locate(&image, Axis::Horizontal, 1, 10), None);
    }

    #[test]
    fn a_second_edge_right_behind_is_not_read_as_coverage() {
        // Black from 10, then grey from 13: the far side is not flat.
        let image = RgbaImage::from_fn(20, 3, |x, _| match x {
            0..=9 => WHITE,
            10..=12 => BLACK,
            _ => Rgba([128, 128, 128, 255]),
        });
        assert_eq!(locate(&image, Axis::Horizontal, 1, 10), None);
    }

    #[test]
    fn low_contrast_and_the_image_border_fall_back() {
        let faint = RgbaImage::from_fn(20, 3, |x, _| {
            if x < 10 {
                Rgba([100, 100, 100, 255])
            } else {
                Rgba([110, 110, 110, 255])
            }
        });
        assert_eq!(locate(&faint, Axis::Horizontal, 1, 10), None);
        let image = boxed(20, 3, 1.0, 0.0, 19.0, 3.0);
        assert_eq!(locate(&image, Axis::Horizontal, 1, 1), None);
        assert_eq!(locate(&image, Axis::Horizontal, 1, 18), None);
        assert_eq!(locate(&image, Axis::Horizontal, 5, 10), None);
    }

    #[test]
    fn hue_counts_as_much_as_brightness() {
        // Equal luma, different hue: still a step.
        let image = RgbaImage::from_fn(20, 3, |x, _| {
            if x < 10 {
                Rgba([200, 50, 50, 255])
            } else {
                Rgba([50, 120, 80, 255])
            }
        });
        assert_eq!(locate(&image, Axis::Horizontal, 1, 10), Some(10.0));
    }

    /// White, with a 40x40 box at (20, 20) drawn as a 1 px `border` around a
    /// `fill` interior: the commonest UI shape there is.
    pub(crate) fn bordered(border: u8, fill: u8) -> RgbaImage {
        RgbaImage::from_fn(80, 80, |x, y| {
            let inside = (20..60).contains(&x) && (20..60).contains(&y);
            let interior = (21..59).contains(&x) && (21..59).contains(&y);
            let v = if interior {
                fill
            } else if inside {
                border
            } else {
                255
            };
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn a_border_between_two_colours_is_not_coverage() {
        // Dark border between white and a light fill: no clean step.
        let image = bordered(0, 225);
        for at in 18..=22 {
            assert_eq!(locate(&image, Axis::Horizontal, 40, at), None, "at {at}");
        }
    }

    #[test]
    fn a_scan_stops_in_front_of_the_first_change() {
        let image = bordered(128, 255);
        // From inside, leftwards: the border pixel is 20, so the interior
        // ends at 21. From outside, rightwards: the border starts at 20.
        assert_eq!(
            first_change(&image, Axis::Horizontal, 40, 30, 18),
            Some(21.0)
        );
        assert_eq!(
            first_change(&image, Axis::Horizontal, 40, 5, 22),
            Some(20.0)
        );
        assert_eq!(first_change(&image, Axis::Vertical, 40, 30, 18), Some(21.0));
        // Nothing within reach.
        assert_eq!(first_change(&image, Axis::Horizontal, 40, 30, 25), None);
    }

    #[test]
    fn approaching_a_border_lands_on_its_near_side_from_either_side() {
        // Canny marks a 1 px border on its neighbours (19 and 21 here), and
        // either one must give the same answer for the same approach.
        for (border, fill) in [(128, 255), (0, 225), (0, 200), (0, 255)] {
            let image = bordered(border, fill);
            for at in [19, 21] {
                assert_eq!(
                    approach(&image, Axis::Horizontal, 40, 40, at),
                    21.0,
                    "{border}/{fill} inside, at {at}"
                );
                assert_eq!(
                    approach(&image, Axis::Horizontal, 40, 5, at),
                    20.0,
                    "{border}/{fill} outside, at {at}"
                );
            }
        }
    }

    /// White, with a box `[x0, x1) x [y0, y1)` drawn as a black border of
    /// `thickness` around a white interior, antialiased by coverage: a 1
    /// logical px border on a fractionally scaled output.
    pub(crate) fn bordered_fractional(
        size: u32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
    ) -> RgbaImage {
        let overlap =
            |p: u32, a: f32, b: f32| (((p + 1) as f32).min(b) - (p as f32).max(a)).max(0.0);
        RgbaImage::from_fn(size, size, |x, y| {
            let outer = overlap(x, x0, x1) * overlap(y, y0, y1);
            let inner = overlap(x, x0 + thickness, x1 - thickness)
                * overlap(y, y0 + thickness, y1 - thickness);
            let v = (255.0 * (1.0 - (outer - inner))).round() as u8;
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn a_scan_reads_the_coverage_of_a_partly_covered_pixel() {
        // A 1.5 px border from 30.05 to 31.55: from inside, leftwards, the
        // interior ends at 31.55, inside pixel 31.
        let image = bordered_fractional(120, 30.05, 30.05, 90.0, 90.0, 1.5);
        let b = first_change(&image, Axis::Horizontal, 60, 45, 28).unwrap();
        assert!((b - 31.55).abs() < 0.05, "{b}");
        // From outside, rightwards: the border starts at 30.05.
        let b = first_change(&image, Axis::Horizontal, 60, 20, 33).unwrap();
        assert!((b - 30.05).abs() < 0.05, "{b}");
    }

    #[test]
    fn noise_on_the_way_does_not_stop_a_scan() {
        // A dark 1 px line at x == 40 on a background with +-12 of noise.
        let image = RgbaImage::from_fn(80, 40, |x, y| {
            let noise = ((x * 7919 + y * 104_729) % 25) as i32 - 12;
            let v = if x == 40 { 60 } else { 200 + noise };
            Rgba([v as u8, v as u8, v as u8, 255])
        });
        for row in 0..40 {
            assert_eq!(
                first_change(&image, Axis::Horizontal, row, 37, 42),
                Some(40.0),
                "row {row}"
            );
            assert_eq!(
                first_change(&image, Axis::Horizontal, row, 43, 38),
                Some(41.0),
                "row {row}"
            );
        }
    }
}
