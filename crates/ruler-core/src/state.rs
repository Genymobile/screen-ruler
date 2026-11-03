//! Interaction state and the rules that govern it.
//!
//! Deliberately free of any windowing or drawing types: the UI layer reads this
//! and renders it, and side effects the shell must perform (quit, clipboard)
//! leave through [`RulerState::take_commands`]. That keeps the behaviour that
//! is easy to get wrong — mode switching, undo/redo, the hold-to-confirm
//! discard — testable without a display.

use std::time::{Duration, Instant};

use crate::color::Sample;
use crate::geometry::{Point, Rect};

/// How long a discard's key or button must be held to confirm it.
const HOLD_TO_CONFIRM: Duration = Duration::from_millis(1000);
/// How long the discard prompt stays up once nothing holds it, so it can be
/// read.
const PROMPT_DURATION: Duration = Duration::from_millis(1500);
/// Close requests carry no key release: a hold by close requests lasts while
/// they keep coming, and ends when none has come for this long. Where a held
/// Alt+F4 repeats (GNOME's Mutter sends a single close instead), it does so
/// once its repeat delay has passed, every 30–40 ms by default and every
/// 400 ms or so at the slowest Windows setting, which still keeps the hold
/// going. A single close, or a burst from a dash Quit, stops at once.
///
/// The first close lapses before the repeats start, as the usual repeat delay
/// (500–660 ms) is longer than this gap: a held Alt+F4 fills the bar from its
/// first repeat, and so takes about the repeat delay plus a second.
const CLOSE_HOLD_GAP: Duration = Duration::from_millis(450);
/// How long transient session feedback stays on screen.
const FEEDBACK_DURATION: Duration = Duration::from_millis(1200);
/// How long the help overlay lingers at start-up before fading.
pub const HELP_AUTO_HIDE: Duration = Duration::from_millis(2000);
/// Fade duration for the help overlay.
pub const HELP_FADE: Duration = Duration::from_millis(220);
/// How long the edge map is flashed after a sensitivity change.
pub const EDGE_PREVIEW: Duration = Duration::from_millis(1000);

/// Slider and wheel adjustment bounds.
pub const SNAP_DISTANCE_MAX: f32 = 30.0;
pub const COLOR_RADIUS_MAX: f32 = 24.0;
pub const SENSITIVITY_MAX: f32 = 100.0;
/// Default edge-snap radius in logical pixels.
pub const DEFAULT_SNAP_DISTANCE: f32 = 10.0;

/// Below this delta a line counts as near-horizontal or near-vertical, and both
/// the distance summary and the distance overlay drop their per-axis breakdown.
pub const DELTA_BREAKDOWN_THRESHOLD: f32 = 8.0;

/// A dial a mode puts on the controls panel.
///
/// A mode declares an ordered list of these. The panel shows all of them, and
/// the wheel drives the first. Modes that snap need two — the snap radius and
/// the sensitivity that decides whether there is an edge there to snap to — and
/// an earlier one-dial-per-mode model could not say that, which left point
/// distance snapping against an edge map it had no way to tune.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Sensitivity,
    SnapDistance,
    ColorRadius,
}

impl Control {
    /// Label shown to the left of the slider.
    pub fn title(self) -> &'static str {
        match self {
            Control::Sensitivity => "Sensitivity",
            Control::SnapDistance => "Snap distance",
            Control::ColorRadius => "Average",
        }
    }

    pub fn max(self) -> f32 {
        match self {
            Control::Sensitivity => SENSITIVITY_MAX,
            Control::SnapDistance => SNAP_DISTANCE_MAX,
            Control::ColorRadius => COLOR_RADIUS_MAX,
        }
    }

    /// True when the read-out also reports how many edges the threshold found.
    pub fn reports_edges(self) -> bool {
        matches!(self, Control::Sensitivity)
    }
}

/// Declares the measurement modes and everything that varies between them.
///
/// One table, one source of truth. The enum, the `1`..`6` shortcut digits, the
/// panel sliders, the wheel target and the snapping rule all come from it, so
/// they cannot disagree. They previously did: the same mapping was written out
/// as four separate `match` arms across three files, and two of them had
/// drifted — shrink-to-fit offered a snap slider that never applied, and point
/// distance snapped using a value the panel would not show.
///
/// Only facts that genuinely vary per mode belong here. Whether a mode snaps is
/// *not* one of them: it is exactly "does this mode offer the snap dial", so it
/// is derived below rather than declared. A column that restates another column
/// is a column that can contradict it.
///
/// Deliberately not in this table either: the button icons. They belong to
/// the UI layer (`ruler-app`), because naming them here would drag drawing
/// code into a module meant to stay free of it. An exhaustive `match` there
/// keeps them complete.
macro_rules! modes {
    (@one $_variant:ident) => { 1usize };

    ($(
        $(#[$meta:meta])*
        $variant:ident => controls [$($control:ident),+], rect $rect:literal,
                          label $label:literal, export $export:literal, hint $hint:literal;
    )+) => {
        /// The measurement modes.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Mode {
            $($(#[$meta])* $variant,)+
        }

        /// How many modes there are, and therefore how many number keys bind.
        pub const MODE_COUNT: usize = 0 $(+ modes!(@one $variant))+;

        /// The most dials any one mode declares, and so the most slider rows the
        /// panel can ever show. Derived from the table so the panel can size
        /// itself without a heap allocation and without a number to keep in step.
        pub const MAX_CONTROLS: usize = {
            let mut max = 0;
            $(
                let count = 0 $(+ modes!(@one $control))+;
                if count > max {
                    max = count;
                }
            )+
            max
        };

        impl Mode {
            pub const ALL: [Mode; MODE_COUNT] = [$(Mode::$variant),+];

            /// Zero-based index, matching the `1`..`6` number-key shortcuts.
            pub fn index(self) -> usize {
                self as usize
            }

            /// Inverse of [`Mode::index`].
            pub fn from_index(index: usize) -> Option<Mode> {
                Mode::ALL.get(index).copied()
            }

            /// The mode selected by a `1`-based number key, if that key binds.
            pub fn from_digit(digit: usize) -> Option<Mode> {
                Mode::from_index(digit.checked_sub(1)?)
            }

            /// The number key that selects this mode, as shown in its tooltip.
            pub fn digit(self) -> usize {
                self.index() + 1
            }

            /// Short name, shown on the button tooltip and in the CLI help.
            pub fn label(self) -> &'static str {
                match self { $(Mode::$variant => $label,)+ }
            }

            /// One-line description of what the mode measures, for `--help`.
            pub fn hint(self) -> &'static str {
                match self { $(Mode::$variant => $hint,)+ }
            }

            /// Heading used for this mode in the Markdown export.
            fn export_title(self) -> &'static str {
                match self { $(Mode::$variant => $export,)+ }
            }

            /// The dials this mode puts on the panel, in the order shown.
            pub fn controls(self) -> &'static [Control] {
                match self { $(Mode::$variant => &[$(Control::$control),+],)+ }
            }

            /// True for the modes driven by dragging a rectangle out by hand.
            pub fn is_rect_selection(self) -> bool {
                match self { $(Mode::$variant => $rect,)+ }
            }
        }
    };
}

// The `controls` column is aligned on the first line of each entry on purpose:
// reading straight down it is how you check that every mode offers the dials
// its behaviour actually depends on, which is the check two bugs failed.
//
// Every snapping mode lists SnapDistance first — the dial the wheel drives, and
// the one being adjusted most often while placing geometry — then Sensitivity,
// because a snap radius is useless over an edge map too coarse to find the edge.
//
// Shrink-to-fit deliberately does not snap: the shrink itself finds the content
// edges, so the drag only has to enclose the content roughly, and a corner
// pulled onto an edge inside it would start the shrink from a rect that already
// cuts into what it should keep. Its dial is the sensitivity, which is what
// decides what the shrink finds.
modes! {
    /// Cast rays outward from the cursor to the nearest edges.
    Crosshair   => controls [Sensitivity], rect false,
                   label "Crosshair", export "Crosshair",
                   hint "Measure to the nearest edges around the cursor";

    /// Drag a freehand rectangle.
    RectDrag    => controls [SnapDistance, Sensitivity], rect true,
                   label "Drag rectangle", export "Rectangle",
                   hint "Drag a rectangle, snapping to edges";

    /// Detect the enclosing container under the cursor.
    Container   => controls [Sensitivity], rect false,
                   label "Container detection", export "Rectangle",
                   hint "Detect the enclosing UI container";

    /// Drag a rectangle, then tighten it onto the content it encloses.
    ShrinkToFit => controls [Sensitivity], rect true,
                   label "Shrink-to-fit", export "Rectangle",
                   hint "Drag, then tighten onto the content inside";

    /// Sample a colour under the cursor.
    ColorPicker => controls [ColorRadius], rect false,
                   label "Color picker", export "Color",
                   hint "Sample a colour, optionally averaged";

    /// Measure between two placed points.
    Distance    => controls [SnapDistance, Sensitivity], rect false,
                   label "Point distance", export "Distance",
                   hint "Measure between two points";
}

/// Facts derived from the mode table rather than declared in it.
impl Mode {
    /// The dial the wheel drives: the first one the mode declares.
    pub fn primary_control(self) -> Control {
        // The macro requires at least one control per mode, so this cannot panic.
        self.controls()[0]
    }

    /// True when the cursor is pulled onto nearby edges in this mode.
    ///
    /// Snapping *is* offering the snap-distance dial, so this reads the table
    /// rather than repeating it: a mode cannot end up snapping with no way to
    /// tune it, or offering a snap radius that never applies.
    pub fn snaps(self) -> bool {
        self.controls().contains(&Control::SnapDistance)
    }

    /// True for the modes that have nothing to show until the edge map is
    /// ready. The others work without it: the colour picker reads the
    /// screenshot, and dragging or placing points just does not snap yet.
    pub fn needs_edges(self) -> bool {
        matches!(self, Mode::Crosshair | Mode::Container)
    }
}

/// What a placed annotation records.
#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationKind {
    Crosshair {
        /// Ray endpoints in logical pixels, monitor-local.
        north: f32,
        south: f32,
        west: f32,
        east: f32,
    },
    Rect {
        width: f32,
        height: f32,
    },
    Color {
        sample: Sample,
        radius: f32,
    },
    Distance {
        to_x: f32,
        to_y: f32,
    },
}

/// One persistent annotation placed in session mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// Anchor point: the cursor for crosshair/colour, the top-left for a
    /// rectangle, point A for a distance.
    pub at: Point,
    pub mode: Mode,
    pub kind: AnnotationKind,
}

#[cfg(test)]
impl Annotation {
    /// A placed rectangle, the simplest annotation to stand in for "some work
    /// has been done".
    pub fn test_rect(x: f32, y: f32, width: f32, height: f32) -> Annotation {
        Annotation {
            at: Point { monitor: 0, x, y },
            mode: Mode::RectDrag,
            kind: AnnotationKind::Rect { width, height },
        }
    }
}

impl Annotation {
    /// The measurement string shown next to the annotation and copied out.
    pub fn measurement_text(&self) -> String {
        match &self.kind {
            AnnotationKind::Crosshair {
                north,
                south,
                west,
                east,
            } => format_size(east - west, south - north),
            AnnotationKind::Rect { width, height } => format_size(*width, *height),
            AnnotationKind::Color { sample, .. } => sample.summary(),
            AnnotationKind::Distance { to_x, to_y } => {
                format_distance_summary(self.at.x, self.at.y, *to_x, *to_y)
            }
        }
    }
}

/// Formats a width/height pair the way the measurement label shows it.
pub fn format_size(width: f32, height: f32) -> String {
    format!(
        "{} \u{00D7} {} px",
        width.abs().round(),
        height.abs().round()
    )
}

/// Formats a scalar pixel distance with at most one decimal place.
pub fn format_distance(value: f32) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < f32::EPSILON {
        format!("{} px", rounded.round())
    } else {
        format!("{rounded:.1} px")
    }
}

/// Formats the full A-to-B summary, adding a per-axis breakdown only when the
/// line is meaningfully diagonal.
/// Whether a distance is diagonal enough to be worth breaking down into
/// Δx and Δy. Judged on the rounded deltas, the ones the breakdown prints, so
/// the overlay's legs and the copied summary always agree.
pub fn shows_delta_breakdown(dx: f32, dy: f32) -> bool {
    dx.abs().round().min(dy.abs().round()) > DELTA_BREAKDOWN_THRESHOLD
}

pub fn format_distance_summary(ax: f32, ay: f32, bx: f32, by: f32) -> String {
    let dx = bx - ax;
    let dy = by - ay;
    let distance = (dx * dx + dy * dy).sqrt();
    let mut summary = format!(
        "A({}, {}) \u{2192} B({}, {}) \u{2014} {}",
        ax.round(),
        ay.round(),
        bx.round(),
        by.round(),
        format_distance(distance)
    );
    if shows_delta_breakdown(dx, dy) {
        summary.push_str(&format!(
            " (\u{0394}x={}, \u{0394}y={})",
            dx.abs().round(),
            dy.abs().round()
        ));
    }
    summary
}

/// An action that discards work, and so is held to confirm it when there
/// are annotations to lose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destructive {
    /// Leave session mode via Tab.
    ToggleSession,
    /// Leave session mode via the SESSION badge.
    SessionButton,
    /// Escape.
    Escape,
    /// Q.
    Quit,
    /// A close request from the window manager, such as Alt+F4.
    Close,
}

impl Destructive {
    /// What to do to confirm the action, shown while it is held and for a
    /// while after a hold that ended too early.
    fn prompt(self) -> &'static str {
        match self {
            Destructive::ToggleSession => "Hold Tab to discard annotations",
            Destructive::SessionButton => "Hold SESSION to discard annotations",
            Destructive::Escape => "Hold Esc to discard annotations",
            Destructive::Quit => "Hold Q to discard annotations and quit",
            // A close request comes from the window manager, through keys or
            // a button the app cannot know, and a held close may arrive as a
            // single request, so the prompt points to Q, which can be held.
            Destructive::Close => "Hold Q to discard annotations and quit",
        }
    }
}

/// A destructive action being held, which confirms once held for
/// [`HOLD_TO_CONFIRM`].
#[derive(Clone, Copy, Debug)]
struct Hold {
    action: Destructive,
    /// When the press that started the hold came.
    since: Instant,
    /// When the latest close request came. Only a [`Destructive::Close`]
    /// hold reads it: see [`CLOSE_HOLD_GAP`].
    last_close: Instant,
    /// Whether a frame has already found the hold full. It confirms on the
    /// frame after, so that a release queued behind that frame's timer is
    /// handled first.
    seen_full: bool,
}

impl Hold {
    /// How long the hold has lasted by `now`. A close hold counts only up to
    /// its latest close request: with no release to end it, the user may have
    /// let go any time since.
    fn held(&self, now: Instant) -> Duration {
        let held_until = match self.action {
            Destructive::Close => self.last_close,
            _ => now,
        };
        held_until.saturating_duration_since(self.since)
    }

    fn is_full(&self, now: Instant) -> bool {
        self.held(now) >= HOLD_TO_CONFIRM
    }
}

/// A side effect for the shell to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Quit,
    /// Copy text, then quit. Used by the "measure and go" quick flow.
    CopyTextAndQuit(String),
    /// Copy text and stay open. Used by session-mode Markdown export.
    CopyText(String),
    /// Composite the given region of a monitor with its annotations and copy it.
    CopyRegionImage(Rect),
    /// Edge thresholds changed; re-analyse every monitor.
    Recompute,
}

/// A value that stops being reported once its deadline passes.
///
/// Both the discard prompt a hold leaves up and the transient feedback message
/// are "show this until it times out", so they share one implementation rather
/// than each hand-rolling the same expiry check.
struct Expiring<T>(Option<(T, Instant)>);

impl<T> Expiring<T> {
    fn new() -> Self {
        Self(None)
    }

    fn set(&mut self, value: T, lifetime: Duration) {
        self.0 = Some((value, Instant::now() + lifetime));
    }

    fn clear(&mut self) {
        self.0 = None;
    }

    /// The value, if one is set and unexpired. Drops it once it has expired.
    fn get(&mut self) -> Option<&T> {
        if matches!(&self.0, Some((_, expiry)) if Instant::now() >= *expiry) {
            self.0 = None;
        }
        self.0.as_ref().map(|(value, _)| value)
    }
}

/// In-progress rectangle drag.
#[derive(Clone, Copy, Debug, Default)]
pub struct DragState {
    pub active: bool,
    pub has_selection: bool,
    pub monitor: usize,
    pub start: (f32, f32),
    pub end: (f32, f32),
    /// Raw cursor position when the button went down, before snapping.
    ///
    /// Kept apart from `start` so that "did the user drag or just click?" is
    /// answered by where the cursor actually went. Measuring it from the
    /// snapped corners instead makes a real drag that begins and ends inside
    /// one edge's snap radius collapse to zero length and be thrown away.
    pub press: (f32, f32),
}

impl DragState {
    /// True when the cursor travelled far enough for this to count as a drag
    /// rather than a click.
    ///
    /// Reads `press` — the raw cursor at button-down — deliberately. Answering
    /// this from `start`/`end` reads the *snapped* corners, and a real drag that
    /// begins and ends within one edge's snap radius has identical corners, so
    /// it would be thrown away as a click.
    pub fn is_drag(&self, pointer: (f32, f32), threshold: f32) -> bool {
        let moved = (pointer.0 - self.press.0)
            .abs()
            .max((pointer.1 - self.press.1).abs());
        moved >= threshold
    }

    /// The normalised selection, or `None` when nothing is selected.
    pub fn rect(&self) -> Option<Rect> {
        if !self.has_selection && !self.active {
            return None;
        }
        Some(Rect {
            monitor: self.monitor,
            x: self.start.0.min(self.end.0),
            y: self.start.1.min(self.end.1),
            width: (self.end.0 - self.start.0).abs(),
            height: (self.end.1 - self.start.1).abs(),
        })
    }

    fn clear(&mut self) {
        self.active = false;
        self.has_selection = false;
    }
}

/// The whole application's interaction state.
pub struct RulerState {
    pub mode: Mode,
    pub sensitivity: f32,
    pub snap_distance: f32,
    pub color_radius: f32,

    pub session: bool,
    pub annotations: Vec<Annotation>,
    redo_stack: Vec<Annotation>,

    /// Cursor position, absent until the pointer first enters a window.
    pub pointer: Option<Point>,
    /// Monitor that owns the controls panel.
    ///
    /// Follows the cursor, but starts on the first monitor so the panel and the
    /// shortcut overlay are visible immediately, before the mouse has moved and
    /// told anyone where it is.
    pub active_monitor: usize,
    /// Cursor pulled onto a nearby edge, when one was in range.
    pub snapped: Option<Point>,

    pub drag: DragState,
    /// Container under the cursor, in the container mode.
    pub container: Option<Rect>,
    /// Colour under the cursor, in the eyedropper mode.
    pub sample: Option<(Point, Sample)>,
    /// First placed point, while awaiting the second.
    pub distance_anchor: Option<Point>,

    /// Composite-export selection, armed by Ctrl+Shift+C.
    pub export_armed: bool,
    pub export_drag: DragState,

    pub help_visible: bool,
    /// When the start-up help overlay should begin fading.
    pub help_auto_hide_at: Option<Instant>,

    /// The discard being held, if any.
    hold: Option<Hold>,
    /// The prompt of a hold that ended before it confirmed, left up so that
    /// it can be read.
    prompt: Expiring<Destructive>,
    feedback: Expiring<String>,

    /// Set when the debug edge overlay should be drawn.
    pub debug_edges: bool,
    /// Transient edge-map preview shown after a sensitivity change.
    pub edge_preview_until: Option<Instant>,

    commands: Vec<Command>,
}

impl RulerState {
    pub fn new(sensitivity: f32, debug_edges: bool) -> Self {
        Self {
            mode: Mode::Crosshair,
            sensitivity,
            snap_distance: DEFAULT_SNAP_DISTANCE,
            color_radius: 0.0,
            session: false,
            annotations: Vec::new(),
            redo_stack: Vec::new(),
            pointer: None,
            active_monitor: 0,
            snapped: None,
            drag: DragState::default(),
            container: None,
            sample: None,
            distance_anchor: None,
            export_armed: false,
            export_drag: DragState::default(),
            help_visible: true,
            help_auto_hide_at: Some(Instant::now() + HELP_AUTO_HIDE),
            hold: None,
            prompt: Expiring::new(),
            feedback: Expiring::new(),
            debug_edges,
            edge_preview_until: None,
            commands: Vec::new(),
        }
    }

    /// Drains queued side effects for the shell to perform.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    fn push(&mut self, command: Command) {
        self.commands.push(command);
    }

    // ---------------------------------------------------------------- modes

    /// Switches mode, discarding any selection that does not carry over.
    pub fn set_mode(&mut self, mode: Mode) {
        self.cancel_hold();
        self.cancel_export();
        if self.mode == mode {
            return;
        }
        self.mode = mode;

        if !mode.is_rect_selection() {
            self.drag.clear();
        }
        // The snapped point is recomputed from the cursor once per frame, but a
        // keyboard mode switch lands *after* that pass and before the overlay is
        // painted. Clearing here stops a marker left over from the mode being
        // left showing for a frame in one that never snaps.
        if !mode.snaps() {
            self.snapped = None;
        }
        if mode != Mode::Container {
            self.container = None;
        }
        if mode != Mode::ColorPicker {
            self.sample = None;
        }
        if mode != Mode::Distance {
            self.distance_anchor = None;
        }
    }

    // ------------------------------------------------------------ selection

    /// The rectangle the current mode is measuring, if any.
    pub fn active_rect(&self) -> Option<Rect> {
        match self.mode {
            Mode::RectDrag | Mode::ShrinkToFit => {
                self.drag.has_selection.then(|| self.drag.rect()).flatten()
            }
            Mode::Container => self.container,
            _ => None,
        }
    }

    /// The measurement text for the current mode, or empty when nothing is measured.
    pub fn measurement_text(&self, crosshair: Option<(f32, f32)>) -> String {
        match self.mode {
            Mode::ColorPicker => self
                .sample
                .as_ref()
                .map(|(_, s)| s.summary())
                .unwrap_or_default(),
            // Measured to where the endpoint would be placed: the snapped
            // point when the cursor is near an edge, not the raw cursor.
            Mode::Distance => match (self.distance_anchor, self.snapped.or(self.pointer)) {
                (Some(a), Some(b)) => format_distance_summary(a.x, a.y, b.x, b.y),
                _ => String::new(),
            },
            Mode::RectDrag | Mode::ShrinkToFit | Mode::Container => self
                .active_rect()
                .map(|r| format_size(r.width, r.height))
                .unwrap_or_default(),
            Mode::Crosshair => crosshair
                .map(|(w, h)| format_size(w, h))
                .unwrap_or_default(),
        }
    }

    // --------------------------------------------------------- annotations

    /// Records an annotation, clearing the redo stack. It also ends a hold
    /// or prompt: it was about the annotations as they were.
    pub fn add_annotation(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
        self.redo_stack.clear();
        self.cancel_hold();
    }

    /// Undoes the last annotation. Returns false when there is nothing to
    /// undo. An undo ends a hold or prompt, as placing does.
    pub fn undo(&mut self) -> bool {
        let Some(annotation) = self.annotations.pop() else {
            return false;
        };
        self.redo_stack.push(annotation);
        self.cancel_hold();
        true
    }

    /// Redoes the last undone annotation. Returns false when there is
    /// nothing to redo. A redo ends a hold or prompt, as placing does.
    pub fn redo(&mut self) -> bool {
        let Some(annotation) = self.redo_stack.pop() else {
            return false;
        };
        self.annotations.push(annotation);
        self.cancel_hold();
        true
    }

    /// Renders every annotation as a Markdown list. When the annotations
    /// span several monitors, each line names its monitor, since the
    /// coordinates are monitor-local:
    ///
    /// ```text
    /// - Rectangle @ (12, 34) on monitor 2: 100 × 50 px
    /// - Distance on monitor 1: A(5, 6) → B(105, 6) — 100 px
    /// ```
    pub fn annotations_markdown(&self) -> String {
        let spans_monitors = self
            .annotations
            .windows(2)
            .any(|w| w[0].at.monitor != w[1].at.monitor);
        self.annotations
            .iter()
            .map(|annotation| {
                let title = annotation.mode.export_title();
                let measurement = annotation.measurement_text();
                // Numbered from 1 in capture order: by top edge, then by
                // left edge.
                let monitor = if spans_monitors {
                    let number = annotation.at.monitor + 1;
                    format!(" on monitor {number}")
                } else {
                    String::new()
                };
                match annotation.mode {
                    // A distance already names both endpoints, so prefixing it
                    // with a single anchor coordinate would be redundant.
                    Mode::Distance => format!("- {title}{monitor}: {measurement}"),
                    _ => {
                        let (x, y) = (annotation.at.x.round(), annotation.at.y.round());
                        format!("- {title} @ ({x}, {y}){monitor}: {measurement}")
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Copies all annotations as Markdown, keeping the session open.
    pub fn copy_annotations(&mut self) {
        let markdown = self.annotations_markdown();
        if markdown.is_empty() {
            self.show_feedback("No annotations to copy");
            return;
        }
        self.push(Command::CopyText(markdown));
        let count = self.annotations.len();
        self.show_feedback(&format!(
            "Copied {count} annotation{}",
            if count == 1 { "" } else { "s" }
        ));
    }

    // ------------------------------------------------------------- session

    /// Enters or leaves session mode. Leaving discards the annotations, the
    /// work in progress and the session's feedback message, which would
    /// otherwise outlive the session it was about.
    pub fn set_session(&mut self, enabled: bool) {
        if self.session == enabled {
            return;
        }
        self.session = enabled;
        if !enabled {
            self.annotations.clear();
            self.redo_stack.clear();
            self.cancel_export();
            // Half-placed work too, or it could be committed as a stale
            // annotation after re-entering the session.
            self.drag.clear();
            self.distance_anchor = None;
            self.feedback.clear();
        }
        self.cancel_hold();
    }

    /// True when leaving session mode would throw away placed annotations.
    fn has_work_to_lose(&self) -> bool {
        self.session && !self.annotations.is_empty()
    }

    /// Runs a destructive action at once, or starts holding it when it would
    /// discard annotations. The hold confirms in [`Self::advance_hold`] once
    /// held long enough, and [`Self::release_destructive`] ends it.
    pub fn request_destructive(&mut self, action: Destructive) {
        self.request_destructive_at(action, Instant::now());
    }

    /// [`Self::request_destructive`], for a press at `now`.
    fn request_destructive_at(&mut self, action: Destructive, now: Instant) {
        if !self.has_work_to_lose() {
            self.cancel_hold();
            self.perform_destructive(action);
            return;
        }
        // Every press starts the hold afresh, so two short presses never add
        // up to a long one. A different action takes the hold over.
        self.start_hold(action, now);
    }

    /// Quits at once, or starts or continues a hold by close requests when
    /// that would discard annotations: the window manager asked to close a
    /// window, as a held Alt+F4 does over and over on some systems.
    pub fn request_close(&mut self) {
        self.request_close_at(Instant::now());
    }

    /// [`Self::request_close`], for a close request at `now`.
    fn request_close_at(&mut self, now: Instant) {
        match &mut self.hold {
            Some(hold)
                if hold.action == Destructive::Close
                    && now.saturating_duration_since(hold.last_close) <= CLOSE_HOLD_GAP =>
            {
                hold.last_close = now;
            }
            // Nothing to lose, no hold, a close hold that has lapsed, or
            // another action's: as any other press.
            _ => self.request_destructive_at(Destructive::Close, now),
        }
    }

    /// Starts holding `action` from `now`, in place of any hold or prompt.
    fn start_hold(&mut self, action: Destructive, now: Instant) {
        self.prompt.clear();
        self.hold = Some(Hold {
            action,
            since: now,
            last_close: now,
            seen_full: false,
        });
    }

    /// Ends the hold, if any, without confirming it: its key or button was
    /// let go. Its prompt stays up a while, so that a tap still says what
    /// holding would do.
    pub fn release_destructive(&mut self) {
        if let Some(hold) = self.hold.take() {
            self.prompt.set(hold.action, PROMPT_DURATION);
        }
    }

    /// Confirms the hold on the frame after the one that finds it full, and
    /// ends a hold by close requests once they have stopped coming. Called
    /// on every frame while [`Self::holding`].
    ///
    /// The prompt left up when a close hold lapses expires on real time, not
    /// on `now`.
    pub fn advance_hold(&mut self, now: Instant) {
        let Some(hold) = &mut self.hold else {
            return;
        };
        let close_stopped = hold.action == Destructive::Close
            && now.saturating_duration_since(hold.last_close) > CLOSE_HOLD_GAP;
        if close_stopped {
            self.release_destructive();
        } else if hold.seen_full {
            let action = hold.action;
            self.cancel_hold();
            self.perform_destructive(action);
        } else if hold.is_full(now) {
            hold.seen_full = true;
        }
    }

    /// Whether a discard is being held, so frames must keep coming to
    /// confirm it.
    pub fn holding(&self) -> bool {
        self.hold.is_some()
    }

    /// How much of the hold has been held, from 0.0 to 1.0, for the bar
    /// that fills in the prompt. 0.0 when nothing is held. A close hold's bar
    /// stops at its latest close request, so it stops when the user lets go.
    pub fn hold_progress(&self, now: Instant) -> f32 {
        self.hold.map_or(0.0, |hold| {
            (hold.held(now).as_secs_f32() / HOLD_TO_CONFIRM.as_secs_f32()).min(1.0)
        })
    }

    fn perform_destructive(&mut self, action: Destructive) {
        match action {
            Destructive::ToggleSession | Destructive::SessionButton => self.set_session(false),
            Destructive::Escape => {
                if self.session {
                    self.set_session(false);
                } else {
                    self.push(Command::Quit);
                }
            }
            Destructive::Quit | Destructive::Close => self.push(Command::Quit),
        }
    }

    /// Ends the hold, if any, without confirming it, and takes its prompt
    /// down: whatever happened instead made it moot.
    pub fn cancel_hold(&mut self) {
        self.hold = None;
        self.prompt.clear();
    }

    /// The discard prompt to display: the held action's, or else the one a
    /// hold left up, while unexpired.
    pub fn destructive_prompt(&mut self) -> Option<&'static str> {
        match self.hold {
            Some(hold) => Some(hold.action.prompt()),
            None => self.prompt.get().map(|action| action.prompt()),
        }
    }

    pub fn show_feedback(&mut self, message: &str) {
        self.feedback.set(message.to_string(), FEEDBACK_DURATION);
    }

    /// The transient feedback message to display, if unexpired.
    pub fn feedback_message(&mut self) -> Option<String> {
        self.feedback.get().cloned()
    }

    // -------------------------------------------------------------- export

    /// Arms the composite-export selection, if there is anything to export.
    pub fn arm_export(&mut self) {
        if !self.session {
            return;
        }
        if self.annotations.is_empty() {
            self.show_feedback("Place an annotation before exporting");
            return;
        }
        self.export_armed = true;
        self.export_drag.clear();
        self.show_feedback("Drag a region to copy it with annotations");
    }

    pub fn cancel_export(&mut self) {
        self.export_armed = false;
        self.export_drag.clear();
    }

    /// Copies the dragged export region, then disarms.
    pub fn finish_export(&mut self) {
        let Some(rect) = self.export_drag.rect() else {
            self.cancel_export();
            return;
        };
        if rect.width < 1.0 || rect.height < 1.0 {
            self.cancel_export();
            return;
        }
        self.push(Command::CopyRegionImage(rect));
        self.cancel_export();
        self.show_feedback("Copied region with annotations");
    }

    // ------------------------------------------------------------ controls

    /// Sets sensitivity and schedules a re-analysis when it actually changed.
    ///
    /// Also arms the edge-map preview: this is the one place the threshold
    /// moves, so it is the one place that decides the flash should happen,
    /// whether the change came from the slider or the wheel.
    pub fn set_sensitivity(&mut self, value: f32) {
        let clamped = value.clamp(0.0, SENSITIVITY_MAX);
        if (clamped - self.sensitivity).abs() < 0.01 {
            return;
        }
        self.sensitivity = clamped;
        self.edge_preview_until = Some(Instant::now() + EDGE_PREVIEW);
        // One queued re-analysis covers any number of changes: it reads the
        // latest sensitivity, so a slider drag must not queue one per step.
        if !self.commands.contains(&Command::Recompute) {
            self.push(Command::Recompute);
        }
    }

    pub fn set_snap_distance(&mut self, value: f32) {
        self.snap_distance = value.clamp(0.0, SNAP_DISTANCE_MAX);
    }

    pub fn set_color_radius(&mut self, value: f32) {
        self.color_radius = value.clamp(0.0, COLOR_RADIUS_MAX);
    }

    /// Current value behind a dial, as its panel slider shows it.
    pub fn control_value(&self, control: Control) -> f32 {
        match control {
            Control::Sensitivity => self.sensitivity,
            Control::SnapDistance => self.snap_distance,
            Control::ColorRadius => self.color_radius,
        }
    }

    /// Writes back a dial, clamped to that dial's own range.
    pub fn set_control_value(&mut self, control: Control, value: f32) {
        match control {
            Control::Sensitivity => self.set_sensitivity(value),
            Control::SnapDistance => self.set_snap_distance(value),
            Control::ColorRadius => self.set_color_radius(value),
        }
    }

    /// Applies a scroll gesture to the current mode's primary control.
    ///
    /// One wheel notch is one step, so the dial responds without having to aim
    /// at the panel. Only the first dial is reachable this way: the wheel has
    /// no second axis to spend, and a modifier-plus-wheel gesture to reach the
    /// rest would be undiscoverable. The panel shows every dial, so nothing is
    /// unreachable — just slower to get at.
    pub fn adjust_by_wheel(&mut self, notches: f32) {
        if notches == 0.0 {
            return;
        }
        let control = self.mode.primary_control();
        self.set_control_value(control, self.control_value(control) + notches);
    }

    // --------------------------------------------------------------- quick

    /// Copies the current measurement and quits, the "measure and go" flow.
    pub fn copy_and_quit(&mut self, crosshair: Option<(f32, f32)>) {
        let text = self.measurement_text(crosshair);
        if text.is_empty() {
            return;
        }
        self.push(Command::CopyTextAndQuit(text));
    }

    /// Toggles the shortcut overlay, cancelling any pending auto-hide.
    pub fn toggle_help(&mut self) {
        self.help_visible = !self.help_visible;
        self.help_auto_hide_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> RulerState {
        RulerState::new(85.0, false)
    }

    fn point(x: f32, y: f32) -> Point {
        Point { monitor: 0, x, y }
    }

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// Runs the frame timer from `t0` to `t0 + for_ms`: a frame every
    /// 16 ms, as the app's.
    fn frames(s: &mut RulerState, t0: Instant, for_ms: u64) {
        for millis in (16..=for_ms).step_by(16) {
            s.advance_hold(t0 + ms(millis));
        }
    }

    /// A session with an annotation, so that a destructive action has to be
    /// held.
    fn session_with_work() -> RulerState {
        let mut s = state();
        s.set_session(true);
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        s
    }

    #[test]
    fn mode_indices_match_the_number_key_shortcuts() {
        assert_eq!(Mode::Crosshair.index(), 0);
        assert_eq!(Mode::Distance.index(), 5);
        assert_eq!(Mode::from_index(0), Some(Mode::Crosshair));
        assert_eq!(Mode::from_index(5), Some(Mode::Distance));
        assert_eq!(Mode::from_index(6), None);
        for (i, mode) in Mode::ALL.iter().enumerate() {
            assert_eq!(mode.index(), i);
        }
    }

    #[test]
    fn number_keys_select_modes_by_digit() {
        assert_eq!(Mode::from_digit(1), Some(Mode::Crosshair));
        assert_eq!(Mode::from_digit(6), Some(Mode::Distance));
        assert_eq!(Mode::from_digit(0), None, "there is no mode 0");
        assert_eq!(Mode::from_digit(MODE_COUNT + 1), None);
        for mode in Mode::ALL {
            assert_eq!(Mode::from_digit(mode.digit()), Some(mode), "{mode:?}");
        }
    }

    #[test]
    fn every_mode_declares_usable_controls() {
        for mode in Mode::ALL {
            let controls = mode.controls();
            assert!(!controls.is_empty(), "{mode:?} has no dial at all");

            let mut seen = Vec::new();
            for control in controls {
                assert!(
                    !seen.contains(control),
                    "{mode:?} lists {control:?} twice, so two sliders would fight \
                     over one value",
                );
                seen.push(*control);
            }
        }
    }

    #[test]
    fn every_snapping_mode_can_reach_both_dials_snapping_depends_on() {
        // Snapping needs two things the user can get wrong: a radius, and an
        // edge map coarse enough to have found the edge. Offering only one of
        // them strands the mode — point distance shipped with exactly that,
        // snapping against a sensitivity it had no way to raise.
        for mode in Mode::ALL.into_iter().filter(|mode| mode.snaps()) {
            let controls = mode.controls();
            assert_eq!(
                controls.first(),
                Some(&Control::SnapDistance),
                "{mode:?} snaps, so the wheel should drive its snap radius",
            );
            assert!(
                controls.contains(&Control::Sensitivity),
                "{mode:?} snaps against the edge map but cannot tune it: {controls:?}",
            );
        }
    }

    #[test]
    fn point_distance_can_adjust_the_snapping_it_actually_uses() {
        // Regression: distance mode snapped the cursor using `snap_distance`,
        // while both the panel slider and the wheel edited sensitivity.
        let mut s = state();
        s.set_mode(Mode::Distance);
        assert!(Mode::Distance.snaps());

        s.adjust_by_wheel(-4.0);
        assert_eq!(s.snap_distance, DEFAULT_SNAP_DISTANCE - 4.0);
        assert_eq!(s.sensitivity, 85.0, "the wheel drives only the first dial");

        // ...but sensitivity is still on the panel, so it is reachable.
        s.set_control_value(Control::Sensitivity, 30.0);
        assert_eq!(s.sensitivity, 30.0);
    }

    #[test]
    fn shrink_to_fit_leaves_the_edges_to_the_shrink() {
        // The shrink finds the content edges itself; snapping the drag corners
        // could only pull them inside the content first.
        assert!(!Mode::ShrinkToFit.snaps());
        assert_eq!(Mode::ShrinkToFit.primary_control(), Control::Sensitivity);
    }

    #[test]
    fn leaving_a_snapping_mode_drops_the_snapped_point() {
        // Regression: the snapped point outlived the mode that produced it for
        // one frame, because a keyboard mode switch lands after the pass that
        // recomputes it, and the marker was painted in between.
        let mut s = state();
        s.set_mode(Mode::RectDrag);
        s.snapped = Some(point(10.0, 10.0));

        s.set_mode(Mode::Crosshair);
        assert!(
            s.snapped.is_none(),
            "a non-snapping mode must not inherit it"
        );

        // Between two snapping modes it is still valid, so it survives.
        s.set_mode(Mode::RectDrag);
        s.snapped = Some(point(10.0, 10.0));
        s.set_mode(Mode::Distance);
        assert!(s.snapped.is_some());
    }

    #[test]
    fn the_wheel_drives_the_primary_dial_and_writes_reach_the_named_one() {
        let mut s = state();
        s.sensitivity = 40.0;
        s.snap_distance = 12.0;
        s.color_radius = 6.0;

        for (mode, expected) in [
            (Mode::Crosshair, 40.0),
            (Mode::RectDrag, 12.0),
            (Mode::Container, 40.0),
            (Mode::ShrinkToFit, 40.0),
            (Mode::ColorPicker, 6.0),
            (Mode::Distance, 12.0),
        ] {
            s.set_mode(mode);
            let primary = mode.primary_control();
            assert_eq!(s.control_value(primary), expected, "{mode:?}");
        }

        // A write lands on the named dial, clamped to that dial's own range.
        s.set_mode(Mode::ColorPicker);
        s.set_control_value(Control::ColorRadius, COLOR_RADIUS_MAX + 10.0);
        assert_eq!(s.color_radius, COLOR_RADIUS_MAX);
        assert_eq!(s.snap_distance, 12.0, "other dials must not move");
    }

    #[test]
    fn switching_mode_drops_only_the_state_that_cannot_carry_over() {
        let mut s = state();
        s.container = Some(Rect {
            monitor: 0,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        });
        s.distance_anchor = Some(point(1.0, 2.0));
        s.drag.has_selection = true;

        s.set_mode(Mode::RectDrag);
        assert!(s.container.is_none(), "container selection should clear");
        assert!(s.distance_anchor.is_none(), "distance anchor should clear");
        assert!(
            s.drag.has_selection,
            "rect selection carries into a rect mode"
        );

        // Shrink-to-fit is also a rect mode, so the selection survives.
        s.set_mode(Mode::ShrinkToFit);
        assert!(s.drag.has_selection);

        s.set_mode(Mode::Crosshair);
        assert!(!s.drag.has_selection, "rect selection should clear");
    }

    #[test]
    fn size_and_distance_formatting_matches_the_previous_ui() {
        assert_eq!(format_size(120.0, 40.0), "120 × 40 px");
        assert_eq!(format_size(-120.4, 40.6), "120 × 41 px");
        assert_eq!(format_distance(10.0), "10 px");
        assert_eq!(format_distance(10.25), "10.3 px");
        assert_eq!(format_distance(10.04), "10 px");
    }

    #[test]
    fn the_breakdown_is_judged_on_the_deltas_it_prints() {
        // 8.1 prints as 8, which is not past the threshold.
        assert!(!shows_delta_breakdown(8.1, 20.0));
        assert!(!format_distance_summary(0.0, 0.0, 8.1, 20.0).contains('\u{0394}'));
        assert!(shows_delta_breakdown(-8.6, 20.0));
        assert!(format_distance_summary(0.0, 0.0, -8.6, 20.0).contains("\u{0394}x=9"));
    }

    #[test]
    fn distance_summary_adds_a_breakdown_only_when_diagonal() {
        // Nearly horizontal: dy is below the threshold, so no breakdown.
        let flat = format_distance_summary(0.0, 0.0, 100.0, 4.0);
        assert!(flat.starts_with("A(0, 0) → B(100, 4) — "), "{flat}");
        assert!(!flat.contains('Δ'), "{flat}");

        let diagonal = format_distance_summary(0.0, 0.0, 100.0, 50.0);
        assert!(diagonal.contains("(Δx=100, Δy=50)"), "{diagonal}");
    }

    #[test]
    fn undo_and_redo_walk_the_annotation_history() {
        let mut s = state();
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        s.add_annotation(Annotation::test_rect(5.0, 5.0, 20.0, 20.0));
        assert_eq!(s.annotations.len(), 2);

        assert!(s.undo());
        assert_eq!(s.annotations.len(), 1);
        assert!(s.redo());
        assert_eq!(s.annotations.len(), 2);

        // Nothing left to redo.
        assert!(!s.redo());
        assert_eq!(s.annotations.len(), 2);
    }

    #[test]
    fn placing_a_new_annotation_discards_the_redo_stack() {
        let mut s = state();
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        assert!(s.undo());
        s.add_annotation(Annotation::test_rect(1.0, 1.0, 5.0, 5.0));

        assert!(!s.redo(), "redo should not resurrect a discarded branch");
        assert_eq!(s.annotations.len(), 1);
    }

    #[test]
    fn undo_ends_a_hold() {
        let mut s = session_with_work();
        s.request_destructive(Destructive::ToggleSession);

        assert!(s.undo());
        assert_eq!(s.destructive_prompt(), None);
        assert!(s.session);
    }

    #[test]
    fn redo_ends_a_hold() {
        let mut s = session_with_work();
        s.add_annotation(Annotation::test_rect(5.0, 5.0, 20.0, 20.0));
        assert!(s.undo());
        s.request_destructive(Destructive::ToggleSession);

        assert!(s.redo());
        assert_eq!(s.destructive_prompt(), None);
        assert!(s.session);
    }

    #[test]
    fn a_redo_with_nothing_to_redo_keeps_the_hold_going() {
        // Nothing changed, so the hold is still about the same annotations.
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::ToggleSession, t0);

        assert!(!s.redo());
        frames(&mut s, t0, 1100);
        assert!(!s.session);
    }

    #[test]
    fn undo_on_an_empty_history_is_a_no_op() {
        let mut s = state();
        assert!(!s.undo());
        assert!(!s.redo());
    }

    #[test]
    fn markdown_export_lists_each_annotation_with_its_anchor() {
        let mut s = state();
        s.add_annotation(Annotation::test_rect(12.0, 34.0, 100.0, 50.0));
        s.add_annotation(Annotation {
            at: point(5.0, 6.0),
            mode: Mode::Distance,
            kind: AnnotationKind::Distance {
                to_x: 105.0,
                to_y: 6.0,
            },
        });

        let markdown = s.annotations_markdown();
        let lines: Vec<&str> = markdown.lines().collect();
        assert_eq!(lines[0], "- Rectangle @ (12, 34): 100 × 50 px");
        // A distance names both endpoints itself, so it carries no anchor prefix.
        assert!(
            lines[1].starts_with("- Distance: A(5, 6) → B(105, 6)"),
            "{}",
            lines[1]
        );
    }

    #[test]
    fn a_line_rectangle_exports_its_zero_side() {
        let mut s = state();
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 200.0, 0.0));
        assert_eq!(s.annotations_markdown(), "- Rectangle @ (0, 0): 200 × 0 px");
    }

    #[test]
    fn markdown_export_names_the_monitor_when_annotations_span_several() {
        let mut s = state();
        s.add_annotation(Annotation::test_rect(12.0, 34.0, 100.0, 50.0));
        s.add_annotation(Annotation {
            at: Point {
                monitor: 1,
                x: 5.0,
                y: 6.0,
            },
            mode: Mode::Distance,
            kind: AnnotationKind::Distance {
                to_x: 105.0,
                to_y: 6.0,
            },
        });

        let markdown = s.annotations_markdown();
        let lines: Vec<&str> = markdown.lines().collect();
        assert_eq!(lines[0], "- Rectangle @ (12, 34) on monitor 1: 100 × 50 px");
        assert!(
            lines[1].starts_with("- Distance on monitor 2: A(5, 6) → B(105, 6)"),
            "{}",
            lines[1]
        );
    }

    #[test]
    fn markdown_export_on_one_monitor_does_not_name_it() {
        let mut s = state();
        for x in [0.0, 50.0] {
            let mut annotation = Annotation::test_rect(x, 0.0, 10.0, 10.0);
            annotation.at.monitor = 1;
            s.add_annotation(annotation);
        }

        let markdown = s.annotations_markdown();
        assert!(!markdown.contains("monitor"), "{markdown}");
    }

    #[test]
    fn markdown_export_numbers_monitors_by_index_not_by_order() {
        // Annotations come in placement order; the number must still be the
        // monitor's own, so that it matches the layout.
        let mut s = state();
        s.add_annotation(Annotation {
            at: Point {
                monitor: 2,
                x: 5.0,
                y: 6.0,
            },
            mode: Mode::Distance,
            kind: AnnotationKind::Distance {
                to_x: 5.0,
                to_y: 56.0,
            },
        });
        s.add_annotation(Annotation::test_rect(12.0, 34.0, 100.0, 50.0));

        let markdown = s.annotations_markdown();
        let lines: Vec<&str> = markdown.lines().collect();
        assert!(
            lines[0].starts_with("- Distance on monitor 3: A(5, 6) → B(5, 56)"),
            "{}",
            lines[0]
        );
        assert_eq!(lines[1], "- Rectangle @ (12, 34) on monitor 1: 100 × 50 px");
    }

    #[test]
    fn markdown_export_of_nothing_is_empty() {
        assert_eq!(state().annotations_markdown(), "");
    }

    #[test]
    fn colour_annotations_export_all_three_notations() {
        let mut s = state();
        s.add_annotation(Annotation {
            at: point(3.0, 4.0),
            mode: Mode::ColorPicker,
            kind: AnnotationKind::Color {
                sample: Sample::from_rgb(230, 25, 94),
                radius: 0.0,
            },
        });
        let markdown = s.annotations_markdown();
        assert!(markdown.contains("#E6195E"), "{markdown}");
        assert!(markdown.contains("rgb(230, 25, 94)"), "{markdown}");
        assert!(markdown.contains("hsl("), "{markdown}");
    }

    #[test]
    fn leaving_session_mode_with_annotations_needs_a_one_second_hold() {
        let mut s = session_with_work();
        let t0 = Instant::now();

        s.request_destructive_at(Destructive::ToggleSession, t0);
        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Tab to discard annotations")
        );
        s.advance_hold(t0 + ms(999));
        assert!(s.session, "a hold short of a second discards nothing");

        // Full at one second; confirmed on the frame after.
        s.advance_hold(t0 + ms(1000));
        s.advance_hold(t0 + ms(1016));
        assert!(!s.session, "a one-second hold confirms");
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn a_full_hold_waits_a_frame_before_it_confirms() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::ToggleSession, t0);

        s.advance_hold(t0 + ms(1000));
        assert!(s.session);
    }

    #[test]
    fn a_release_right_after_the_hold_fills_discards_nothing() {
        // Slint runs timers before queued input: a release that came before
        // the frame which found the hold full is handled after it.
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::ToggleSession, t0);

        s.advance_hold(t0 + ms(1000));
        s.release_destructive();
        s.advance_hold(t0 + ms(1016));

        assert!(s.session);
        assert_eq!(s.annotations.len(), 1);
    }

    #[test]
    fn letting_go_early_discards_nothing() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::Escape, t0);

        frames(&mut s, t0, 500);
        s.release_destructive();
        frames(&mut s, t0 + ms(500), 1500);

        assert!(s.session);
        assert_eq!(s.annotations.len(), 1);
    }

    #[test]
    fn letting_go_early_leaves_the_prompt_up_with_an_empty_bar() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::Escape, t0);

        s.release_destructive();

        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Esc to discard annotations")
        );
        assert_eq!(s.hold_progress(t0 + ms(500)), 0.0, "nothing is held");
    }

    #[test]
    fn pressing_twice_does_not_confirm() {
        let mut s = session_with_work();
        let t0 = Instant::now();

        s.request_destructive_at(Destructive::ToggleSession, t0);
        s.release_destructive();
        s.request_destructive_at(Destructive::ToggleSession, t0 + ms(800));
        frames(&mut s, t0 + ms(800), 400);

        assert!(s.session, "the second press starts the hold afresh");
    }

    #[test]
    fn a_second_press_restarts_the_hold_even_without_a_release() {
        // A release can go missing; the next press must not then confirm a
        // hold nobody kept.
        let mut s = session_with_work();
        let t0 = Instant::now();

        s.request_destructive_at(Destructive::ToggleSession, t0);
        s.request_destructive_at(Destructive::ToggleSession, t0 + ms(800));
        frames(&mut s, t0 + ms(800), 400);

        assert!(s.session);
    }

    #[test]
    fn a_different_action_takes_the_hold_over_from_its_own_press() {
        let mut s = session_with_work();
        let t0 = Instant::now();

        s.request_destructive_at(Destructive::ToggleSession, t0);
        s.request_destructive_at(Destructive::Quit, t0 + ms(600));
        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Q to discard annotations and quit")
        );

        // Past a second after the Tab, but not after the Q.
        frames(&mut s, t0 + ms(600), 600);
        assert!(s.session, "the Tab hold is gone");
        assert!(s.take_commands().is_empty());

        frames(&mut s, t0 + ms(1200), 500);
        assert!(s.take_commands().contains(&Command::Quit));
    }

    #[test]
    fn a_single_close_only_starts_a_hold_that_lapses() {
        let mut s = session_with_work();
        let t0 = Instant::now();

        s.request_close_at(t0);
        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Q to discard annotations and quit")
        );
        frames(&mut s, t0, 500);

        assert!(s.take_commands().is_empty());
        assert!(!s.holding(), "the hold is gone");
        assert!(s.destructive_prompt().is_some(), "the prompt stays up");
    }

    #[test]
    fn a_close_burst_does_not_quit() {
        // Some docks send several close requests for one Quit.
        let mut s = session_with_work();
        let t0 = Instant::now();
        for millis in [0, 1, 2] {
            s.request_close_at(t0 + ms(millis));
        }

        frames(&mut s, t0, 1500);
        assert!(s.take_commands().is_empty());
        assert!(s.session);
    }

    #[test]
    fn a_held_close_fills_the_hold_and_quits() {
        // A held Alt+F4: one close, nothing through a 500 ms repeat delay,
        // then a close every 30 ms; a frame every 16 ms.
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_close_at(t0);

        let mut quit_at = None;
        for millis in 1..=2000 {
            let now = t0 + ms(millis);
            if millis >= 500 && (millis - 500) % 30 == 0 {
                s.request_close_at(now);
            }
            if millis % 16 == 0 {
                s.advance_hold(now);
            }
            if s.take_commands().contains(&Command::Quit) {
                quit_at = Some(millis);
                break;
            }
        }

        // The single close lapses during the repeat delay, so the hold runs
        // from the first repeat, at 500 ms. The close at 1520 ms is the first
        // a second after it, and the frame after it, at 1536 ms, confirms.
        assert_eq!(quit_at, Some(1536));
    }

    #[test]
    fn letting_go_of_a_held_close_before_the_bar_fills_does_not_quit() {
        // Closes every 30 ms up to 85% of the hold, then none: there is no
        // release, so only the missing requests can tell.
        let mut s = session_with_work();
        let t0 = Instant::now();
        for millis in (0..=850).step_by(30) {
            s.request_close_at(t0 + ms(millis));
        }

        frames(&mut s, t0, 2000);
        assert!(s.take_commands().is_empty());
        assert!(s.session);
    }

    #[test]
    fn a_close_hold_bar_stops_when_the_closes_stop() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        for millis in (0..=850).step_by(50) {
            s.request_close_at(t0 + ms(millis));
        }

        // Every frame until the hold lapses, 450 ms after the last close.
        for millis in (850..=1300).step_by(16) {
            let now = t0 + ms(millis);
            s.advance_hold(now);
            assert_eq!(s.hold_progress(now), 0.85, "at {millis} ms");
        }
    }

    #[test]
    fn a_close_request_a_second_into_the_hold_quits() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        for millis in (0..=1020).step_by(30) {
            s.request_close_at(t0 + ms(millis));
        }

        frames(&mut s, t0, 1100);
        assert!(s.take_commands().contains(&Command::Quit));
    }

    #[test]
    fn a_close_repeating_every_400_ms_still_fills_the_hold() {
        // The slowest key repeat Windows offers; a frame every 16 ms.
        let mut s = session_with_work();
        let t0 = Instant::now();
        for millis in 0..=1300 {
            let now = t0 + ms(millis);
            if millis % 400 == 0 {
                s.request_close_at(now);
            }
            if millis % 16 == 0 {
                s.advance_hold(now);
            }
        }

        assert!(s.take_commands().contains(&Command::Quit));
    }

    #[test]
    fn a_close_quits_at_once_when_nothing_would_be_lost() {
        let mut s = state();
        s.set_session(true);
        s.request_close_at(Instant::now());
        assert!(s.take_commands().contains(&Command::Quit));
    }

    #[test]
    fn placing_an_annotation_ends_a_hold() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::ToggleSession, t0);

        s.add_annotation(Annotation::test_rect(5.0, 5.0, 20.0, 20.0));
        assert_eq!(s.destructive_prompt(), None);
        frames(&mut s, t0, 1100);
        assert!(s.session);
        assert_eq!(s.annotations.len(), 2);
    }

    #[test]
    fn the_hold_progress_fills_over_the_hold() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        assert_eq!(s.hold_progress(t0), 0.0);

        s.request_destructive_at(Destructive::Quit, t0);
        assert_eq!(s.hold_progress(t0), 0.0);
        assert_eq!(s.hold_progress(t0 + ms(250)), 0.25);
        assert_eq!(s.hold_progress(t0 + ms(5000)), 1.0, "capped when full");
    }

    #[test]
    fn a_held_esc_leaves_the_session_without_quitting() {
        // Esc both leaves the session and quits: a confirmed hold must do
        // only the first.
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::Escape, t0);

        frames(&mut s, t0, 1100);
        assert!(!s.session);
        assert!(s.take_commands().is_empty());
    }

    #[test]
    fn a_confirmed_hold_leaves_no_prompt_behind() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::Quit, t0);

        frames(&mut s, t0, 1100);
        assert!(s.take_commands().contains(&Command::Quit));
        assert_eq!(s.destructive_prompt(), None);
        assert!(!s.holding());
    }

    #[test]
    fn no_action_is_held_when_nothing_would_be_lost() {
        let actions = [
            Destructive::ToggleSession,
            Destructive::SessionButton,
            Destructive::Escape,
            Destructive::Quit,
        ];
        for action in actions {
            let mut s = state();
            s.set_session(true);
            let t0 = Instant::now();
            s.request_destructive_at(action, t0);

            let quit = s.take_commands().contains(&Command::Quit);
            assert!(!s.session || quit, "{action:?} ran at once");
            assert_eq!(s.destructive_prompt(), None, "{action:?}");
            assert_eq!(s.hold_progress(t0), 0.0, "{action:?}");
        }
    }

    #[test]
    fn switching_mode_ends_a_hold() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::ToggleSession, t0);

        s.set_mode(Mode::Distance);
        assert_eq!(s.destructive_prompt(), None);
        frames(&mut s, t0, 1100);
        assert!(s.session);
    }

    #[test]
    fn undo_takes_down_the_prompt_a_hold_left_up() {
        let mut s = session_with_work();
        s.request_destructive_at(Destructive::Escape, Instant::now());
        s.release_destructive();
        assert!(s.destructive_prompt().is_some());

        assert!(s.undo());
        assert_eq!(s.destructive_prompt(), None);
    }

    #[test]
    fn the_prompt_a_hold_left_up_goes_once_it_expires() {
        // Set directly with no lifetime: `release_destructive` gives it
        // `PROMPT_DURATION` of real time, which a test would have to sleep.
        let mut s = session_with_work();
        s.prompt.set(Destructive::Escape, Duration::ZERO);

        assert_eq!(s.destructive_prompt(), None);
    }

    #[test]
    fn a_stopped_close_hold_ends_on_the_first_frame_after_a_stall() {
        // Closes come for 700 ms then stop. The first frame, after a long
        // stall, comes 510 ms after the last close: the hold must end.
        let mut s = session_with_work();
        let t0 = Instant::now();
        for millis in (0..=700).step_by(30) {
            s.request_close_at(t0 + ms(millis));
        }

        s.advance_hold(t0 + ms(1200));
        assert!(s.take_commands().is_empty());
        assert!(!s.holding(), "the hold is gone");
        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Q to discard annotations and quit")
        );
    }

    #[test]
    fn close_requests_exactly_the_gap_apart_keep_the_hold_going() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_close_at(t0);
        s.request_close_at(t0 + CLOSE_HOLD_GAP);

        s.advance_hold(t0 + CLOSE_HOLD_GAP * 2);
        assert_eq!(
            s.hold_progress(t0 + CLOSE_HOLD_GAP * 2),
            0.45,
            "held from the first close to the second"
        );
    }

    #[test]
    fn a_close_after_a_lapse_starts_the_hold_afresh_without_a_frame_between() {
        // No frame ran to end the first hold: the second close must still
        // see it has lapsed.
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_close_at(t0);
        s.request_close_at(t0 + ms(500));

        assert_eq!(s.hold_progress(t0 + ms(500)), 0.0);
    }

    #[test]
    fn a_close_takes_the_hold_over_from_a_key() {
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_destructive_at(Destructive::ToggleSession, t0);
        // Within `CLOSE_HOLD_GAP` of the Tab, which must not matter.
        s.request_close_at(t0 + ms(100));
        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Q to discard annotations and quit")
        );

        // The close is held, repeating every 40 ms.
        let mut quit_at = None;
        for millis in (140..=2000).step_by(40) {
            let now = t0 + ms(millis);
            s.request_close_at(now);
            s.advance_hold(now);
            if s.take_commands().contains(&Command::Quit) {
                quit_at = Some(millis);
                break;
            }
        }

        assert!(s.session, "the Tab hold did not confirm");
        assert_eq!(
            quit_at,
            Some(1140),
            "on the frame after the close a second after the first"
        );
    }

    #[test]
    fn a_key_takes_the_hold_over_from_a_close() {
        // The key's hold has a release, so it does not lapse when closes
        // stop coming.
        let mut s = session_with_work();
        let t0 = Instant::now();
        s.request_close_at(t0);
        s.request_destructive_at(Destructive::Quit, t0 + ms(100));
        assert_eq!(
            s.destructive_prompt(),
            Some("Hold Q to discard annotations and quit")
        );

        s.advance_hold(t0 + ms(1099));
        s.advance_hold(t0 + ms(1100));
        assert!(s.take_commands().is_empty());
        s.advance_hold(t0 + ms(1116));
        assert!(s.take_commands().contains(&Command::Quit));
    }

    #[test]
    fn destructive_actions_run_immediately_when_nothing_would_be_lost() {
        let mut s = state();
        s.request_destructive(Destructive::Quit);
        assert!(s.take_commands().contains(&Command::Quit));

        // In session mode but with no annotations, there is nothing to confirm.
        let mut s = state();
        s.set_session(true);
        s.request_destructive(Destructive::ToggleSession);
        assert!(!s.session);
    }

    #[test]
    fn escape_leaves_session_mode_before_it_quits() {
        let mut s = state();
        s.set_session(true);
        s.request_destructive(Destructive::Escape);
        assert!(!s.session);
        assert!(
            !s.take_commands().contains(&Command::Quit),
            "escape should exit the session, not the app"
        );

        s.request_destructive(Destructive::Escape);
        assert!(s.take_commands().contains(&Command::Quit));
    }

    #[test]
    fn sensitivity_changes_schedule_exactly_one_recompute() {
        let mut s = state();
        s.set_sensitivity(50.0);
        assert_eq!(s.take_commands(), vec![Command::Recompute]);

        // An identical value must not trigger redundant re-analysis.
        s.set_sensitivity(50.0);
        assert!(s.take_commands().is_empty());

        s.set_sensitivity(-10.0);
        assert_eq!(s.sensitivity, 0.0);
        assert_eq!(s.take_commands(), vec![Command::Recompute]);
    }

    #[test]
    fn a_slider_drag_queues_a_single_recompute() {
        let mut s = state();
        for value in [50.0, 51.0, 52.0, 53.0] {
            s.set_sensitivity(value);
        }
        assert_eq!(s.take_commands(), vec![Command::Recompute]);
        assert_eq!(s.sensitivity, 53.0);

        // Once drained, the next change queues again.
        s.set_sensitivity(60.0);
        assert_eq!(s.take_commands(), vec![Command::Recompute]);
    }

    #[test]
    fn the_distance_readout_measures_to_the_snapped_endpoint() {
        let mut s = state();
        s.set_mode(Mode::Distance);
        s.distance_anchor = Some(point(0.0, 0.0));
        s.pointer = Some(point(97.0, 0.0));
        assert!(s.measurement_text(None).contains("B(97, 0)"));

        s.snapped = Some(point(100.0, 0.0));
        let text = s.measurement_text(None);
        assert!(text.contains("B(100, 0)"), "{text}");
    }

    #[test]
    fn leaving_the_session_drops_half_placed_work() {
        let mut s = state();
        s.set_session(true);
        s.set_mode(Mode::Distance);
        s.distance_anchor = Some(point(1.0, 2.0));
        s.drag.has_selection = true;

        s.request_destructive(Destructive::ToggleSession);
        assert!(!s.session);
        assert!(s.distance_anchor.is_none());
        assert!(s.drag.rect().is_none());
    }

    #[test]
    fn leaving_the_session_clears_its_feedback() {
        let mut s = state();
        s.set_session(true);
        s.show_feedback("Session mode: measurements stay on screen");

        s.request_destructive(Destructive::ToggleSession);
        assert!(!s.session);
        assert_eq!(s.feedback_message(), None);
    }

    #[test]
    fn the_wheel_drives_whichever_control_the_mode_owns() {
        let mut s = state();

        s.set_mode(Mode::Crosshair);
        s.adjust_by_wheel(5.0);
        assert_eq!(s.sensitivity, 90.0);

        s.set_mode(Mode::RectDrag);
        s.adjust_by_wheel(-3.0);
        assert_eq!(s.snap_distance, DEFAULT_SNAP_DISTANCE - 3.0);
        assert_eq!(
            s.sensitivity, 90.0,
            "sensitivity must not move in rect mode"
        );

        s.set_mode(Mode::ColorPicker);
        s.adjust_by_wheel(4.0);
        assert_eq!(s.color_radius, 4.0);
    }

    #[test]
    fn control_values_are_clamped_to_their_slider_range() {
        let mut s = state();
        s.set_mode(Mode::ColorPicker);
        s.adjust_by_wheel(1000.0);
        assert_eq!(s.color_radius, COLOR_RADIUS_MAX);
        s.adjust_by_wheel(-1000.0);
        assert_eq!(s.color_radius, 0.0);

        s.set_mode(Mode::RectDrag);
        s.adjust_by_wheel(1000.0);
        assert_eq!(s.snap_distance, SNAP_DISTANCE_MAX);
    }

    #[test]
    fn quick_copy_emits_nothing_when_there_is_no_measurement() {
        let mut s = state();
        s.set_mode(Mode::ColorPicker);
        s.copy_and_quit(None);
        assert!(s.take_commands().is_empty());

        s.set_mode(Mode::Crosshair);
        s.copy_and_quit(Some((100.0, 50.0)));
        assert_eq!(
            s.take_commands(),
            vec![Command::CopyTextAndQuit("100 × 50 px".to_string())]
        );
    }

    #[test]
    fn export_requires_a_session_with_annotations() {
        let mut s = state();
        s.arm_export();
        assert!(!s.export_armed, "export outside a session should be inert");

        s.set_session(true);
        s.arm_export();
        assert!(!s.export_armed, "export needs at least one annotation");

        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        s.arm_export();
        assert!(s.export_armed);
    }

    #[test]
    fn a_degenerate_export_drag_copies_nothing() {
        let mut s = state();
        s.set_session(true);
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        s.arm_export();
        let _ = s.take_commands();

        s.export_drag.has_selection = true;
        s.export_drag.start = (10.0, 10.0);
        s.export_drag.end = (10.2, 10.2);
        s.finish_export();

        assert!(!s.export_armed);
        assert!(
            !s.take_commands()
                .iter()
                .any(|c| matches!(c, Command::CopyRegionImage(_))),
            "a sub-pixel drag should not produce an image"
        );
    }

    #[test]
    fn a_real_export_drag_requests_the_composite() {
        let mut s = state();
        s.set_session(true);
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        s.arm_export();
        let _ = s.take_commands();

        s.export_drag.has_selection = true;
        s.export_drag.monitor = 1;
        s.export_drag.start = (100.0, 80.0);
        s.export_drag.end = (20.0, 20.0);
        s.finish_export();

        let commands = s.take_commands();
        let rect = commands
            .iter()
            .find_map(|c| match c {
                Command::CopyRegionImage(r) => Some(*r),
                _ => None,
            })
            .expect("expected a composite request");
        // The drag is normalised regardless of its direction.
        assert_eq!(rect.monitor, 1);
        assert_eq!((rect.x, rect.y), (20.0, 20.0));
        assert_eq!((rect.width, rect.height), (80.0, 60.0));
    }

    #[test]
    fn a_drag_is_judged_by_the_cursor_not_the_snapped_corners() {
        // Regression: with snapping on, a real drag whose ends both land on the
        // same edge pixel has `start == end`. Judging it from those corners
        // reported "no movement" and silently discarded the selection, so short
        // drags started next to a UI edge did nothing at all.
        let drag = DragState {
            active: true,
            has_selection: false,
            monitor: 0,
            press: (100.0, 100.0),
            // Both corners snapped onto the same edge pixel.
            start: (96.0, 96.0),
            end: (96.0, 96.0),
        };

        assert!(
            drag.is_drag((100.0, 112.0), 2.0),
            "a 12 px drag is a drag however its corners snapped",
        );
        assert!(
            !drag.is_drag((101.0, 101.0), 2.0),
            "a 1 px wobble is a click"
        );
        // Exactly at the threshold counts, so the boundary is not a dead zone.
        assert!(drag.is_drag((102.0, 100.0), 2.0));
    }

    #[test]
    fn drag_rects_normalise_in_every_direction() {
        let mut drag = DragState {
            active: false,
            has_selection: true,
            monitor: 0,
            start: (100.0, 100.0),
            end: (40.0, 30.0),
            press: (100.0, 100.0),
        };
        let rect = drag.rect().expect("selection");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (40.0, 30.0, 60.0, 70.0)
        );

        drag.clear();
        assert!(drag.rect().is_none());
    }
}
