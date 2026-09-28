//! screen-ruler application entry point.
//!
//! This is currently a Slint feasibility spike: a single ordinary window
//! proving the toolchain and rendering backend work end to end. It will be
//! replaced by the real per-monitor borderless overlay once that placement
//! approach is validated, and wired to the backend modules ported into
//! `ruler-core`.

slint::slint! {
    export component SpikeWindow inherits Window {
        title: "screen-ruler (Slint spike)";
        width: 320px;
        height: 160px;

        Text {
            text: "Slint is wired up.\nNext: per-monitor overlay placement.";
            horizontal-alignment: center;
            vertical-alignment: center;
        }
    }
}

mod cli;

use std::process::ExitCode;

use cli::Parsed;

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

    // The spike does not use the options yet; echo them so the parsing can be
    // checked end to end until the overlay consumes them.
    println!(
        "screen-ruler (Slint spike): sensitivity {}, thresholds {:?}, debug edges {}",
        options.sensitivity, options.thresholds, options.debug_edges
    );
    match SpikeWindow::new().and_then(|window| window.run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("screen-ruler: {e}");
            ExitCode::FAILURE
        }
    }
}
