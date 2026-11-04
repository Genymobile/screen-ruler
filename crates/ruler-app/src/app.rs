//! The controller: owns the state machine, one [`Surface`] per monitor and
//! the overlay windows, turns window callbacks into state changes, carries
//! out the commands the state emits, and pushes the result back into the
//! windows' properties.
//!
//! Everything runs on the Slint event loop. The only other thread is the
//! edge analysis, whose results come back through
//! `slint::invoke_from_event_loop` and a thread-local handle to the app.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use image::RgbaImage;
use ruler_core::analysis::Analysis;
use ruler_core::capture;
use ruler_core::clipboard::{self, Copied, Payload};
use ruler_core::color::{KernelCache, Sample};
use ruler_core::edges;
use ruler_core::geometry::{Point, Rect};
use ruler_core::state::{
    format_distance, format_size, shows_delta_breakdown, Annotation, AnnotationKind, Command,
    Control, Destructive, DragState, Mode, RulerState, HELP_FADE, MODE_COUNT,
};
use slint::platform::Key;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::analysis;
use crate::export::{self, Placement};
use crate::input::{self, Wheel};
use crate::surface::{LogicalRays, Surface};
use crate::{AnnotationMark, Dial, MarkKind, Overlay, Rays};

/// Redraw interval while something animates (the edge preview and help
/// fades), while a session message is up, until it expires, and while a
/// discard is held.
const FRAME: Duration = Duration::from_millis(16);

pub type Shared = Rc<RefCell<App>>;

thread_local! {
    /// The running app, for work that finishes off the event loop and has
    /// to come back to it: `invoke_from_event_loop` closures must be `Send`,
    /// which the app itself is not.
    static APP: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

fn with_app(f: impl FnOnce(&mut App)) {
    // Cloned out first, so the thread-local is not borrowed while `f` runs.
    if let Some(app) = APP.with(|slot| slot.borrow().clone()) {
        f(&mut app.borrow_mut());
    }
}

pub struct App {
    state: RulerState,
    surfaces: Vec<Surface>,
    windows: Vec<Overlay>,
    /// Monitor the pointer is over, if any. Live marks are drawn only there.
    hovered: Option<usize>,
    /// Where the left button went down, for telling clicks from drags.
    press: Option<(usize, f32, f32)>,
    /// The selection as it was before the current press, restored if the
    /// press turns out to be a confirming click.
    previous_drag: Option<DragState>,
    /// Where each window sits in the desktop's frame, by monitor, taken when
    /// an export press starts: what lets the export region span screens.
    export_layout: Vec<Option<Placement>>,
    wheel: Wheel,
    /// Sampling kernels, cached per radius.
    kernels: KernelCache,
    /// Thresholds for the next analysis; `None` derives them from the
    /// sensitivity. Only the command line sets this, for the first run.
    thresholds: Option<(u16, u16)>,
    /// Incremented per analysis request; results from older ones are stale.
    generation: u64,
    /// When the latest re-analysis landed, for the edge preview flash.
    preview_since: Option<Instant>,
    /// Rendered edge maps, built only once something shows them.
    edge_images: Vec<Option<slint::Image>>,
    /// Edges found across all monitors, for the sensitivity readout.
    edge_count: Option<usize>,
    /// Per-window models, kept and updated in place: replacing them would
    /// rebuild the rows, and a slider being dragged would lose its grab.
    dial_models: Vec<Rc<VecModel<Dial>>>,
    help_models: Vec<Rc<VecModel<SharedString>>>,
    /// Each window's placed annotations.
    annotation_models: Vec<Rc<VecModel<AnnotationMark>>>,
    recompute_timer: Timer,
    frame_timer: Timer,
}

impl App {
    /// Takes ownership of the windows and surfaces, wires every window's
    /// callbacks, and starts the first analysis.
    pub fn start(
        mut state: RulerState,
        surfaces: Vec<Surface>,
        windows: Vec<Overlay>,
        thresholds: Option<(u16, u16)>,
    ) -> Shared {
        let count = surfaces.len();
        // The help stays up until it is dismissed, rather than timing out
        // before it has been read.
        state.help_auto_hide_at = None;
        let app = Rc::new(RefCell::new(App {
            state,
            surfaces,
            windows,
            hovered: None,
            press: None,
            previous_drag: None,
            export_layout: Vec::new(),
            wheel: Wheel::default(),
            kernels: KernelCache::new(),
            thresholds,
            generation: 0,
            preview_since: None,
            edge_images: vec![None; count],
            edge_count: None,
            dial_models: (0..count).map(|_| Rc::new(VecModel::default())).collect(),
            help_models: (0..count).map(|_| Rc::new(VecModel::default())).collect(),
            annotation_models: (0..count).map(|_| Rc::new(VecModel::default())).collect(),
            recompute_timer: Timer::default(),
            frame_timer: Timer::default(),
        }));
        APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));

        {
            let app = app.borrow();
            let tooltips: Vec<SharedString> = Mode::ALL
                .iter()
                .map(|mode| format!("{}  ({})", mode.label(), mode.digit()).into())
                .collect();
            for (monitor, window) in app.windows.iter().enumerate() {
                wire(window, monitor);
                window.set_mode_tooltips(ModelRc::new(VecModel::from(tooltips.clone())));
                window.set_dials(ModelRc::from(app.dial_models[monitor].clone()));
                window.set_help_lines(ModelRc::from(app.help_models[monitor].clone()));
                window.set_annotations(ModelRc::from(app.annotation_models[monitor].clone()));
            }
        }
        let mut started = app.borrow_mut();
        started.analyse();
        started.render();
        drop(started);
        app
    }

    // ------------------------------------------------------------- input

    fn pointer_moved(&mut self, monitor: usize, x: f32, y: f32) {
        if let Some(width) = self.window_logical_width(monitor) {
            self.surfaces[monitor].reconcile_scale(width);
        }
        // A drag belongs to the monitor it started on: another window's
        // coordinates are in a different space, so they cannot extend it.
        if self.state.drag.active && self.state.drag.monitor != monitor {
            return;
        }
        if self.state.export_drag.active && self.state.export_drag.monitor != monitor {
            return;
        }
        self.hovered = Some(monitor);
        self.state.pointer = Some(Point { monitor, x, y });
        self.state.active_monitor = monitor;
        self.refresh_derived();
        if self.state.drag.active {
            if let Some(point) = self.placement() {
                self.state.drag.end = (point.x, point.y);
            }
        }
        // Unsnapped, and possibly outside the window, onto other screens:
        // the window keeps getting the moves of a press made on it.
        if self.state.export_drag.active {
            self.state.export_drag.end = (x, y);
        }
        self.render();
    }

    /// Recomputes what the current mode derives from the pointer: the
    /// snapped point and the container under it.
    fn refresh_derived(&mut self) {
        let Some(pointer) = self.state.pointer else {
            return;
        };
        let surface = &self.surfaces[pointer.monitor];
        self.state.snapped = self
            .state
            .mode
            .snaps()
            .then(|| surface.snap(pointer.x, pointer.y, self.state.snap_distance))
            .flatten()
            .map(|(x, y)| Point {
                monitor: pointer.monitor,
                x,
                y,
            });
        self.state.container = (self.state.mode == Mode::Container)
            .then(|| surface.container_at(pointer.x, pointer.y))
            .flatten();
        self.state.sample = (self.state.mode == Mode::ColorPicker).then(|| {
            let ((x, y), sample) = surface.sample(
                &mut self.kernels,
                pointer.x,
                pointer.y,
                self.state.color_radius,
            );
            (
                Point {
                    monitor: pointer.monitor,
                    x,
                    y,
                },
                sample,
            )
        });
    }

    /// Where something placed now would land: the snapped point when the
    /// cursor is near an edge, otherwise the pointer itself.
    fn placement(&self) -> Option<Point> {
        self.state.snapped.or(self.state.pointer)
    }

    fn pointer_left(&mut self, monitor: usize) {
        if self.hovered == Some(monitor) {
            self.hovered = None;
            self.render();
        }
    }

    fn pointer_pressed(&mut self, monitor: usize, x: f32, y: f32) {
        // Starting a measurement is a sign the help has been read.
        self.dismiss_help();
        self.press = Some((monitor, x, y));
        if self.state.export_armed {
            // The region is the screens' own pixels, so nothing snaps.
            self.state.export_drag = DragState::pressed(monitor, (x, y), (x, y));
            self.export_layout = self.lay_out_export(monitor);
            self.render();
        } else if self.state.mode.is_rect_selection() {
            // Kept aside: if this press turns out to be a click, it confirms
            // the selection a previous drag left rather than starting anew.
            self.previous_drag = Some(self.state.drag);
            let start = self
                .placement()
                .filter(|p| p.monitor == monitor)
                .map_or((x, y), |p| (p.x, p.y));
            self.state.drag = DragState::pressed(monitor, start, (x, y));
            self.render();
        }
    }

    fn pointer_released(&mut self, monitor: usize, x: f32, y: f32) {
        let Some((pressed_on, px, py)) = self.press.take() else {
            return;
        };
        // Acting on release, not press: a click is only a click once the
        // button comes back up without the pointer having travelled.
        let click = pressed_on == monitor && input::is_click((px, py), (x, y));
        if self.state.export_armed {
            self.finish_export(click);
        } else if self.state.mode.is_rect_selection() {
            self.finish_drag(click);
        } else if click {
            self.click(monitor, x, y);
        }
        self.process_commands();
        self.render();
    }

    fn finish_drag(&mut self, click: bool) {
        let previous = self.previous_drag.take().unwrap_or_default();
        // In a session a selection is placed as soon as it is dragged, so a
        // click has nothing to confirm.
        if self.state.session {
            if !click {
                if let Some(rect) = self.state.drag.rect() {
                    self.annotate_rect(self.fitted(rect));
                }
            }
            self.state.drag = DragState::default();
            return;
        }
        if click {
            self.state.drag = previous;
            self.confirm_selection();
            return;
        }
        self.state.drag.active = false;
        self.state.drag.has_selection = true;
        if self.state.mode == Mode::ShrinkToFit {
            if let Some(rect) = self.state.drag.rect() {
                let shrunk = self.surfaces[rect.monitor].shrink(rect);
                self.state.drag.start = (shrunk.x, shrunk.y);
                self.state.drag.end = (shrunk.x + shrunk.width, shrunk.y + shrunk.height);
            }
        }
    }

    /// Copies the export region as an image: the whole screen under a click,
    /// or the dragged region, unless it is under a pixel on a side, which
    /// counts as a click too.
    ///
    /// The drag ends on a screen. If the layout puts its end on none, the
    /// layout is wrong: the region is then cut at the start screen's edge
    /// rather than stitched from misplaced screens.
    fn finish_export(&mut self, click: bool) {
        let start = self.state.export_drag.monitor;
        let Some(screen) = self.window_rect(start) else {
            self.state.cancel_export();
            return;
        };
        if !export::lands_on_a_window(&self.export_layout, start, self.state.export_drag.end) {
            eprintln!(
                "screen-ruler: the drag ended off every screen the image export knows the place \
                 of, so the image stops at the edge of the screen where the drag started"
            );
            self.export_layout = self.alone(&self.export_layout, start);
        }
        if click {
            self.state.export_region(screen);
        } else {
            self.state.finish_export(screen);
        }
    }

    /// `rect` as the current mode selects it: tightened onto its content in
    /// shrink-to-fit, as dragged otherwise.
    fn fitted(&self, rect: Rect) -> Rect {
        if self.state.mode == Mode::ShrinkToFit {
            self.surfaces[rect.monitor].shrink(rect)
        } else {
            rect
        }
    }

    /// Forgets the press in progress and the drag or selection it would
    /// finish, so that its release neither clicks, confirms nor places
    /// anything.
    fn cancel_drag(&mut self) {
        self.press = None;
        self.previous_drag = None;
        self.state.drag = DragState::default();
    }

    /// Copies a finished rectangle selection and quits. Quick mode only: a
    /// session places a selection as soon as it is dragged.
    fn confirm_selection(&mut self) {
        if self.state.drag.has_selection && !self.state.session {
            self.state.copy_and_quit(None);
        }
    }

    fn click(&mut self, monitor: usize, x: f32, y: f32) {
        if self.state.session {
            self.session_click(monitor, x, y);
            return;
        }
        match self.state.mode {
            Mode::Crosshair => {
                let size = self.surfaces[monitor].rays_at(x, y).map(|r| r.size());
                self.state.copy_and_quit(size);
            }
            Mode::Container | Mode::ColorPicker => self.state.copy_and_quit(None),
            Mode::Distance => match self.state.distance_anchor {
                // The second point: measure to it and go.
                Some(anchor) if anchor.monitor == monitor => self.state.copy_and_quit(None),
                // The first point, or a new start on another monitor.
                _ => self.state.distance_anchor = self.placement().filter(|p| p.monitor == monitor),
            },
            Mode::RectDrag | Mode::ShrinkToFit => {}
        }
    }

    /// A click in session mode places the current measurement as an
    /// annotation, instead of copying it and quitting.
    fn session_click(&mut self, monitor: usize, x: f32, y: f32) {
        match self.state.mode {
            Mode::Crosshair => {
                if let Some(rays) = self.surfaces[monitor].rays_at(x, y) {
                    let kind = AnnotationKind::Crosshair {
                        north: y - rays.north,
                        south: y + rays.south,
                        west: x - rays.west,
                        east: x + rays.east,
                    };
                    self.place(Point { monitor, x, y }, kind);
                }
            }
            Mode::Container => {
                if let Some(rect) = self.state.container.filter(|r| r.monitor == monitor) {
                    self.annotate_rect(rect);
                }
            }
            Mode::ColorPicker => {
                if let Some((at, sample)) = self
                    .state
                    .sample
                    .clone()
                    .filter(|(p, _)| p.monitor == monitor)
                {
                    let radius = self.state.color_radius;
                    self.place(at, AnnotationKind::Color { sample, radius });
                }
            }
            Mode::Distance => {
                let b = self.placement().filter(|p| p.monitor == monitor);
                match (self.state.distance_anchor, b) {
                    (Some(a), Some(b)) if a.monitor == monitor => {
                        if (a.x, a.y) == (b.x, b.y) {
                            // A zero-length distance measures nothing: A
                            // stays, so that B can be placed again.
                            self.state.show_feedback("Point B is on point A");
                        } else {
                            self.state.distance_anchor = None;
                            let kind = AnnotationKind::Distance {
                                to_x: b.x,
                                to_y: b.y,
                            };
                            self.place(a, kind);
                        }
                    }
                    // The first point, or a new start on another monitor.
                    _ => self.state.distance_anchor = b,
                }
            }
            // Placed when the drag is released, in `finish_drag`.
            Mode::RectDrag | Mode::ShrinkToFit => {}
        }
    }

    /// Places an annotation of the current mode at `at`.
    fn place(&mut self, at: Point, kind: AnnotationKind) {
        self.state.add_annotation(Annotation {
            at,
            mode: self.state.mode,
            kind,
        });
    }

    /// Places a rectangle annotation, if it is [`placeable`].
    fn annotate_rect(&mut self, rect: Rect) {
        if !placeable(&rect) {
            return;
        }
        let at = Point {
            monitor: rect.monitor,
            x: rect.x,
            y: rect.y,
        };
        let kind = AnnotationKind::Rect {
            width: rect.width,
            height: rect.height,
        };
        self.place(at, kind);
    }

    /// Enters session mode, or asks to leave it (held to confirm when that
    /// would discard annotations). Tab and the SESSION button.
    fn toggle_session(&mut self, via: Destructive) {
        if self.state.session {
            self.request_destructive(via);
        } else {
            self.state.set_session(true);
            // A press, a drag or a selection waiting for its confirming click
            // belongs to quick mode: finished in the session, it would place
            // an annotation, and a selection would linger with a hint about
            // copying and quitting.
            self.cancel_drag();
            self.state
                .show_feedback("Session mode: measurements stay on screen");
        }
    }

    /// Runs a destructive action, or starts holding it when it has to be
    /// held to confirm.
    fn request_destructive(&mut self, action: Destructive) {
        self.cancel_drag_if_session_or_export_ends(|state| state.request_destructive(action));
    }

    /// Runs `change`, which may leave the session or disarm the image
    /// export. If it does either, the press in progress goes too: released in
    /// quick mode, it would leave a selection behind, or copy and quit, and
    /// released with the export disarmed, it would place an annotation.
    fn cancel_drag_if_session_or_export_ends<R>(
        &mut self,
        change: impl FnOnce(&mut RulerState) -> R,
    ) -> R {
        let (session, armed) = (self.state.session, self.state.export_armed);
        let result = change(&mut self.state);
        if self.state.session != session || (armed && !self.state.export_armed) {
            self.cancel_drag();
        }
        result
    }

    /// Ends a discard's hold without confirming it: a key or the SESSION
    /// button was let go, or the window lost the focus, and with it the
    /// release it would have reported.
    fn release_destructive(&mut self) {
        self.state.release_destructive();
        self.render();
    }

    /// Confirms a discard once it has been held long enough, on the frame
    /// timer.
    fn advance_hold(&mut self) {
        self.cancel_drag_if_session_or_export_ends(|state| state.advance_hold(Instant::now()));
        self.process_commands();
    }

    /// The SESSION button went down. It acts on the press, not the click, so
    /// that holding it can confirm a discard.
    fn session_pressed(&mut self) {
        self.dismiss_help();
        self.toggle_session(Destructive::SessionButton);
        self.process_commands();
        self.render();
    }

    fn markdown_clicked(&mut self) {
        self.copy_markdown();
        self.process_commands();
        self.render();
    }

    /// Copies every annotation as Markdown, then quits: the MD button, and
    /// Ctrl+C in a session.
    fn copy_markdown(&mut self) {
        self.state.cancel_hold();
        self.state.copy_annotations();
    }

    fn image_clicked(&mut self) {
        self.toggle_export();
        self.process_commands();
        self.render();
    }

    /// Arms the image export, or disarms it: the IMG button, and
    /// Ctrl+Shift+C in a session. A press in progress goes, so that its
    /// release neither exports nor places anything.
    fn toggle_export(&mut self) {
        self.cancel_drag();
        self.state.toggle_export();
    }

    /// Switches mode, which disarms the image export: the number keys and
    /// the mode buttons.
    fn set_mode(&mut self, mode: Mode) {
        self.cancel_drag_if_session_or_export_ends(|state| state.set_mode(mode));
        self.refresh_derived();
    }

    /// Undoes the last annotation, which disarms the image export when it
    /// leaves none. Returns false when there is nothing to undo.
    fn undo(&mut self) -> bool {
        self.cancel_drag_if_session_or_export_ends(RulerState::undo)
    }

    fn mode_clicked(&mut self, index: usize) {
        if let Some(mode) = Mode::from_index(index) {
            self.set_mode(mode);
            self.render();
        }
    }

    fn help_clicked(&mut self) {
        self.state.toggle_help();
        self.render();
    }

    /// The window manager asked to close a window: quits, or, with
    /// annotations to lose, holds while close requests keep coming, as from
    /// a held Alt+F4; see `CLOSE_HOLD_GAP`.
    fn close_requested(&mut self) {
        self.state.request_close();
        self.process_commands();
        self.render();
    }

    /// Fades the help out, when it is showing and not already fading.
    fn dismiss_help(&mut self) {
        if self.state.help_visible && self.state.help_auto_hide_at.is_none() {
            self.state.help_auto_hide_at = Some(Instant::now());
            self.animate();
            self.render();
        }
    }

    fn dial_changed(&mut self, index: usize, value: f32) {
        if let Some(&control) = self.state.mode.controls().get(index) {
            self.state.set_control_value(control, value.round());
            self.refresh_derived();
            self.process_commands();
            self.render();
        }
    }

    fn wheel(&mut self, delta: f32) {
        let notches = self.wheel.feed(delta);
        if notches != 0 {
            self.state.adjust_by_wheel(notches as f32);
            // A new snap radius moves the snapped point right away.
            self.refresh_derived();
            self.process_commands();
            self.render();
        }
    }

    /// Returns whether the key was handled.
    fn key(&mut self, text: &str, ctrl: bool, shift: bool, repeat: bool) -> bool {
        let is = |key: Key| text == SharedString::from(key).as_str();
        let digit = text.parse::<usize>().ok().filter(|_| text.len() == 1);
        let quits = !ctrl && text.eq_ignore_ascii_case("q");
        // With Ctrl held, C can arrive as its control character U+0003.
        let is_c = text.eq_ignore_ascii_case("c") || text == "\u{3}";
        // In a session, Ctrl+Shift+C toggles the image export instead of
        // copying the Markdown.
        let exports = ctrl && shift && self.state.session && is_c;
        let copies = ctrl && !exports && is_c;

        // A repeat must neither restart a hold, which would then never
        // complete, nor run an action again: with nothing to lose, a held
        // Tab would toggle the session on and off, and a held Esc would
        // leave the session, then quit. A held Ctrl+C would copy again and
        // again, and a held Ctrl+Shift+C would arm and disarm the export.
        let discards = is(Key::Tab) || is(Key::Escape) || quits;
        if repeat && (discards || copies || exports) {
            return true;
        }

        let handled = if let Some(mode) = digit.and_then(Mode::from_digit).filter(|_| !ctrl) {
            self.set_mode(mode);
            true
        } else if is(Key::Escape) {
            // Steps back one level at a time: the image export, a drag in
            // progress, a half-placed distance or a pending selection before
            // leaving.
            if self.state.export_armed {
                self.cancel_drag();
                self.state.cancel_export();
            } else if self.state.drag.active {
                self.cancel_drag();
            } else if self.state.distance_anchor.is_some() {
                self.state.distance_anchor = None;
            } else if self.state.drag.has_selection {
                self.state.drag.has_selection = false;
            } else {
                self.request_destructive(Destructive::Escape);
            }
            true
        } else if is(Key::Tab) {
            self.toggle_session(Destructive::ToggleSession);
            true
        } else if self.state.session && (text.eq_ignore_ascii_case("z") || text == "\u{1a}") {
            // Ctrl+Z or Z undoes, Ctrl+Shift+Z redoes. A half-placed
            // distance goes first, as with Esc. With Ctrl held, Z can arrive
            // as its control character U+001A.
            if ctrl && shift {
                if !self.state.redo() {
                    self.state.show_feedback("Nothing to redo");
                }
            } else if self.state.distance_anchor.is_some() {
                self.state.distance_anchor = None;
            } else if !self.undo() {
                self.state.show_feedback("Nothing to undo");
            }
            true
        } else if is(Key::Return) {
            if self.state.mode.is_rect_selection() {
                self.confirm_selection();
            }
            true
        } else if !ctrl && (text == "?" || text.eq_ignore_ascii_case("h")) {
            self.state.toggle_help();
            true
        } else if quits {
            self.request_destructive(Destructive::Quit);
            true
        } else if exports {
            self.toggle_export();
            true
        } else if copies {
            if self.state.session {
                self.copy_markdown();
            } else {
                let size = self.live_crosshair().map(|(_, rays)| rays.size());
                self.state.copy_and_quit(size);
            }
            true
        } else {
            false
        };
        self.process_commands();
        self.render();
        handled
    }

    // ---------------------------------------------------------- commands

    fn process_commands(&mut self) {
        for command in self.state.take_commands() {
            match command {
                Command::Quit => {
                    let _ = slint::quit_event_loop();
                }
                Command::CopyTextAndQuit(text) => {
                    let failure = report_copy(clipboard::copy_text(&text), Payload::Text);
                    match stay_open_message(failure, self.state.session) {
                        Some(message) => self.state.show_feedback(&message),
                        None => {
                            // Print the text, so the measurement can be
                            // scripted, and quit. On a quick-mode failure the
                            // windows close before a message could be read, so
                            // problems only go to stderr.
                            println!("{text}");
                            let _ = slint::quit_event_loop();
                        }
                    }
                }
                Command::Recompute => {
                    self.recompute_timer
                        .start(TimerMode::SingleShot, analysis::DEBOUNCE, || {
                            with_app(App::analyse)
                        });
                }
                Command::CopyImageAndQuit(rect) => self.schedule_image_copy(rect),
            }
        }
    }

    /// Snapshots every window `rect` covers, on the next turn of the event
    /// loop, cuts each one's part of `rect` out of its snapshot, and copies
    /// them stitched into one image. `rect` is on its monitor's window, in
    /// its logical px, and runs onto the other screens the export layout
    /// places.
    ///
    /// Deferred, because the snapshot renders the window, and rendering runs
    /// change handlers that call `with_app`: taken while the app is borrowed,
    /// as it is here, the snapshot would panic.
    fn schedule_image_copy(&mut self, rect: Rect) {
        // `export::composite` then uses the start screen alone.
        if export::leaves_out_overlapping_screens(rect, &self.export_layout) {
            eprintln!(
                "screen-ruler: two screens the image export places overlap, as mirrored \
                 displays do, so the image stops at the edge of the screen where the drag \
                 started"
            );
        }
        let Some(composite) = export::composite(rect, &self.export_layout) else {
            self.copy_image_and_quit(Err("the region is outside the window".to_string()));
            return;
        };
        // Weak handles: a window gone by then has nothing left to copy.
        let windows: Vec<_> = composite
            .parts
            .iter()
            .map(|part| {
                let window = self.windows[part.monitor].as_weak();
                let capture_size = self.surfaces[part.monitor].image.dimensions();
                (window, capture_size)
            })
            .collect();
        Timer::single_shot(Duration::ZERO, move || {
            let images = composite
                .parts
                .iter()
                .zip(&windows)
                .map(|(part, (window, capture_size))| {
                    let window = window.upgrade().ok_or("the window is gone")?;
                    snapshot_region(&window, part.region, *capture_size)
                })
                .collect::<Result<Vec<_>, String>>();
            let image = images.map(|images| export::stitch(&composite, &images));
            with_app(|app| app.copy_image_and_quit(image));
        });
    }

    /// Copies the export image, or reports why there is none, and quits,
    /// unless that failed: a session then stays open with the message.
    fn copy_image_and_quit(&mut self, image: Result<RgbaImage, String>) {
        let failure = match image {
            Ok(image) => report_copy(clipboard::copy_image(&image), Payload::Png),
            Err(e) => Some(report_image_failure(&e)),
        };
        match stay_open_message(failure, self.state.session) {
            Some(message) => {
                self.state.show_feedback(&message);
                self.render();
            }
            None => {
                let _ = slint::quit_event_loop();
            }
        }
    }

    // ---------------------------------------------------------- analysis

    fn analyse(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        let thresholds = self
            .thresholds
            .take()
            .unwrap_or_else(|| edges::sensitivity_to_thresholds(self.state.sensitivity));
        let images = self.surfaces.iter().map(|s| s.image.clone()).collect();
        analysis::spawn(images, thresholds, move |maps| {
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|app| app.analysed(generation, maps));
            });
        });
    }

    fn analysed(&mut self, generation: u64, results: Vec<Analysis>) {
        if generation != self.generation {
            return;
        }
        let first = self.surfaces.iter().all(|s| s.edges.is_none());
        for ((surface, image), result) in self
            .surfaces
            .iter_mut()
            .zip(&mut self.edge_images)
            .zip(results)
        {
            surface.edges = Some(result.edges);
            surface.regions = Some(result.regions);
            *image = None;
        }
        self.edge_count = Some(
            self.surfaces
                .iter()
                .filter_map(|s| s.edges.as_ref())
                .map(|e| e.count())
                .sum(),
        );
        // Snapping and the container were derived from the old maps.
        self.refresh_derived();
        // Flash the new map, unless this is the start-up analysis nobody
        // asked to see.
        if !first {
            self.preview_since = Some(Instant::now());
            self.animate();
        }
        self.render();
    }

    /// Keeps frames coming while something fades (the edge preview, the
    /// start-up help), a session message is up, until it expires, or a
    /// discard is held; `tick` stops them once nothing does.
    fn animate(&mut self) {
        if !self.frame_timer.running() {
            self.frame_timer
                .start(TimerMode::Repeated, FRAME, || with_app(App::tick));
        }
    }

    fn tick(&mut self) {
        self.advance_hold();
        if let Some(since) = self.preview_since {
            if input::preview_finished(since.elapsed()) {
                self.preview_since = None;
            }
        }
        let help_fading = self.state.help_visible && self.state.help_auto_hide_at.is_some();
        if self.preview_since.is_none() && !help_fading && !self.session_needs_frames() {
            self.frame_timer.stop();
        }
        self.render();
    }

    /// Whether a discard is held, which frames confirm, or a session
    /// message is up. Messages expire on their own, so frames keep coming
    /// until they have, or they would stay on screen.
    fn session_needs_frames(&mut self) -> bool {
        self.state.holding()
            || self.state.destructive_prompt().is_some()
            || self.state.feedback_message().is_some()
    }

    // ------------------------------------------------------------ render

    /// The crosshair under the pointer, when it has something to show.
    fn live_crosshair(&self) -> Option<(Point, LogicalRays)> {
        let point = self.state.pointer?;
        if self.state.mode != Mode::Crosshair || self.hovered != Some(point.monitor) {
            return None;
        }
        let rays = self.surfaces[point.monitor].rays_at(point.x, point.y)?;
        Some((point, rays))
    }

    /// What `monitor`'s window should show right now.
    fn live(&self, monitor: usize) -> Live {
        let mut live = Live::default();
        let hovered = self.hovered == Some(monitor);
        let pointer = self.state.pointer.filter(|p| p.monitor == monitor);

        // While armed, the export replaces the measurement: only its region
        // shows, on every screen it covers, without a size, since the image
        // is what gets copied.
        if self.state.export_armed {
            live.rect = self
                .state
                .export_drag
                .rect()
                .and_then(|r| export::region_on(r, &self.export_layout, monitor));
            if monitor == self.state.active_monitor {
                live.hint = Some(
                    "Drag a region or click a screen to copy it as an image, then quit · Esc to cancel",
                );
            }
            return live;
        }

        let analysing = hovered && self.surfaces[monitor].edges.is_none();
        if analysing && self.state.mode.needs_edges() {
            if let Some(p) = pointer {
                live.chip = Some(("Detecting edges…".to_string(), p.x, p.y));
            }
            return live;
        }

        match self.state.mode {
            Mode::Crosshair => {
                if let Some((p, rays)) = self.live_crosshair().filter(|(p, _)| p.monitor == monitor)
                {
                    live.crosshair = Some((p.x, p.y, rays));
                    live.chip = Some((self.state.measurement_text(Some(rays.size())), p.x, p.y));
                }
            }
            Mode::RectDrag | Mode::ShrinkToFit => {
                let drag = &self.state.drag;
                if let Some(rect) = drag.rect().filter(|r| r.monitor == monitor) {
                    live.rect = Some(rect);
                    live.chip = Some((format_size(rect.width, rect.height), rect.x, rect.y));
                    if drag.has_selection && !drag.active {
                        live.hint = Some("Click or press Enter to copy · Esc to cancel");
                    }
                }
            }
            Mode::Container => {
                if let Some(rect) = self
                    .state
                    .container
                    .filter(|r| hovered && r.monitor == monitor)
                {
                    live.rect = Some(rect);
                    live.chip = Some((format_size(rect.width, rect.height), rect.x, rect.y));
                }
            }
            Mode::ColorPicker => {
                if let Some((p, sample)) = self
                    .state
                    .sample
                    .as_ref()
                    .filter(|(p, _)| hovered && p.monitor == monitor)
                {
                    live.color = Some(ColorMark {
                        x: p.x,
                        y: p.y,
                        radius: self.state.color_radius,
                        swatch: swatch(sample),
                        text: notations(sample),
                    });
                }
            }
            Mode::Distance => {
                let a = self.state.distance_anchor.filter(|a| a.monitor == monitor);
                let b = self.placement().filter(|b| hovered && b.monitor == monitor);
                if let (Some(a), Some(b)) = (a, b) {
                    let mark = DistanceMark::between((a.x, a.y), (b.x, b.y));
                    live.chip = Some((self.state.measurement_text(None), mark.mid.0, mark.mid.1));
                    live.chip_offset = mark.chip_offset;
                    live.distance = Some(mark);
                }
            }
        }

        // Always shown in the snapping modes, so the first point of a
        // two-point measurement can be aimed before anything snaps.
        if hovered && self.state.mode.snaps() {
            live.snapped = self.state.snapped.is_some();
            live.snap = self
                .placement()
                .filter(|p| p.monitor == monitor)
                .map(|p| (p.x, p.y));
        }
        // Until the first analysis lands nothing snaps; say so, unless a
        // measurement in progress already has the chip.
        if analysing && self.state.mode.snaps() && live.chip.is_none() {
            if let Some(p) = pointer {
                live.chip = Some(("Detecting edges…".to_string(), p.x, p.y));
            }
        }
        live
    }

    fn render(&mut self) {
        let opacity = input::edges_opacity(
            self.state.debug_edges,
            self.preview_since.map(|t| t.elapsed()),
        );
        let dragging = self.state.drag.active || self.state.export_drag.active;
        let (help, help_visible) = input::help_opacity(
            self.state.help_visible,
            self.state.help_auto_hide_at,
            HELP_FADE,
            Instant::now(),
        );
        if !help_visible && self.state.help_visible {
            // Faded out: hidden for good, until toggled back.
            self.state.help_visible = false;
            self.state.help_auto_hide_at = None;
        }
        let prompt = self.state.destructive_prompt().unwrap_or_default();
        let progress = self.state.hold_progress(Instant::now());
        let feedback = self.state.feedback_message().unwrap_or_default();
        if self.session_needs_frames() {
            self.animate();
        }

        for monitor in 0..self.windows.len() {
            let live = self.live(monitor);
            let window = &self.windows[monitor];

            window.set_edges_opacity(opacity);
            if opacity > 0.0 {
                if let (None, Some(map)) =
                    (&self.edge_images[monitor], &self.surfaces[monitor].edges)
                {
                    let image = analysis::edge_image(map);
                    window.set_edges(image.clone());
                    self.edge_images[monitor] = Some(image);
                }
            }

            window.set_show_crosshair(live.crosshair.is_some());
            if let Some((x, y, rays)) = live.crosshair {
                window.set_cursor_x(x);
                window.set_cursor_y(y);
                window.set_rays(Rays {
                    north: rays.north,
                    south: rays.south,
                    west: rays.west,
                    east: rays.east,
                });
            }

            window.set_show_rect(live.rect.is_some());
            // Eased when the rect jumps (a new container, a shrink), exact
            // while it follows a drag.
            window.set_rect_animated(!dragging);
            if let Some(rect) = live.rect {
                window.set_rect_x(rect.x);
                window.set_rect_y(rect.y);
                window.set_rect_width(rect.width);
                window.set_rect_height(rect.height);
            }

            window.set_show_snap(live.snap.is_some());
            window.set_snap_snapped(live.snapped);
            if let Some((x, y)) = live.snap {
                window.set_snap_x(x);
                window.set_snap_y(y);
            }

            window.set_show_color(live.color.is_some());
            if let Some(color) = &live.color {
                window.set_color_x(color.x);
                window.set_color_y(color.y);
                window.set_color_radius(color.radius);
                window.set_color_swatch(color.swatch);
                window.set_color_text(color.text.clone());
            }

            window.set_show_distance(live.distance.is_some());
            if let Some(distance) = &live.distance {
                window.set_distance_ax(distance.a.0);
                window.set_distance_ay(distance.a.1);
                window.set_distance_bx(distance.b.0);
                window.set_distance_by(distance.b.1);
                window.set_distance_legs(distance.legs);
                let (dx, dy) = distance.leg_texts();
                window.set_distance_dx(dx);
                window.set_distance_dy(dy);
            }

            window.set_chip_offset_x(live.chip_offset.0);
            window.set_chip_offset_y(live.chip_offset.1);
            match live.chip {
                Some((text, x, y)) => {
                    window.set_chip_text(text.into());
                    window.set_chip_x(x);
                    window.set_chip_y(y);
                }
                None => window.set_chip_text(SharedString::new()),
            }
            window.set_hint_text(live.hint.unwrap_or_default().into());

            window.set_session(self.state.session);
            window.set_export_armed(self.state.export_armed);
            sync(
                &self.annotation_models[monitor],
                self.state
                    .annotations
                    .iter()
                    .filter(|a| a.at.monitor == monitor)
                    .map(annotation_mark)
                    .collect(),
            );
            window.set_prompt_text(prompt.into());
            window.set_prompt_progress(progress);
            window.set_feedback_text(feedback.as_str().into());

            let chrome = monitor == self.state.active_monitor;
            window.set_show_chrome(chrome);
            if chrome {
                window.set_mode(self.state.mode.index() as i32);
                window.set_help_opacity(help);
                window.set_help_visible(self.state.help_visible);
                sync(&self.dial_models[monitor], self.dials());
                sync(
                    &self.help_models[monitor],
                    help_lines(self.state.mode, self.state.session),
                );
            }
        }
    }

    /// The current mode's dials, as the panel shows them.
    fn dials(&self) -> Vec<Dial> {
        self.state
            .mode
            .controls()
            .iter()
            .map(|&control| {
                let value = self.state.control_value(control);
                let readout = match (control, self.edge_count) {
                    (Control::Sensitivity, Some(edges)) => {
                        format!("{}  ·  {edges} edges", value.round())
                    }
                    (Control::Sensitivity, None) => format!("{}", value.round()),
                    _ => format!("{} px", value.round()),
                };
                Dial {
                    title: control.title().into(),
                    value,
                    max: control.max(),
                    readout: readout.into(),
                }
            })
            .collect()
    }

    /// The whole of `monitor`'s window, in its logical px.
    fn window_rect(&self, monitor: usize) -> Option<Rect> {
        let window = self.windows.get(monitor)?.window();
        let size = window.size().to_logical(window.scale_factor());
        Some(Rect {
            monitor,
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        })
    }

    fn window_logical_width(&self, monitor: usize) -> Option<f32> {
        let window = self.windows.get(monitor)?.window();
        let width = window.size().to_logical(window.scale_factor()).width;
        (width > 0.0).then_some(width)
    }

    // ------------------------------------------------------ export layout

    /// Where each window sits in the desktop's frame, for an export pressed
    /// on `start`'s window, by monitor.
    ///
    /// A window winit cannot tell the monitor of is left out. If that is the
    /// start window, it cannot be related to any other, so it stands alone:
    /// the region is then cut at its edge.
    fn lay_out_export(&mut self, start: usize) -> Vec<Option<Placement>> {
        // The capture scale of a window the pointer has not been over yet
        // is still the capture backend's guess.
        for monitor in 0..self.windows.len() {
            if let Some(width) = self.window_logical_width(monitor) {
                self.surfaces[monitor].reconcile_scale(width);
            }
        }
        let logical = capture::desktop_frame_is_logical();
        let layout: Vec<_> = (0..self.windows.len())
            .map(|monitor| self.frame_placement(monitor, logical))
            .collect();
        if layout.get(start).copied().flatten().is_some() {
            layout
        } else {
            self.alone(&layout, start)
        }
    }

    /// `layout` with only `start`'s window left in it, placed on its own if
    /// it has no place: that cuts the region at the start screen's edge.
    fn alone(&self, layout: &[Option<Placement>], start: usize) -> Vec<Option<Placement>> {
        let placement = layout
            .get(start)
            .copied()
            .flatten()
            .or_else(|| self.standalone_placement(start));
        (0..self.windows.len())
            .map(|monitor| placement.filter(|_| monitor == start))
            .collect()
    }

    /// Where `monitor`'s window sits in the desktop's frame, from the
    /// monitor winit says it is on. `logical` is whether that frame is in
    /// logical px ([`capture::desktop_frame_is_logical`]).
    fn frame_placement(&self, monitor: usize, logical: bool) -> Option<Placement> {
        let window = self.windows.get(monitor)?.window();
        let (origin, system_scale) = window
            .with_winit_window(|winit_window| {
                let screen = winit_window.current_monitor()?;
                let position = screen.position();
                let (x, y) = (f64::from(position.x), f64::from(position.y));
                let origin = if logical {
                    // winit reports a logical layout scaled by the
                    // monitor's own factor; dividing it back gives it.
                    let factor = screen.scale_factor();
                    (factor.is_finite() && factor > 0.0)
                        .then(|| ((x / factor) as f32, (y / factor) as f32))?
                } else {
                    (x as f32, y as f32)
                };
                // The window system's scale, which `SLINT_SCALE_FACTOR`
                // leaves as it is while it overrides the window's.
                Some((origin, winit_window.scale_factor() as f32))
            })
            .flatten()?;
        let units = export::frame_units(logical, window.scale_factor(), system_scale);
        self.placement_at(monitor, origin, units)
    }

    /// A place for `monitor`'s window with no relation to any other: at the
    /// frame's origin, one unit per logical px.
    fn standalone_placement(&self, monitor: usize) -> Option<Placement> {
        self.placement_at(monitor, (0.0, 0.0), 1.0)
    }

    /// `monitor`'s window at `origin` in the frame, with `units` frame units
    /// per logical px.
    fn placement_at(&self, monitor: usize, origin: (f32, f32), units: f32) -> Option<Placement> {
        let window = self.windows.get(monitor)?.window();
        let size = window.size().to_logical(window.scale_factor());
        let capture_scale = self.surfaces.get(monitor)?.geometry.sanitised_scale();
        Some(Placement {
            origin,
            units,
            density: capture_scale / units,
            size: (size.width, size.height),
        })
    }
}

/// Snapshots `window` with only its screenshot and placed annotations drawn,
/// and cuts `rect`, its part of the export region, out of it at the
/// resolution of the screen's capture, `capture_size` device px.
///
/// Never called while the app is borrowed: see `App::schedule_image_copy`.
fn snapshot_region(
    window: &Overlay,
    rect: Rect,
    capture_size: (u32, u32),
) -> Result<RgbaImage, String> {
    window.set_snapshot(true);
    let snapshot = window.window().take_snapshot();
    window.set_snapshot(false);
    let snapshot = snapshot.map_err(|e| format!("cannot take a snapshot of the window: {e}"))?;
    let scale = window.window().scale_factor();
    export::image(&snapshot, rect, scale, capture_size)
        .ok_or_else(|| "the region is outside the window".to_string())
}

/// Routes one window's callbacks to the app, tagged with its monitor.
fn wire(window: &Overlay, monitor: usize) {
    window.on_pointer_moved(move |x, y| with_app(|app| app.pointer_moved(monitor, x, y)));
    window.on_pointer_pressed(move |x, y| with_app(|app| app.pointer_pressed(monitor, x, y)));
    window.on_pointer_released(move |x, y| with_app(|app| app.pointer_released(monitor, x, y)));
    window.on_pointer_left(move || with_app(|app| app.pointer_left(monitor)));
    window.on_wheel(|delta| with_app(|app| app.wheel(delta)));
    window.on_mode_clicked(|index| with_app(|app| app.mode_clicked(index as usize)));
    window.on_dial_changed(|index, value| with_app(|app| app.dial_changed(index as usize, value)));
    window.on_help_clicked(|| with_app(App::help_clicked));
    window.on_help_hovered(|| with_app(App::dismiss_help));
    window.on_session_pressed(|| with_app(App::session_pressed));
    window.on_session_released(|| with_app(App::release_destructive));
    window.on_markdown_clicked(|| with_app(App::markdown_clicked));
    window.on_image_clicked(|| with_app(App::image_clicked));
    window.on_key(|text, ctrl, shift, repeat| {
        let mut handled = false;
        with_app(|app| handled = app.key(&text, ctrl, shift, repeat));
        handled
    });
    // Any release ends a hold, not only the held key's: a release can
    // report other text than its press (Shift+Tab gives Backtab, and the
    // case or layout can change in between).
    window.on_key_released(|| with_app(App::release_destructive));
    // A release made after the window lost the focus goes elsewhere.
    window.on_focus_lost(|| with_app(App::release_destructive));
    // A close request (Alt+F4, or the window manager) quits, for every
    // monitor at once, rather than closing this one window.
    window.window().on_close_requested(|| {
        with_app(App::close_requested);
        slint::CloseRequestResponse::KeepWindowShown
    });
}

/// Logs the details of a clipboard copy to stderr, and returns the message
/// to show on screen, if there is one.
///
/// Only a failed copy shows a message. A copy that may not last is a warning
/// on stderr alone: a clipboard manager, such as GNOME's, often keeps it, and
/// the app cannot tell, so on screen it would mostly be a false alarm.
/// `payload` picks the package that warning tells the user to install.
fn report_copy(result: Result<Copied, String>, payload: Payload) -> Option<String> {
    match result {
        Ok(Copied::Lasting) => None,
        Ok(Copied::MayNotLast) => {
            eprintln!(
                "screen-ruler: no clipboard helper for this copy, so it may not stay \
                 in the clipboard: it can go at once, or when screen-ruler quits; {}",
                clipboard::install_hint(payload)
            );
            None
        }
        Err(e) => {
            eprintln!("screen-ruler: could not copy to the clipboard: {e}");
            Some("Could not copy to the clipboard".to_string())
        }
    }
}

/// Logs why the export image could not be made, its snapshot or its crop,
/// and returns the message to show on screen.
fn report_image_failure(error: &str) -> String {
    eprintln!("screen-ruler: could not make the image: {error}");
    "Could not make the image".to_string()
}

/// The message to show if a copy keeps the app open, given the on-screen
/// message `report_copy` returned for it; `None` means the copy quits.
///
/// Every copy quits, except one that failed in a session: that stays open
/// with the message, so the annotations are not lost and the copy can be
/// retried. A failure in quick mode still quits, as there is nothing to lose.
fn stay_open_message(failure: Option<String>, session: bool) -> Option<String> {
    failure.filter(|_| session)
}

/// Updates `model` to `rows`, touching only the rows that changed.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() != rows.len() {
        model.set_vec(rows);
        return;
    }
    for (index, row) in rows.into_iter().enumerate() {
        if model.row_data(index).as_ref() != Some(&row) {
            model.set_row_data(index, row);
        }
    }
}

/// The shortcut help for `mode`, in quick or session mode.
fn help_lines(mode: Mode, session: bool) -> Vec<SharedString> {
    let click = if session {
        match mode {
            Mode::RectDrag => "Drag — place a rectangle",
            Mode::ShrinkToFit => "Drag — tighten onto the content and place it",
            Mode::Distance => "Click — place point A, then point B",
            Mode::Crosshair | Mode::Container | Mode::ColorPicker => {
                "Click — place the measurement"
            }
        }
    } else {
        match mode {
            Mode::RectDrag | Mode::ShrinkToFit => {
                "Drag — select  ·  then click or Enter — copy it, then quit"
            }
            Mode::Distance => "Click — place point A, then point B to copy, then quit",
            Mode::Crosshair | Mode::Container | Mode::ColorPicker => {
                "Click — copy the measurement, then quit"
            }
        }
    };
    let (copy, undo_or_session, leave) = if session {
        (
            "Ctrl+C — copy all annotations as Markdown, then quit",
            "Ctrl+Z or Z — undo  ·  Ctrl+Shift+Z — redo",
            "Tab or Esc — leave session mode  ·  Q — quit",
        )
    } else {
        (
            "Ctrl+C — copy the measurement, then quit",
            "Tab — session mode: place several annotations",
            "Esc or Q — quit",
        )
    };
    let mut lines: Vec<SharedString> = vec![
        format!("1–{MODE_COUNT} — switch measurement mode").into(),
        click.into(),
        copy.into(),
    ];
    if session {
        lines.push(
            "Ctrl+Shift+C — drag a region or click a screen to copy it as an image, then quit  ·  \
             Esc cancels"
                .into(),
        );
    }
    lines.extend([
        undo_or_session.into(),
        "Wheel — adjust the first slider".into(),
        "? or H — toggle this help (hover it to hide it)".into(),
        leave.into(),
    ]);
    lines
}

/// A placed annotation as its window draws it: the same marks and chip
/// as the live measurement it was placed from.
fn annotation_mark(annotation: &Annotation) -> AnnotationMark {
    let (x, y) = (annotation.at.x, annotation.at.y);
    let mut mark = AnnotationMark {
        x,
        y,
        chip_x: x,
        chip_y: y,
        chip_offset_x: CHIP_OFFSET.0,
        chip_offset_y: CHIP_OFFSET.1,
        ..AnnotationMark::default()
    };
    match &annotation.kind {
        AnnotationKind::Crosshair {
            north,
            south,
            west,
            east,
        } => {
            mark.kind = MarkKind::Crosshair;
            mark.text = annotation.measurement_text().into();
            mark.rays = Rays {
                north: y - north,
                south: south - y,
                west: x - west,
                east: east - x,
            };
        }
        AnnotationKind::Rect { width, height } => {
            mark.kind = MarkKind::Rect;
            mark.text = annotation.measurement_text().into();
            mark.width = *width;
            mark.height = *height;
        }
        AnnotationKind::Color { sample, radius } => {
            mark.kind = MarkKind::Color;
            mark.text = notations(sample);
            mark.radius = *radius;
            mark.swatch = swatch(sample);
        }
        AnnotationKind::Distance { to_x, to_y } => {
            let distance = DistanceMark::between((x, y), (*to_x, *to_y));
            mark.kind = MarkKind::Distance;
            mark.text = annotation.measurement_text().into();
            mark.bx = *to_x;
            mark.by = *to_y;
            mark.legs = distance.legs;
            (mark.dx, mark.dy) = distance.leg_texts();
            (mark.chip_x, mark.chip_y) = distance.mid;
            (mark.chip_offset_x, mark.chip_offset_y) = distance.chip_offset;
        }
    }
    mark
}

/// The colour of `sample`, for its swatch.
fn swatch(sample: &Sample) -> slint::Color {
    slint::Color::from_rgb_u8(sample.r, sample.g, sample.b)
}

/// The notations of `sample` (hex, rgb() and hsl()), one per line, as the
/// colour bubble stacks them, live or placed.
fn notations(sample: &Sample) -> SharedString {
    sample.notations().join("\n").into()
}

/// Whether a rectangle is worth placing as an annotation: anything but a
/// point. A line, under a pixel on one side only, still measures its length.
fn placeable(rect: &Rect) -> bool {
    rect.width >= 1.0 || rect.height >= 1.0
}

/// The live marks one window shows, in its logical px.
struct Live {
    crosshair: Option<(f32, f32, LogicalRays)>,
    rect: Option<Rect>,
    /// Whether the placement marker below is pulled onto an edge.
    snapped: bool,
    snap: Option<(f32, f32)>,
    color: Option<ColorMark>,
    distance: Option<DistanceMark>,
    /// Text and the point it is anchored to.
    chip: Option<(String, f32, f32)>,
    /// Where the chip sits relative to its anchor.
    chip_offset: (f32, f32),
    hint: Option<&'static str>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            crosshair: None,
            rect: None,
            snapped: false,
            snap: None,
            color: None,
            distance: None,
            chip: None,
            chip_offset: CHIP_OFFSET,
            hint: None,
        }
    }
}

/// The chip's usual place: just below and right of its anchor.
const CHIP_OFFSET: (f32, f32) = (14.0, 4.0);

struct ColorMark {
    x: f32,
    y: f32,
    radius: f32,
    swatch: slint::Color,
    /// Hex, rgb() and hsl(), one per line.
    text: SharedString,
}

/// A distance being measured from A to B.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DistanceMark {
    a: (f32, f32),
    b: (f32, f32),
    delta: (f32, f32),
    mid: (f32, f32),
    /// Whether the line is diagonal enough to show its dashed legs.
    legs: bool,
    /// Offsets the total chip off the line, along its normal, so it never
    /// sits on top of what it measures.
    chip_offset: (f32, f32),
}

impl DistanceMark {
    const CHIP_CLEARANCE: f32 = 14.0;

    fn between(a: (f32, f32), b: (f32, f32)) -> Self {
        let delta = (b.0 - a.0, b.1 - a.1);
        let length = (delta.0 * delta.0 + delta.1 * delta.1).sqrt();
        let chip_offset = if length > 0.0 {
            let normal = (-delta.1 / length, delta.0 / length);
            (
                normal.0 * Self::CHIP_CLEARANCE,
                normal.1 * Self::CHIP_CLEARANCE,
            )
        } else {
            CHIP_OFFSET
        };
        Self {
            a,
            b,
            delta,
            mid: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
            legs: shows_delta_breakdown(delta.0, delta.1),
            chip_offset,
        }
    }

    /// The lengths of the horizontal and vertical legs, as their chips show
    /// them.
    fn leg_texts(&self) -> (SharedString, SharedString) {
        (
            format_distance(self.delta.0.abs()).into(),
            format_distance(self.delta.1.abs()).into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Point {
        Point { monitor: 0, x, y }
    }

    #[test]
    fn a_placed_crosshair_draws_the_rays_it_was_measured_with() {
        // Stored as ray endpoints, drawn as lengths from the cursor.
        let annotation = Annotation {
            at: at(100.0, 50.0),
            mode: Mode::Crosshair,
            kind: AnnotationKind::Crosshair {
                north: 30.0,
                south: 80.0,
                west: 60.0,
                east: 150.0,
            },
        };
        let mark = annotation_mark(&annotation);
        assert_eq!(mark.kind, MarkKind::Crosshair);
        assert_eq!((mark.rays.north, mark.rays.south), (20.0, 30.0));
        assert_eq!((mark.rays.west, mark.rays.east), (40.0, 50.0));
        assert_eq!(mark.text, "90 \u{00D7} 50 px");
        assert_eq!((mark.chip_x, mark.chip_y), (100.0, 50.0));
    }

    #[test]
    fn a_placed_distance_puts_its_chip_off_the_middle_of_the_line() {
        let annotation = Annotation {
            at: at(0.0, 0.0),
            mode: Mode::Distance,
            kind: AnnotationKind::Distance {
                to_x: 100.0,
                to_y: 0.0,
            },
        };
        let mark = annotation_mark(&annotation);
        assert_eq!(mark.kind, MarkKind::Distance);
        assert_eq!((mark.bx, mark.by), (100.0, 0.0));
        assert_eq!((mark.chip_x, mark.chip_y), (50.0, 0.0));
        assert_eq!((mark.chip_offset_x, mark.chip_offset_y), (0.0, 14.0));
        assert!(!mark.legs);
    }

    #[test]
    fn a_placed_diagonal_distance_labels_both_legs() {
        let annotation = Annotation {
            at: at(0.0, 0.0),
            mode: Mode::Distance,
            kind: AnnotationKind::Distance {
                to_x: 100.0,
                to_y: 50.0,
            },
        };
        let mark = annotation_mark(&annotation);
        assert!(mark.legs);
        assert_eq!((mark.dx.as_str(), mark.dy.as_str()), ("100 px", "50 px"));
    }

    #[test]
    fn leg_texts_are_lengths_whatever_the_direction() {
        // B up and to the left of A: negative deltas, positive lengths.
        let mark = DistanceMark::between((100.0, 50.0), (0.0, 0.0));
        let (dx, dy) = mark.leg_texts();
        assert_eq!((dx.as_str(), dy.as_str()), ("100 px", "50 px"));
    }

    #[test]
    fn a_placed_rectangle_draws_its_size_from_its_corner() {
        let annotation = Annotation {
            at: at(10.0, 20.0),
            mode: Mode::RectDrag,
            kind: AnnotationKind::Rect {
                width: 100.0,
                height: 50.0,
            },
        };
        let mark = annotation_mark(&annotation);
        assert_eq!(mark.kind, MarkKind::Rect);
        assert_eq!((mark.x, mark.y), (10.0, 20.0));
        assert_eq!((mark.width, mark.height), (100.0, 50.0));
        assert_eq!(mark.text, "100 \u{00D7} 50 px");
        assert_eq!((mark.chip_x, mark.chip_y), (10.0, 20.0));
    }

    #[test]
    fn a_placed_colour_shows_every_notation_in_its_bubble() {
        let sample = Sample::from_rgb(255, 0, 0);
        let annotation = Annotation {
            at: at(5.0, 6.0),
            mode: Mode::ColorPicker,
            kind: AnnotationKind::Color {
                sample: sample.clone(),
                radius: 3.0,
            },
        };
        let mark = annotation_mark(&annotation);
        assert_eq!(mark.kind, MarkKind::Color);
        assert_eq!(mark.radius, 3.0);
        assert_eq!(mark.text, sample.notations().join("\n"));
    }

    #[test]
    fn a_placed_colour_swatch_is_the_opaque_sampled_colour() {
        let annotation = Annotation {
            at: at(5.0, 6.0),
            mode: Mode::ColorPicker,
            kind: AnnotationKind::Color {
                sample: Sample::from_rgb(230, 25, 94),
                radius: 0.0,
            },
        };
        let swatch = annotation_mark(&annotation).swatch;
        // The default colour is transparent: an unset swatch would vanish.
        assert_eq!(
            (swatch.red(), swatch.green(), swatch.blue(), swatch.alpha()),
            (230, 25, 94, 255)
        );
    }

    #[test]
    fn a_line_rectangle_is_placeable() {
        let line = Rect {
            monitor: 0,
            x: 10.0,
            y: 10.0,
            width: 200.0,
            height: 0.0,
        };
        assert!(placeable(&line));
    }

    #[test]
    fn a_point_rectangle_is_not_placeable() {
        let point = Rect {
            monitor: 0,
            x: 10.0,
            y: 10.0,
            width: 0.5,
            height: 0.5,
        };
        assert!(!placeable(&point));
    }

    #[test]
    fn colour_notations_stack_hex_rgb_then_hsl() {
        let text = notations(&Sample::from_rgb(230, 25, 94));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0], "#E6195E");
        assert_eq!(lines[1], "rgb(230, 25, 94)");
        assert!(lines[2].starts_with("hsl("), "{text}");
    }

    #[test]
    fn a_lasting_copy_needs_no_message() {
        assert_eq!(report_copy(Ok(Copied::Lasting), Payload::Text), None);
    }

    #[test]
    fn a_copy_that_may_not_last_shows_no_message() {
        assert_eq!(report_copy(Ok(Copied::MayNotLast), Payload::Text), None);
    }

    #[test]
    fn a_failed_copy_says_so() {
        assert_eq!(
            report_copy(Err("no display".to_string()), Payload::Text).as_deref(),
            Some("Could not copy to the clipboard")
        );
    }

    #[test]
    fn an_image_that_could_not_be_made_says_so() {
        assert_eq!(
            report_image_failure("no snapshot"),
            "Could not make the image"
        );
    }

    #[test]
    fn a_failed_session_copy_stays_open_with_its_message() {
        let failure = report_copy(Err("no display".to_string()), Payload::Text);
        assert_eq!(
            stay_open_message(failure, true).as_deref(),
            Some("Could not copy to the clipboard")
        );
    }

    #[test]
    fn a_session_copy_that_may_not_last_still_quits() {
        let failure = report_copy(Ok(Copied::MayNotLast), Payload::Text);
        assert_eq!(stay_open_message(failure, true), None);
    }

    #[test]
    fn a_failed_quick_copy_quits() {
        let failure = report_copy(Err("no display".to_string()), Payload::Text);
        assert_eq!(stay_open_message(failure, false), None);
    }

    #[test]
    fn a_successful_copy_quits() {
        for session in [false, true] {
            let failure = report_copy(Ok(Copied::Lasting), Payload::Text);
            assert_eq!(
                stay_open_message(failure, session),
                None,
                "session: {session}"
            );
        }
    }

    #[test]
    fn the_help_says_what_a_click_does_in_each_mode() {
        let quick = help_lines(Mode::Crosshair, false);
        assert!(quick.iter().any(|l| l.contains("then quit")));
        assert!(quick.iter().any(|l| l.starts_with("Tab")), "{quick:?}");

        for mode in Mode::ALL {
            let session = help_lines(mode, true);
            let click = session
                .iter()
                .find(|l| l.starts_with("Click") || l.starts_with("Drag"))
                .expect("a click line");
            assert!(
                !click.contains("then quit"),
                "{mode:?}: nothing quits on a click in a session: {session:?}"
            );
            assert!(session.iter().any(|l| l.contains("undo")), "{mode:?}");
        }
    }

    #[test]
    fn the_session_help_says_a_copy_quits() {
        let session = help_lines(Mode::Crosshair, true);
        assert!(
            session
                .iter()
                .any(|l| l.starts_with("Ctrl+C") && l.ends_with("then quit")),
            "{session:?}"
        );
    }

    #[test]
    fn the_session_help_names_the_undo_and_leave_keys() {
        let session = help_lines(Mode::Crosshair, true);
        assert!(
            session.iter().any(|l| l.starts_with("Ctrl+Z or Z")),
            "{session:?}"
        );
        assert!(
            session.iter().any(|l| l.starts_with("Tab or Esc")),
            "{session:?}"
        );
    }

    #[test]
    fn the_session_help_names_the_image_export_key() {
        let session = help_lines(Mode::Crosshair, true);
        assert!(
            session.iter().any(|l| l.starts_with("Ctrl+Shift+C")
                && l.contains("then quit")
                && l.ends_with("Esc cancels")),
            "{session:?}"
        );
    }

    #[test]
    fn the_quick_help_does_not_offer_the_image_export() {
        let quick = help_lines(Mode::Crosshair, false);
        assert!(
            !quick.iter().any(|l| l.contains("Ctrl+Shift+C")),
            "{quick:?}"
        );
    }

    #[test]
    fn the_quick_help_names_the_quit_keys() {
        let quick = help_lines(Mode::Crosshair, false);
        assert!(quick.iter().any(|l| l.starts_with("Esc or Q")), "{quick:?}");
    }

    #[test]
    fn legs_show_only_for_a_meaningfully_diagonal_line() {
        assert!(!DistanceMark::between((0.0, 0.0), (100.0, 4.0)).legs);
        assert!(DistanceMark::between((0.0, 0.0), (100.0, 50.0)).legs);
    }

    #[test]
    fn the_total_chip_sits_off_the_line() {
        // A horizontal line: the chip is pushed straight down, off it.
        let mark = DistanceMark::between((0.0, 0.0), (100.0, 0.0));
        assert_eq!(mark.mid, (50.0, 0.0));
        assert_eq!(mark.chip_offset, (0.0, 14.0));
        // A zero-length line falls back to the usual offset.
        assert_eq!(
            DistanceMark::between((5.0, 5.0), (5.0, 5.0)).chip_offset,
            CHIP_OFFSET
        );
    }
}
