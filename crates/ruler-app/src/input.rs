//! Turning raw input into the units the state machine speaks.

use std::time::Duration;

/// Logical px of scroll per wheel notch: what Slint's winit backend reports
/// for one line of a mouse wheel.
///
/// Slint has no notion of a notch (unlike Qt's 120 per `angleDelta` step),
/// so this mirrors its winit backend, which turns a `LineDelta` into
/// `lines * 60.` (`winitwindowadapter.rs` in i-slint-backend-winit 1.18).
/// If that changes, so does the wheel's speed.
const NOTCH: f32 = 60.0;
/// Most notches one scroll event may apply, so a flung touchpad does not
/// slam a dial from one end to the other.
const MAX_NOTCHES_PER_EVENT: i32 = 5;

/// Pointer travel, logical px, above which a press-release is a drag rather
/// than a click.
pub const CLICK_SLOP: f32 = 4.0;

/// Accumulates scroll deltas into whole notches.
///
/// A mouse wheel sends one notch per event, but a touchpad sends a stream of
/// small pixel deltas; carrying the remainder over is what lets a touchpad
/// drive a dial at all instead of rounding every event to zero.
#[derive(Default)]
pub struct Wheel {
    pending: f32,
}

impl Wheel {
    /// Adds a scroll delta (positive = away from the user) and returns the
    /// whole notches it completes.
    pub fn feed(&mut self, delta: f32) -> i32 {
        if !delta.is_finite() {
            return 0;
        }
        self.pending += delta;
        let notches = (self.pending / NOTCH).trunc();
        self.pending -= notches * NOTCH;
        (notches as i32).clamp(-MAX_NOTCHES_PER_EVENT, MAX_NOTCHES_PER_EVENT)
    }
}

/// True when a press at `from` released at `to` counts as a click.
pub fn is_click(from: (f32, f32), to: (f32, f32)) -> bool {
    (to.0 - from.0).abs().max((to.1 - from.1).abs()) < CLICK_SLOP
}

/// How long the edge map stays fully shown after a sensitivity change.
const PREVIEW_HOLD: Duration = Duration::from_millis(1000);
/// How long it then takes to fade out.
const PREVIEW_FADE: Duration = Duration::from_millis(1000);
const PREVIEW_OPACITY: f32 = 0.5;
const DEBUG_OPACITY: f32 = 0.3;

/// Opacity of the edge overlay: constant under `--debug-edge-overlay`, and
/// a hold-then-fade flash for `since_change` after a new edge map lands, so
/// the user sees what the new sensitivity actually finds.
pub fn edges_opacity(debug: bool, since_change: Option<Duration>) -> f32 {
    let preview = since_change.map_or(0.0, |elapsed| {
        if elapsed <= PREVIEW_HOLD {
            PREVIEW_OPACITY
        } else {
            let fading = (elapsed - PREVIEW_HOLD).as_secs_f32() / PREVIEW_FADE.as_secs_f32();
            PREVIEW_OPACITY * (1.0 - fading).max(0.0)
        }
    });
    if debug {
        preview.max(DEBUG_OPACITY)
    } else {
        preview
    }
}

/// True once the preview has fully faded, so nothing needs redrawing.
pub fn preview_finished(since_change: Duration) -> bool {
    since_change >= PREVIEW_HOLD + PREVIEW_FADE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_wheel_line_is_one_notch() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.feed(60.0), 1);
        assert_eq!(wheel.feed(-120.0), -2);
    }

    #[test]
    fn small_touchpad_deltas_add_up() {
        let mut wheel = Wheel::default();
        let notches: i32 = (0..30).map(|_| wheel.feed(4.0)).sum();
        assert_eq!(notches, 2, "120 px of travel is two notches");
    }

    #[test]
    fn one_event_moves_a_dial_by_at_most_five() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.feed(6000.0), 5);
        assert_eq!(wheel.feed(f32::NAN), 0);
    }

    #[test]
    fn a_small_wobble_is_a_click_and_a_real_move_is_not() {
        assert!(is_click((100.0, 100.0), (102.0, 101.0)));
        assert!(!is_click((100.0, 100.0), (100.0, 110.0)));
    }

    #[test]
    fn the_preview_holds_then_fades_out() {
        let at = |ms| edges_opacity(false, Some(Duration::from_millis(ms)));
        assert_eq!(edges_opacity(false, None), 0.0);
        assert_eq!(at(0), 0.5);
        assert_eq!(at(1000), 0.5);
        assert!((at(1500) - 0.25).abs() < 1e-6);
        assert_eq!(at(2500), 0.0);
        assert!(preview_finished(Duration::from_millis(2000)));
    }

    #[test]
    fn the_debug_overlay_never_drops_below_its_floor() {
        assert_eq!(edges_opacity(true, None), 0.3);
        assert_eq!(edges_opacity(true, Some(Duration::ZERO)), 0.5);
    }
}
