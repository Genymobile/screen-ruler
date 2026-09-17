//! ruler-core: platform-agnostic logic for screen-ruler.
//!
//! The pure, testable half of the app, ported from the Python implementation
//! in `ruler/`: screen capture, edge detection, geometry and measurement,
//! colour sampling, clipboard, export, and the per-mode state machine.
//!
//! Modules are added incrementally as each piece lands.

pub mod geometry;

pub const CRATE_NAME: &str = "ruler-core";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "ruler-core");
    }
}
