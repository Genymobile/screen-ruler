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

use ruler_core::clipboard;
use ruler_core::edges;
use ruler_core::geometry::Point;
use ruler_core::state::{Command, Destructive, Mode, RulerState};
use slint::platform::Key;
use slint::{ComponentHandle, SharedString, Timer, TimerMode};

use crate::analysis;
use crate::input::{self, Wheel};
use crate::surface::Surface;
use crate::{Overlay, Rays};

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
    wheel: Wheel,
    /// Thresholds for the next analysis; `None` derives them from the
    /// sensitivity. Only the command line sets this, for the first run.
    thresholds: Option<(u16, u16)>,
    /// Incremented per analysis request; results from older ones are stale.
    generation: u64,
    /// When the latest re-analysis landed, for the edge preview flash.
    preview_since: Option<Instant>,
    /// Rendered edge maps, built only once something shows them.
    edge_images: Vec<Option<slint::Image>>,
    recompute_timer: Timer,
    frame_timer: Timer,
}

impl App {
    /// Takes ownership of the windows and surfaces, wires every window's
    /// callbacks, and starts the first analysis.
    pub fn start(
        state: RulerState,
        surfaces: Vec<Surface>,
        windows: Vec<Overlay>,
        thresholds: Option<(u16, u16)>,
    ) -> Shared {
        let count = surfaces.len();
        let app = Rc::new(RefCell::new(App {
            state,
            surfaces,
            windows,
            hovered: None,
            press: None,
            wheel: Wheel::default(),
            thresholds,
            generation: 0,
            preview_since: None,
            edge_images: vec![None; count],
            recompute_timer: Timer::default(),
            frame_timer: Timer::default(),
        }));
        APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));

        {
            let app = app.borrow();
            for (monitor, window) in app.windows.iter().enumerate() {
                wire(window, monitor);
            }
        }
        app.borrow_mut().analyse();
        app.borrow_mut().render();
        app
    }

    // ------------------------------------------------------------- input

    fn pointer_moved(&mut self, monitor: usize, x: f32, y: f32) {
        if let Some(width) = self.window_logical_width(monitor) {
            self.surfaces[monitor].reconcile_scale(width);
        }
        self.hovered = Some(monitor);
        self.state.pointer = Some(Point { monitor, x, y });
        self.state.active_monitor = monitor;
        self.render();
    }

    fn pointer_left(&mut self, monitor: usize) {
        if self.hovered == Some(monitor) {
            self.hovered = None;
            self.render();
        }
    }

    fn pointer_pressed(&mut self, monitor: usize, x: f32, y: f32) {
        self.press = Some((monitor, x, y));
    }

    fn pointer_released(&mut self, monitor: usize, x: f32, y: f32) {
        let Some((pressed_on, px, py)) = self.press.take() else {
            return;
        };
        // Acting on release, not press: a click is only a click once the
        // button comes back up without the pointer having travelled.
        if pressed_on == monitor && input::is_click((px, py), (x, y)) {
            self.click(monitor, x, y);
        }
    }

    fn click(&mut self, monitor: usize, x: f32, y: f32) {
        if self.state.mode == Mode::Crosshair {
            let size = self.surfaces[monitor].rays_at(x, y).map(|r| r.size());
            self.state.copy_and_quit(size);
        }
        self.process_commands();
    }

    fn wheel(&mut self, delta: f32) {
        let notches = self.wheel.feed(delta);
        if notches != 0 {
            self.state.adjust_by_wheel(notches as f32);
            self.process_commands();
            self.render();
        }
    }

    /// Returns whether the key was handled.
    fn key(&mut self, text: &str, ctrl: bool, _shift: bool) -> bool {
        let handled = if text == SharedString::from(Key::Escape).as_str() {
            self.state.request_destructive(Destructive::Escape);
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

    fn analysed(&mut self, generation: u64, maps: Vec<edges::EdgeMap>) {
        if generation != self.generation {
            return;
        }
        let first = self.surfaces.iter().all(|s| s.edges.is_none());
        for ((surface, image), map) in self
            .surfaces
            .iter_mut()
            .zip(&mut self.edge_images)
            .zip(maps)
        {
            surface.edges = Some(map);
            *image = None;
        }
        // Flash the new map, unless this is the start-up analysis nobody
        // asked to see.
        if !first {
            self.preview_since = Some(Instant::now());
            self.frame_timer
                .start(TimerMode::Repeated, FRAME, || with_app(App::tick));
        }
        self.render();
    }

    fn tick(&mut self) {
        if let Some(since) = self.preview_since {
            if input::preview_finished(since.elapsed()) {
                self.preview_since = None;
                self.frame_timer.stop();
            }
        }
        self.render();
    }

    // ------------------------------------------------------------ render

    /// The crosshair under the pointer, when it has something to show.
    fn live_crosshair(&self) -> Option<(Point, crate::surface::LogicalRays)> {
        let point = self.state.pointer?;
        if self.state.mode != Mode::Crosshair || self.hovered != Some(point.monitor) {
            return None;
        }
        let rays = self.surfaces[point.monitor].rays_at(point.x, point.y)?;
        Some((point, rays))
    }

    fn render(&mut self) {
        let opacity = input::edges_opacity(
            self.state.debug_edges,
            self.preview_since.map(|t| t.elapsed()),
        );
        let crosshair = self.live_crosshair();
        let pointer = self.state.pointer;

        for (monitor, window) in self.windows.iter().enumerate() {
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

            match (crosshair, pointer) {
                (Some((point, rays)), _) if point.monitor == monitor => {
                    window.set_cursor_x(point.x);
                    window.set_cursor_y(point.y);
                    window.set_rays(Rays {
                        north: rays.north,
                        south: rays.south,
                        west: rays.west,
                        east: rays.east,
                    });
                    window.set_show_crosshair(true);
                    window.set_chip_text(self.state.measurement_text(Some(rays.size())).into());
                }
                // Hovered, but the edge map is still being computed.
                (None, Some(point))
                    if point.monitor == monitor
                        && self.hovered == Some(monitor)
                        && self.surfaces[monitor].edges.is_none() =>
                {
                    window.set_cursor_x(point.x);
                    window.set_cursor_y(point.y);
                    window.set_show_crosshair(false);
                    window.set_chip_text("Detecting edges…".into());
                }
                _ => {
                    window.set_show_crosshair(false);
                    window.set_chip_text(SharedString::new());
                }
            }
        }
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
    window.on_key(|text, ctrl, shift| {
        let mut handled = false;
        with_app(|app| handled = app.key(&text, ctrl, shift));
        handled
    });
}
