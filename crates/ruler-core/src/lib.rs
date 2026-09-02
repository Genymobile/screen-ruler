//! ruler-core: platform-agnostic logic for screen-ruler.
//!
//! This crate hosts the pure/testable logic ported from the Python
//! implementation (`ruler/backend.py`, `ruler/core.py`, `ruler/capture.py`)
//! and adapted/ported from Edznux's `rust-rewrite` reference branch
//! (screen capture, edge detection, geometry/measurement ray-casting, color
//! sampling, clipboard, PNG/composite export, and the per-mode state
//! machine), credited in the relevant module-level doc comments as they are
//! ported.
//!
//! Modules are added incrementally as each backend porting todo lands; this
//! file is intentionally a placeholder until then.

pub const CRATE_NAME: &str = "ruler-core";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "ruler-core");
    }
}
