// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.
// spell-checker:ignore numfmt

#![no_main]
use libfuzzer_sys::fuzz_target;
use uu_numfmt::uumain;

use rand::RngExt;
use rand::seq::IndexedRandom;
use std::ffi::OsString;

use uufuzz::CommandResult;
use uufuzz::{compare_result, generate_and_run_uumain, generate_random_string, run_gnu_cmd};

static CMD_PATH: &str = "/usr/bin/numfmt";

fn generate_number(rng: &mut rand::rngs::ThreadRng) -> String {
    match rng.random_range(0..=8) {
        0 => rng.random_range(-10_000_000i64..=10_000_000).to_string(),
        // Fractional inputs still diverge in rounding when value < scale
        // (uutils/coreutils#11663, e.g. `--to=si 3.14` → uutils 4 vs GNU 3).
        1 => rng.random_range(-1_000_000i64..=1_000_000).to_string(),
        2 => {
            // Number with SI suffix
            let n = rng.random_range(1..=9999);
            let suffix = ["K", "M", "G", "T", "P", "E", "Ki", "Mi", "Gi", "Ti"]
                .choose(rng)
                .unwrap();
            format!("{n}{suffix}")
        }
        // Cap below 2^52 — above ~1e17 GNU rejects `--format` precisions that
        // can't represent the value (e.g. `--format=%5.1f 1e18`), but uutils
        // happily prints it. f64 mantissa is 52 bits anyway, so this is the
        // natural ceiling for the shared numeric path.
        3 => rng.random_range(0u64..(1u64 << 52)).to_string(),
        4 => "0".to_string(),
        // Scientific notation #11655 is fixed in default (--invalid=abort) mode,
        // but combining sci-notation with --invalid=warn/ignore/fail still
        // diverges (uutils halts + exits 2, GNU prints+continues+exits 0). Leave
        // sci-notation out of the generator until that follow-on is fixed.
        5 => rng.random_range(1..=1_000_000).to_string(),
        6 => generate_random_string(rng.random_range(1..=8)),
        7 => format!("-{}", rng.random_range(1..=1_000_000)),
        _ => rng.random_range(1..=1000).to_string(),
    }
}

fn generate_numfmt_args() -> Vec<String> {
    let mut rng = rand::rng();
    let mut args: Vec<String> = Vec::new();

    let scales = ["none", "si", "iec", "iec-i", "auto"];
    let rounds = ["up", "down", "from-zero", "towards-zero", "nearest"];
    let invalids = ["abort", "fail", "warn", "ignore"];

    if rng.random_bool(0.7) {
        args.push(format!("--from={}", scales.choose(&mut rng).unwrap()));
    }
    if rng.random_bool(0.7) {
        args.push(format!("--to={}", scales.choose(&mut rng).unwrap()));
    }
    if rng.random_bool(0.3) {
        args.push(format!("--round={}", rounds.choose(&mut rng).unwrap()));
    }
    if rng.random_bool(0.2) {
        args.push(format!("--invalid={}", invalids.choose(&mut rng).unwrap()));
    }
    if rng.random_bool(0.2) {
        args.push(format!("--from-unit={}", rng.random_range(1..=1024)));
    }
    // --to-unit left disabled: prefix selection (#11666) is fixed, but combining
    // --to-unit with --to=si/iec/iec-i still exposes the #11663-family rounding
    // divergence (e.g. `--to=iec --to-unit=689 701` → GNU 1 vs uutils 2).
    if rng.random_bool(0.2) {
        args.push(format!("--padding={}", rng.random_range(-30..=30)));
    }
    if rng.random_bool(0.15) {
        args.push(format!(
            "--suffix={}",
            generate_random_string(rng.random_range(1..=3))
        ));
    }
    if rng.random_bool(0.15) {
        // A simple printf-style format
        let width = rng.random_range(1..=20);
        // Cap precision at 3: precision >= 4 combined with --to=iec/iec-i
        // exposes a remaining #11663-family divergence in fractional rounding
        // (e.g. `--format=%.5f --to=iec 874497` → GNU 854.00100K vs uutils
        // 854.00098K).
        let prec = rng.random_range(0..=3);
        let flag = ["", "-", "'", "0"].choose(&mut rng).unwrap();
        args.push(format!("--format=%{flag}{width}.{prec}f"));
    }
    if rng.random_bool(0.1) {
        args.push("--grouping".to_string());
    }
    if rng.random_bool(0.1) {
        args.push("--debug".to_string());
    }
    if rng.random_bool(0.1) {
        args.push(format!("--header={}", rng.random_range(1..=5)));
    }
    if rng.random_bool(0.1) {
        args.push(format!("--field={}", rng.random_range(1..=5)));
    }
    if rng.random_bool(0.1) {
        let delim: char = ['-', ',', ':', ';', '|', ' ']
            .choose(&mut rng)
            .copied()
            .unwrap();
        args.push(format!("--delimiter={delim}"));
    }

    // Numbers as positional arguments. If any start with '-' we add a `--`
    // separator so they're treated as positionals (both GNU and uutils now
    // reject unseparated negatives the same way, but keeping the separator
    // preserves coverage of the negative-number formatting pipeline).
    let num_count = rng.random_range(1..=3);
    let numbers: Vec<String> = (0..num_count).map(|_| generate_number(&mut rng)).collect();
    if numbers.iter().any(|n| n.starts_with('-')) {
        args.push("--".to_string());
    }
    args.extend(numbers);

    args
}

fuzz_target!(|_data: &[u8]| {
    // Match the locale `run_gnu_cmd` uses for GNU (LC_ALL=C), otherwise all
    // localized error/help strings and number grouping diverge spuriously.
    // SAFETY: libFuzzer runs the target single-threaded.
    unsafe {
        std::env::set_var("LC_ALL", "C");
        std::env::set_var("LANG", "C");
    }

    let numfmt_args = generate_numfmt_args();
    let mut args = vec![OsString::from("numfmt")];
    args.extend(numfmt_args.iter().map(OsString::from));

    let rust_result = generate_and_run_uumain(&args, uumain, None);

    let gnu_result = match run_gnu_cmd(CMD_PATH, &args[1..], false, None) {
        Ok(result) => result,
        Err(error_result) => {
            eprintln!("Failed to run GNU command:");
            eprintln!("Stderr: {}", error_result.stderr);
            eprintln!("Exit Code: {}", error_result.exit_code);
            CommandResult {
                stdout: String::new(),
                stderr: error_result.stderr,
                exit_code: error_result.exit_code,
            }
        }
    };

    compare_result(
        "numfmt",
        &format!("{:?}", &args[1..]),
        None,
        &rust_result,
        &gnu_result,
        false,
    );
});
