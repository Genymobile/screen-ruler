//! ruler-core: everything in screen-ruler that is not UI.
//!
//! Screen capture, edge detection, geometry and measurement, colour sampling,
//! the clipboard, and the interaction state machine. None of it depends on
//! Slint or needs a display, so all of it is tested headlessly; `ruler-app`
//! draws the state and carries out the commands it emits.

pub mod analysis;
pub mod boundary;
pub mod capture;
pub mod clipboard;
pub mod color;
pub mod edges;
pub mod geometry;
pub mod measure;
pub mod regions;
pub mod state;
