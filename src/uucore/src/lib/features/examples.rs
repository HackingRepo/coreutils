// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! Provides `--examples` flag support for utilities.
//!
//! When enabled, each utility can display tldr-based usage examples.
//! The examples are embedded at compile time from `docs/tldr.zip`.

use std::io::Write;

include!(concat!(env!("OUT_DIR"), "/examples_map.rs"));

/// Print the tldr examples for the given utility, if available.
///
/// Returns `true` if examples were found and printed, `false` otherwise.
pub fn print_examples(util_name: &str) -> bool {
    if let Some(examples) = get_examples(util_name) {
        let mut stdout = std::io::stdout().lock();
        let _ = write!(stdout, "{examples}");
        true
    } else {
        eprintln!("No examples available for '{util_name}'.");
        false
    }
}
