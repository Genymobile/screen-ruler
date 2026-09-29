//! screen-ruler application entry point.
//!
//! Captures every monitor, then covers each one with a borderless fullscreen
//! overlay window showing its frozen screenshot. Measurement, the controls
//! panel and the rest of the UI are drawn on top of these in later steps.

slint::slint! {
    export component Overlay inherits Window {
        in property <image> backdrop;
        in property <string> label;
        callback quit();

        no-frame: true;
        forward-focus: keys;

        keys := FocusScope {
            key-pressed(event) => {
                if (event.text == Key.Escape) {
                    root.quit();
                    return accept;
                }
                reject
            }

            Image {
                source: root.backdrop;
                width: 100%;
                height: 100%;
                image-fit: fill;
            }

            Rectangle {
                x: 16px;
                y: 16px;
                width: caption.preferred-width + 16px;
                height: caption.preferred-height + 8px;
                background: #000000b0;
                border-radius: 6px;

                caption := Text {
                    text: root.label;
                    color: white;
                }
            }
        }
    }
}

mod cli;
mod placement;

use std::process::ExitCode;

use cli::{Options, Parsed};
use ruler_core::capture::{self, CapturedMonitor};
use slint::winit_030::winit::window::Fullscreen;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

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

    let overlays = monitors
        .iter()
        .map(|monitor| open_overlay(monitor, options))
        .collect::<Result<Vec<_>, _>>()?;

    slint::run_event_loop().map_err(|e| e.to_string())?;
    drop(overlays);
    Ok(())
}

/// Opens one overlay and pins it, borderless fullscreen, to the output
/// `monitor` was captured from.
fn open_overlay(monitor: &CapturedMonitor, options: &Options) -> Result<Overlay, String> {
    let geometry = &monitor.geometry;
    let overlay = Overlay::new().map_err(|e| e.to_string())?;

    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        monitor.image.as_raw(),
        monitor.image.width(),
        monitor.image.height(),
    );
    overlay.set_backdrop(Image::from_rgba8(pixels));
    overlay.set_label(
        format!(
            "{} · {}×{} @{}x · sensitivity {} · Esc to quit",
            geometry.name, geometry.size.0, geometry.size.1, geometry.scale, options.sensitivity
        )
        .into(),
    );
    overlay.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    overlay.show().map_err(|e| e.to_string())?;

    // The winit window only exists once the event loop is running, so the
    // placement waits for it rather than racing it.
    let window = overlay.as_weak();
    let geometry = geometry.clone();
    slint::spawn_local(async move {
        let Some(overlay) = window.upgrade() else {
            return;
        };
        match overlay.window().winit_window().await {
            Ok(winit_window) => place(&winit_window, &geometry),
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
