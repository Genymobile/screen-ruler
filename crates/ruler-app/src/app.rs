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

use ruler_core::analysis::Analysis;
use ruler_core::clipboard;
use ruler_core::color::KernelCache;
use ruler_core::edges;
use ruler_core::geometry::{Point, Rect};
use ruler_core::state::{
    format_distance, format_size, shows_delta_breakdown, Command, Control, Destructive, DragState,
    Mode, RulerState, HELP_FADE, MODE_COUNT,
};
use slint::platform::Key;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::analysis;
use crate::input::{self, Wheel};
use crate::surface::{LogicalRays, Surface};
use crate::{Dial, Overlay, Rays};

/// Redraw interval while something animates (the edge preview fade).
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
            wheel: Wheel::default(),
            kernels: KernelCache::new(),
            thresholds,
            generation: 0,
            preview_since: None,
            edge_images: vec![None; count],
            edge_count: None,
            dial_models: (0..count).map(|_| Rc::new(VecModel::default())).collect(),
            help_models: (0..count).map(|_| Rc::new(VecModel::default())).collect(),
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
        self.hovered = Some(monitor);
        self.state.pointer = Some(Point { monitor, x, y });
        self.state.active_monitor = monitor;
        self.refresh_derived();
        if self.state.drag.active {
            if let Some(point) = self.placement() {
                self.state.drag.end = (point.x, point.y);
            }
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
        if self.state.mode.is_rect_selection() {
            // Kept aside: if this press turns out to be a click, it confirms
            // the selection a previous drag left rather than starting anew.
            self.previous_drag = Some(self.state.drag);
            let start = self
                .placement()
                .filter(|p| p.monitor == monitor)
                .map_or((x, y), |p| (p.x, p.y));
            self.state.drag = DragState {
                active: true,
                has_selection: false,
                monitor,
                start,
                end: start,
                press: (x, y),
            };
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
        if self.state.mode.is_rect_selection() {
            self.finish_drag(click);
        } else if click {
            self.click(monitor, x, y);
        }
        self.process_commands();
        self.render();
    }

    fn finish_drag(&mut self, click: bool) {
        let previous = self.previous_drag.take().unwrap_or_default();
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

    /// Drops a drag in progress. The button is still down, so the press is
    /// forgotten too: its release must neither confirm nor finish anything.
    fn cancel_drag(&mut self) {
        self.press = None;
        self.previous_drag = None;
        self.state.drag = DragState::default();
    }

    /// Copies a finished rectangle selection and quits.
    fn confirm_selection(&mut self) {
        if self.state.drag.has_selection {
            self.state.copy_and_quit(None);
        }
    }

    fn click(&mut self, monitor: usize, x: f32, y: f32) {
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

    fn mode_clicked(&mut self, index: usize) {
        if let Some(mode) = Mode::from_index(index) {
            self.state.set_mode(mode);
            self.refresh_derived();
            self.render();
        }
    }

    fn help_clicked(&mut self) {
        self.state.toggle_help();
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
    fn key(&mut self, text: &str, ctrl: bool, _shift: bool) -> bool {
        let is = |key: Key| text == SharedString::from(key).as_str();
        let digit = text.parse::<usize>().ok().filter(|_| text.len() == 1);

        let handled = if let Some(mode) = digit.and_then(Mode::from_digit).filter(|_| !ctrl) {
            self.state.set_mode(mode);
            self.refresh_derived();
            true
        } else if is(Key::Escape) {
            // Steps back one level at a time: a drag in progress, a
            // half-placed distance or a pending selection before leaving.
            if self.state.drag.active {
                self.cancel_drag();
            } else if self.state.distance_anchor.is_some() {
                self.state.distance_anchor = None;
            } else if self.state.drag.has_selection {
                self.state.drag.has_selection = false;
            } else {
                self.state.request_destructive(Destructive::Escape);
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
        } else if !ctrl && text.eq_ignore_ascii_case("q") {
            self.state.request_destructive(Destructive::Quit);
            true
        } else if ctrl && (text.eq_ignore_ascii_case("c") || text == "\u{3}") {
            let size = self.live_crosshair().map(|(_, rays)| rays.size());
            self.state.copy_and_quit(size);
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
                    if let Err(e) = clipboard::copy_text(&text) {
                        eprintln!("screen-ruler: could not copy to the clipboard: {e}");
                    }
                    // Also on stdout, so the measurement can be scripted.
                    println!("{text}");
                    let _ = slint::quit_event_loop();
                }
                Command::Recompute => {
                    self.recompute_timer
                        .start(TimerMode::SingleShot, analysis::DEBOUNCE, || {
                            with_app(App::analyse)
                        });
                }
                // Session mode (annotations, Markdown, image export) is not
                // wired yet, and nothing reachable emits these.
                Command::CopyText(_) | Command::CopyRegionImage(_) => {}
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
    /// start-up help); `tick` stops them once nothing does.
    fn animate(&mut self) {
        if !self.frame_timer.running() {
            self.frame_timer
                .start(TimerMode::Repeated, FRAME, || with_app(App::tick));
        }
    }

    fn tick(&mut self) {
        if let Some(since) = self.preview_since {
            if input::preview_finished(since.elapsed()) {
                self.preview_since = None;
            }
        }
        let help_fading = self.state.help_visible && self.state.help_auto_hide_at.is_some();
        if self.preview_since.is_none() && !help_fading {
            self.frame_timer.stop();
        }
        self.render();
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
                        rgb: [sample.r, sample.g, sample.b],
                        text: sample.notations().join("\n"),
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
        let dragging = self.state.drag.active;
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
                let [r, g, b] = color.rgb;
                window.set_color_swatch(slint::Color::from_rgb_u8(r, g, b));
                window.set_color_text(color.text.as_str().into());
            }

            window.set_show_distance(live.distance.is_some());
            if let Some(distance) = &live.distance {
                window.set_distance_ax(distance.a.0);
                window.set_distance_ay(distance.a.1);
                window.set_distance_bx(distance.b.0);
                window.set_distance_by(distance.b.1);
                window.set_distance_legs(distance.legs);
                window.set_distance_dx(format_distance(distance.delta.0.abs()).into());
                window.set_distance_dy(format_distance(distance.delta.1.abs()).into());
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

            let chrome = monitor == self.state.active_monitor;
            window.set_show_chrome(chrome);
            if chrome {
                window.set_mode(self.state.mode.index() as i32);
                window.set_help_opacity(help);
                window.set_help_visible(self.state.help_visible);
                sync(&self.dial_models[monitor], self.dials());
                sync(&self.help_models[monitor], help_lines(self.state.mode));
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

    fn window_logical_width(&self, monitor: usize) -> Option<f32> {
        let window = self.windows.get(monitor)?.window();
        let width = window.size().width as f32 / window.scale_factor();
        (width > 0.0).then_some(width)
    }
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
    window.on_key(|text, ctrl, shift| {
        let mut handled = false;
        with_app(|app| handled = app.key(&text, ctrl, shift));
        handled
    });
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

/// The shortcut help for `mode`, in quick mode.
fn help_lines(mode: Mode) -> Vec<SharedString> {
    let click = match mode {
        Mode::RectDrag | Mode::ShrinkToFit => {
            "Drag — select  ·  then click or Enter — copy it, then quit"
        }
        Mode::Distance => "Click — place point A, then point B to copy, then quit",
        Mode::Crosshair | Mode::Container | Mode::ColorPicker => {
            "Click — copy the measurement, then quit"
        }
    };
    [
        format!("1–{MODE_COUNT} — switch measurement mode"),
        click.to_string(),
        "Ctrl+C — copy the measurement, then quit".to_string(),
        "Wheel — adjust the first slider".to_string(),
        "? or H — toggle this help (hover it to hide it)".to_string(),
        "Esc or Q — quit".to_string(),
    ]
    .into_iter()
    .map(SharedString::from)
    .collect()
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
    rgb: [u8; 3],
    /// Hex, rgb() and hsl(), one per line.
    text: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
