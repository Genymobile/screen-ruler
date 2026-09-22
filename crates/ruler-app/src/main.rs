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

fn main() -> Result<(), slint::PlatformError> {
    println!("{} - Slint spike window", ruler_core::CRATE_NAME);
    let window = SpikeWindow::new()?;
    window.run()
}
