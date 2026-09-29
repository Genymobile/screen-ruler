//! Interaction state and the rules that govern it.
//!
//! Deliberately free of any windowing or drawing types: the UI layer reads this
//! and renders it, and side effects the shell must perform (quit, clipboard)
//! leave through [`RulerState::take_commands`]. That keeps the behaviour that
//! is easy to get wrong — mode switching, undo/redo, the confirm-before-discard
//! flow — testable without a display.

use std::time::{Duration, Instant};

use crate::color::Sample;
use crate::geometry::{Point, Rect};

/// How long a destructive action stays armed awaiting confirmation.
const CONFIRM_WINDOW: Duration = Duration::from_millis(1500);
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
    ShrinkToFit => controls [SnapDistance, Sensitivity], rect true,
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
    if dx.abs().round().min(dy.abs().round()) > DELTA_BREAKDOWN_THRESHOLD {
        summary.push_str(&format!(
            " (\u{0394}x={}, \u{0394}y={})",
            dx.abs().round(),
            dy.abs().round()
        ));
    }
    summary
}

/// An action that discards work and therefore asks for confirmation.
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
}

impl Destructive {
    fn prompt(self) -> &'static str {
        match self {
            Destructive::ToggleSession => "Press Tab again to discard annotations",
            Destructive::SessionButton => "Click SESSION again to discard annotations",
            Destructive::Escape => "Press Esc again to discard annotations",
            Destructive::Quit => "Press Q again to discard annotations",
        }
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
/// Both the confirm-before-discard prompt and the transient feedback message
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

    pending_destructive: Expiring<Destructive>,
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
            pending_destructive: Expiring::new(),
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
        self.clear_pending_destructive();
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

    /// Records an annotation, clearing the redo stack.
    pub fn add_annotation(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
        self.redo_stack.clear();
    }

    /// Undoes the last annotation. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.annotations.pop() {
            Some(annotation) => {
                self.redo_stack.push(annotation);
                true
            }
            None => false,
        }
    }

    /// Redoes the last undone annotation.
    pub fn redo(&mut self) -> bool {
        match self.redo_stack.pop() {
            Some(annotation) => {
                self.annotations.push(annotation);
                true
            }
            None => false,
        }
    }

    /// Renders every annotation as a Markdown list.
    pub fn annotations_markdown(&self) -> String {
        self.annotations
            .iter()
            .map(|annotation| {
                let measurement = annotation.measurement_text();
                match annotation.mode {
                    // A distance already names both endpoints, so prefixing it
                    // with a single anchor coordinate would be redundant.
                    Mode::Distance => {
                        format!("- {}: {}", annotation.mode.export_title(), measurement)
                    }
                    _ => format!(
                        "- {} @ ({}, {}): {}",
                        annotation.mode.export_title(),
                        annotation.at.x.round(),
                        annotation.at.y.round(),
                        measurement
                    ),
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
        }
        self.clear_pending_destructive();
    }

    /// True when leaving session mode would throw away placed annotations.
    fn has_work_to_lose(&self) -> bool {
        self.session && !self.annotations.is_empty()
    }

    /// Runs a destructive action, or arms it for confirmation when it would
    /// discard annotations.
    pub fn request_destructive(&mut self, action: Destructive) {
        if !self.has_work_to_lose() {
            self.clear_pending_destructive();
            self.perform_destructive(action);
            return;
        }

        // A second press of the *same* action within the window confirms it;
        // a different action, or the same one after the window, re-arms
        // rather than firing.
        if self.pending_destructive.get() == Some(&action) {
            self.clear_pending_destructive();
            self.perform_destructive(action);
            return;
        }

        self.pending_destructive.set(action, CONFIRM_WINDOW);
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
            Destructive::Quit => self.push(Command::Quit),
        }
    }

    pub fn clear_pending_destructive(&mut self) {
        self.pending_destructive.clear();
    }

    /// The confirmation prompt to display, if one is armed and unexpired.
    pub fn destructive_prompt(&mut self) -> Option<&'static str> {
        self.pending_destructive.get().map(|action| action.prompt())
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
    fn shrink_to_fit_snaps_like_the_dial_it_offers() {
        // Regression: it showed a snap-distance slider, but snapping was never
        // applied in that mode, so the control was inert.
        assert!(Mode::ShrinkToFit.snaps());
        assert_eq!(Mode::ShrinkToFit.primary_control(), Control::SnapDistance);
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
            (Mode::ShrinkToFit, 12.0),
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
    fn leaving_session_mode_with_annotations_asks_for_confirmation() {
        let mut s = state();
        s.set_session(true);
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));

        s.request_destructive(Destructive::ToggleSession);
        assert!(s.session, "first press must not discard anything");
        assert_eq!(
            s.destructive_prompt(),
            Some("Press Tab again to discard annotations")
        );

        s.request_destructive(Destructive::ToggleSession);
        assert!(!s.session, "second press confirms");
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn a_different_destructive_action_re_arms_rather_than_confirming() {
        let mut s = state();
        s.set_session(true);
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));

        s.request_destructive(Destructive::ToggleSession);
        // Pressing Q now must not be treated as confirming the Tab.
        s.request_destructive(Destructive::Quit);
        assert!(s.session, "Q must not confirm a pending Tab");
        assert!(!s.take_commands().contains(&Command::Quit));
        assert_eq!(
            s.destructive_prompt(),
            Some("Press Q again to discard annotations")
        );

        s.request_destructive(Destructive::Quit);
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
    fn a_repeat_after_the_confirmation_expired_re_arms_instead() {
        let mut s = state();
        s.set_session(true);
        s.add_annotation(Annotation::test_rect(0.0, 0.0, 10.0, 10.0));
        // Armed, but already past its window, with nothing having polled it.
        s.pending_destructive
            .set(Destructive::ToggleSession, Duration::ZERO);

        s.request_destructive(Destructive::ToggleSession);
        assert!(
            s.session,
            "a late second press must not discard the annotations"
        );
        assert_eq!(
            s.destructive_prompt(),
            Some("Press Tab again to discard annotations")
        );
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
