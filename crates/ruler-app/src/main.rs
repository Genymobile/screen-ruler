//! screen-ruler application entry point.
//!
//! Captures every monitor, then covers each one with a borderless fullscreen
//! overlay window showing its frozen screenshot, and hands them to [`app`],
//! which draws the measurement on top.

slint::slint! {
    export { Overlay, Rays } from "ui/overlay.slint";
}

mod analysis;
mod app;
mod cli;
mod input;
mod placement;
mod surface;

use std::process::ExitCode;

use cli::{Options, Parsed};
use ruler_core::capture::{self, CapturedMonitor};
use ruler_core::state::RulerState;
use slint::winit_030::winit::window::Fullscreen;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use surface::Surface;

fn main() -> ExitCode {
    let options = match cli::parse(std::env::args().skip(1)) {
        Parsed::Run(options) => options,
        Parsed::Help => {
            print!("{}", cli::usage());
            return ExitCode::SUCCESS;
        }
        Parsed::Version => {
            println!("screen-ruler {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Parsed::Error(message) => {
            eprintln!("screen-ruler: {message}\nTry 'screen-ruler --help' for usage.");
            return ExitCode::from(2);
        }
    };

    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("screen-ruler: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: &Options) -> Result<(), String> {
    // Placement goes through winit, so make sure that is the backend in use.
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .select()
        .map_err(|e| format!("cannot start the winit backend: {e}"))?;

    // Capture before any window exists, so the overlays are not in the shot.
    let monitors =
        capture::capture_all().map_err(|e| format!("{e}\n{}", capture::permission_hint()))?;

    let windows = monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| open_overlay(monitor, index == 0))
        .collect::<Result<Vec<_>, _>>()?;
    let surfaces = monitors
        .into_iter()
        .map(|m| Surface::new(m.geometry, m.image))
        .collect();

    let state = RulerState::new(options.sensitivity, options.debug_edges);
    let app = app::App::start(state, surfaces, windows, options.thresholds);

    slint::run_event_loop().map_err(|e| e.to_string())?;
    drop(app);
    Ok(())
}

/// Opens one overlay and pins it, borderless fullscreen, to the output
/// `monitor` was captured from. `focus` asks for keyboard focus too: an
/// overlay that never receives keys could not be quit.
fn open_overlay(monitor: &CapturedMonitor, focus: bool) -> Result<Overlay, String> {
    let overlay = Overlay::new().map_err(|e| e.to_string())?;

    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        monitor.image.as_raw(),
        monitor.image.width(),
        monitor.image.height(),
    );
    overlay.set_backdrop(Image::from_rgba8(pixels));
    overlay.show().map_err(|e| e.to_string())?;

    // The winit window only exists once the event loop is running, so the
    // placement waits for it rather than racing it.
    let window = overlay.as_weak();
    let geometry = monitor.geometry.clone();
    slint::spawn_local(async move {
        let Some(overlay) = window.upgrade() else {
            return;
        };
        match overlay.window().winit_window().await {
            Ok(winit_window) => {
                place(&winit_window, &geometry);
                if focus {
                    winit_window.focus_window();
                }
            }
            Err(e) => eprintln!("screen-ruler: {}: no native window: {e}", geometry.name),
        }
    })
    .map_err(|e| e.to_string())?;

    Ok(overlay)
}

/// Makes `window` borderless fullscreen on the output matching `geometry`.
fn place(
    window: &slint::winit_030::winit::window::Window,
    geometry: &ruler_core::geometry::MonitorGeometry,
) {
    let monitors: Vec<_> = window.available_monitors().collect();
    let candidates: Vec<_> = monitors
        .iter()
        .map(|m| placement::Candidate {
            name: m.name(),
            position: (m.position().x, m.position().y),
        })
        .collect();

    let target = placement::pick(geometry, &candidates).map(|i| monitors[i].clone());
    if target.is_none() {
        // Fullscreen on whichever output the window landed on beats a
        // floating window, but the overlay may now cover the wrong screen.
        eprintln!(
            "screen-ruler: {}: no matching output among {:?}",
            geometry.name, candidates
        );
    }
    window.set_fullscreen(Some(Fullscreen::Borderless(target)));
}
