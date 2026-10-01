//! Matching captured monitors to the window system's monitors.
//!
//! Each overlay window is made borderless-fullscreen on one specific output,
//! the only placement a Wayland client gets to choose. That needs the window
//! system's handle for the output a capture came from, and the two sides
//! only share what they report about it.

use ruler_core::geometry::MonitorGeometry;

/// What the window system reports about one of its monitors.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub name: Option<String>,
    /// Top-left corner in physical pixels.
    pub position: (i32, i32),
}

/// Index of the candidate that shows `captured`, if any.
///
/// By name first: Wayland and X11 capture read the same output names the
/// window system reports (`eDP-1`, `DP-2`), and a name survives the
/// logical-vs-physical disagreements positions are prone to under scaling.
/// By position otherwise, which is what is left on platforms whose capture
/// and windowing layers name monitors differently.
///
/// A name only identifies a monitor when it is unique: macOS names displays
/// by model, so two identical ones share a name. Those are told apart by
/// position, and if that fails too it is `None` rather than a guess, which
/// would put two overlays on the same screen.
///
/// The window system may also list one output more than once (winit does on
/// GNOME Wayland); entries that agree on name and position are that one
/// output, not namesakes.
pub fn pick(captured: &MonitorGeometry, candidates: &[Candidate]) -> Option<usize> {
    let at_position = |i: &usize| candidates[*i].position == captured.position;
    let named: Vec<usize> = (0..candidates.len())
        .filter(|&i| candidates[i].name.as_deref() == Some(captured.name.as_str()))
        .collect();
    let one_output = named
        .windows(2)
        .all(|pair| candidates[pair[0]].position == candidates[pair[1]].position);

    match named.as_slice() {
        [] => (0..candidates.len()).find(at_position),
        [first, ..] if one_output => Some(*first),
        several => several.iter().copied().find(at_position),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(name: &str, position: (i32, i32)) -> MonitorGeometry {
        MonitorGeometry {
            name: name.to_string(),
            position,
            size: (1920, 1080),
            scale: 1.0,
            is_primary: false,
        }
    }

    fn candidate(name: Option<&str>, position: (i32, i32)) -> Candidate {
        Candidate {
            name: name.map(str::to_string),
            position,
        }
    }

    #[test]
    fn monitors_are_matched_by_name() {
        let candidates = [
            candidate(Some("eDP-1"), (0, 0)),
            candidate(Some("DP-2"), (1920, 0)),
        ];
        assert_eq!(pick(&captured("DP-2", (1920, 0)), &candidates), Some(1));
    }

    #[test]
    fn a_name_match_wins_over_a_position_match() {
        // Positions can disagree under scaling (logical vs physical), names
        // cannot: here DP-2's reported position collides with eDP-1's.
        let candidates = [
            candidate(Some("eDP-1"), (0, 0)),
            candidate(Some("DP-2"), (3840, 0)),
        ];
        assert_eq!(pick(&captured("DP-2", (0, 0)), &candidates), Some(1));
    }

    #[test]
    fn unnamed_monitors_fall_back_to_position() {
        let candidates = [candidate(None, (0, 0)), candidate(None, (1920, 0))];
        assert_eq!(
            pick(&captured("Display 2", (1920, 0)), &candidates),
            Some(1)
        );
    }

    #[test]
    fn a_shared_name_is_settled_by_position() {
        // Two identical displays, as macOS names them.
        let candidates = [
            candidate(Some("DELL U2720Q"), (0, 0)),
            candidate(Some("Built-in Retina Display"), (0, 1440)),
            candidate(Some("DELL U2720Q"), (2560, 0)),
        ];
        assert_eq!(
            pick(&captured("DELL U2720Q", (2560, 0)), &candidates),
            Some(2)
        );
        assert_eq!(pick(&captured("DELL U2720Q", (0, 0)), &candidates), Some(0));
        // A position elsewhere must not fall back to a differently named
        // monitor, nor guess between the two namesakes.
        assert_eq!(pick(&captured("DELL U2720Q", (0, 1440)), &candidates), None);
    }

    #[test]
    fn an_output_listed_twice_is_still_matched_by_name() {
        // As winit reports three outputs on GNOME Wayland: each one twice,
        // at positions that are not in the capture's coordinates.
        let listed = [
            candidate(Some("DP-2"), (1920, 0)),
            candidate(Some("DP-4"), (0, 0)),
            candidate(Some("eDP-1"), (2016, 2160)),
        ];
        let candidates: Vec<_> = listed.iter().chain(&listed).cloned().collect();
        assert_eq!(pick(&captured("eDP-1", (1514, 1623)), &candidates), Some(2));
        assert_eq!(pick(&captured("DP-2", (2885, 0)), &candidates), Some(0));
    }

    #[test]
    fn nothing_matching_is_reported_as_none() {
        let candidates = [candidate(Some("eDP-1"), (0, 0))];
        assert_eq!(pick(&captured("DP-2", (1920, 0)), &candidates), None);
        assert_eq!(pick(&captured("DP-2", (1920, 0)), &[]), None);
    }
}
