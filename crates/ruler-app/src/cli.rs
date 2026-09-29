//! Command-line parsing.
//!
//! Hand-rolled rather than pulling in an argument-parsing crate: the interface
//! is four flags, and every dependency avoided is one less thing to keep
//! building on three platforms.

use ruler_core::edges;
use ruler_core::state::{Mode, MODE_COUNT};

/// Parsed command-line options.
#[derive(Debug, PartialEq)]
pub struct Options {
    /// Starting sensitivity, 0..100.
    pub sensitivity: f32,
    /// Explicit Canny thresholds `(low, high)`, when given on the command
    /// line. The dial cannot represent every pair, so these are kept exactly
    /// and should drive edge detection until the user moves the dial;
    /// `sensitivity` is only the nearest dial position, for display.
    pub thresholds: Option<(u16, u16)>,
    /// Keep the edge map visible for alignment debugging.
    pub debug_edges: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            sensitivity: edges::DEFAULT_SENSITIVITY,
            thresholds: None,
            debug_edges: false,
        }
    }
}

/// What the caller should do after parsing.
#[derive(Debug, PartialEq)]
pub enum Parsed {
    Run(Options),
    /// Print usage and exit successfully.
    Help,
    /// Print the version and exit successfully.
    Version,
    /// Report a usage error and exit non-zero.
    Error(String),
}

/// The `MODES` block, generated from the mode table rather than transcribed.
///
/// This list is the same one the number keys and the button tooltips come from,
/// so `--help` cannot advertise a mode the app does not have, name it something
/// the tooltip disagrees with, or number it differently.
fn modes_block() -> String {
    let width = Mode::ALL
        .iter()
        .map(|mode| mode.label().chars().count())
        .max()
        .unwrap_or(0);

    Mode::ALL
        .iter()
        .map(|mode| {
            format!(
                "    {}  {:width$}  {}",
                mode.digit(),
                mode.label(),
                mode.hint()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The full `--help` text.
pub fn usage() -> String {
    format!(
        "\
screen-ruler - measure distances between UI edges on any screen

USAGE:
    screen-ruler [OPTIONS]

OPTIONS:
    --sensitivity N        Edge-detection sensitivity, 0-100 (default: {default}).
                           Higher finds more edges.
    --threshold-low N      Lower Canny threshold, 0-255. Overrides --sensitivity.
    --threshold-high N     Upper Canny threshold, 0-255. Overrides --sensitivity.
    --debug-edge-overlay   Keep the detected edge map visible.
    -h, --help             Show this help.
    -V, --version          Show the version.

MODES (press 1-{last})
{modes}

KEYS
    Tab            Toggle session mode (persistent annotations)
    Click          Copy the measurement and quit / place an annotation
    Enter          Copy the current selection and quit
    Ctrl+C         Copy the measurement / export annotations as Markdown
    Ctrl+Z         Undo annotation        Ctrl+Shift+Z  Redo annotation
    Ctrl+Shift+C   Drag a region to copy it with annotations
    Wheel          Adjust the active mode's control
    ? or H         Toggle the shortcut overlay
    Esc / Q        Quit
",
        default = edges::DEFAULT_SENSITIVITY,
        last = MODE_COUNT,
        modes = modes_block(),
    )
}

/// Options that are switches and take no value.
const FLAGS: [&str; 5] = ["-h", "--help", "-V", "--version", "--debug-edge-overlay"];

/// Parses arguments, excluding the program name.
pub fn parse<I, S>(args: I) -> Parsed
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|a| a.as_ref().to_string()).collect();
    let mut options = Options::default();
    let mut low: Option<u16> = None;
    let mut high: Option<u16> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        // Accept both `--flag value` and `--flag=value`.
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg, None),
        };

        let mut take_value = |what: &str| -> Result<String, String> {
            if let Some(value) = inline.clone() {
                return Ok(value);
            }
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{what} needs a value"))
        };

        // Flags that take no value must not silently swallow one: accepting
        // `--debug-edge-overlay=false` would turn the overlay *on*.
        if inline.is_some() && FLAGS.contains(&name) {
            return Parsed::Error(format!("{name} does not take a value"));
        }

        match name {
            "-h" | "--help" => return Parsed::Help,
            "-V" | "--version" => return Parsed::Version,
            "--debug-edge-overlay" => options.debug_edges = true,
            "--sensitivity" => match take_value("--sensitivity") {
                Ok(value) => match value.parse::<f32>() {
                    Ok(parsed) if (0.0..=100.0).contains(&parsed) => options.sensitivity = parsed,
                    Ok(parsed) => {
                        return Parsed::Error(format!("--sensitivity must be 0-100, got {parsed}"))
                    }
                    Err(_) => {
                        return Parsed::Error(format!(
                            "--sensitivity expects a number, got '{value}'"
                        ))
                    }
                },
                Err(e) => return Parsed::Error(e),
            },
            "--threshold-low" | "--threshold-high" => {
                let value = match take_value(name) {
                    Ok(value) => value,
                    Err(e) => return Parsed::Error(e),
                };
                match value.parse::<u16>() {
                    Ok(parsed) if parsed <= 255 => {
                        if name == "--threshold-low" {
                            low = Some(parsed);
                        } else {
                            high = Some(parsed);
                        }
                    }
                    _ => {
                        return Parsed::Error(format!("{name} must be 0-255, got '{value}'"));
                    }
                }
            }
            other => return Parsed::Error(format!("unknown option '{other}'")),
        }

        index += 1;
    }

    // Explicit thresholds win, and map back onto the slider so the UI agrees
    // with what was asked for on the command line.
    if low.is_some() || high.is_some() {
        let (default_low, default_high) = edges::sensitivity_to_thresholds(options.sensitivity);
        let low = low.unwrap_or(default_low);
        let high = high.unwrap_or(default_high);
        if high <= low {
            return Parsed::Error(format!(
                "--threshold-high ({high}) must be greater than --threshold-low ({low})"
            ));
        }
        options.sensitivity = edges::thresholds_to_sensitivity(low, high);
        options.thresholds = Some((low, high));
    }

    Parsed::Run(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Options {
        match parse(args) {
            Parsed::Run(options) => options,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn the_help_text_lists_every_mode_with_its_number_key() {
        let usage = usage();
        for mode in Mode::ALL {
            let line = format!("    {}  {}", mode.digit(), mode.label());
            assert!(usage.contains(&line), "missing {mode:?}:\n{usage}");
            assert!(usage.contains(mode.hint()), "missing hint for {mode:?}");
        }
        assert!(
            usage.contains(&format!("MODES (press 1-{MODE_COUNT})")),
            "{usage}"
        );
    }

    #[test]
    fn the_help_text_states_the_real_default_sensitivity() {
        let expected = format!("(default: {})", edges::DEFAULT_SENSITIVITY);
        assert!(usage().contains(&expected), "{}", usage());
    }

    fn error(args: &[&str]) -> String {
        match parse(args) {
            Parsed::Error(message) => message,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn no_arguments_uses_the_defaults() {
        let options = run(&[]);
        assert_eq!(options.sensitivity, edges::DEFAULT_SENSITIVITY);
        assert!(!options.debug_edges);
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert_eq!(parse(["--help"]), Parsed::Help);
        assert_eq!(parse(["-h"]), Parsed::Help);
        assert_eq!(parse(["--version"]), Parsed::Version);
        assert_eq!(parse(["-V"]), Parsed::Version);
        // They win even alongside other arguments.
        assert_eq!(parse(["--sensitivity", "50", "--help"]), Parsed::Help);
    }

    #[test]
    fn sensitivity_accepts_both_argument_forms() {
        assert_eq!(run(&["--sensitivity", "42"]).sensitivity, 42.0);
        assert_eq!(run(&["--sensitivity=42"]).sensitivity, 42.0);
    }

    #[test]
    fn the_debug_flag_is_recognised() {
        assert!(run(&["--debug-edge-overlay"]).debug_edges);
    }

    #[test]
    fn explicit_thresholds_map_back_onto_the_slider() {
        // The values the Python implementation shipped as defaults.
        let options = run(&["--threshold-low", "16", "--threshold-high", "54"]);
        assert!(
            (options.sensitivity - edges::DEFAULT_SENSITIVITY).abs() < 1.0,
            "got {}",
            options.sensitivity
        );
    }

    #[test]
    fn one_threshold_alone_is_combined_with_the_sensitivity_default() {
        let options = run(&["--threshold-low", "5"]);
        assert!(options.sensitivity > 0.0);
        let (_, default_high) = edges::sensitivity_to_thresholds(edges::DEFAULT_SENSITIVITY);
        assert_eq!(options.thresholds, Some((5, default_high)));
    }

    #[test]
    fn explicit_thresholds_are_kept_exactly() {
        // Regression: the pair used to survive only as a dial position, and
        // 0/1 comes back from the dial as 5/25.
        let options = run(&["--threshold-low", "0", "--threshold-high", "1"]);
        assert_eq!(options.thresholds, Some((0, 1)));
        assert_eq!(run(&[]).thresholds, None);
    }

    #[test]
    fn flags_reject_an_inline_value() {
        assert!(error(&["--debug-edge-overlay=false"]).contains("does not take a value"));
        assert!(error(&["--help=no"]).contains("--help"));
    }

    #[test]
    fn out_of_range_and_malformed_values_are_rejected() {
        assert!(error(&["--sensitivity", "150"]).contains("0-100"));
        assert!(error(&["--sensitivity", "abc"]).contains("expects a number"));
        assert!(error(&["--threshold-low", "999"]).contains("0-255"));
        assert!(error(&["--sensitivity"]).contains("needs a value"));
    }

    #[test]
    fn crossed_thresholds_are_rejected() {
        let message = error(&["--threshold-low", "100", "--threshold-high", "20"]);
        assert!(message.contains("must be greater than"), "{message}");
    }

    #[test]
    fn unknown_options_are_reported_by_name() {
        assert!(error(&["--wat"]).contains("--wat"));
    }
}
