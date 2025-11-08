// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.
#![allow(clippy::cast_possible_wrap)]

use uutests::at_and_ucmd;
use uutests::new_ucmd;

#[test]
#[cfg(target_os = "linux")]
fn test_tsort_non_utf8_paths() {
    use std::os::unix::ffi::OsStringExt;
    let (at, mut ucmd) = at_and_ucmd!();

    let filename = std::ffi::OsString::from_vec(vec![0xFF, 0xFE]);
    std::fs::write(at.plus(&filename), b"a b\nb c\n").unwrap();

    ucmd.arg(&filename).succeeds().stdout_is("a\nb\nc\n");
}

#[test]
fn test_invalid_arg() {
    new_ucmd!().arg("--definitely-invalid").fails_with_code(1);
}
#[test]
fn test_sort_call_graph() {
    new_ucmd!()
        .arg("call_graph.txt")
        .succeeds()
        .stdout_is_fixture("call_graph.expected");
}

#[test]
fn test_sort_self_loop() {
    new_ucmd!()
        .pipe_in("first first\nfirst second second second")
        .succeeds()
        .stdout_only("first\nsecond\n");
}

#[test]
fn test_sort_floating_nodes() {
    new_ucmd!()
        .pipe_in("d d\nc c\na a\nb b")
        .succeeds()
        .stdout_only("a\nb\nc\nd\n");
}

#[test]
fn test_no_such_file() {
    new_ucmd!()
        .arg("invalid_file_txt")
        .fails()
        .stderr_contains("No such file or directory");
}

#[test]
fn test_version_flag() {
    let version_short = new_ucmd!().arg("-V").succeeds();
    let version_long = new_ucmd!().arg("--version").succeeds();

    assert_eq!(version_short.stdout_str(), version_long.stdout_str());
}

#[test]
fn test_help_flag() {
    let help_short = new_ucmd!().arg("-h").succeeds();
    let help_long = new_ucmd!().arg("--help").succeeds();

    assert_eq!(help_short.stdout_str(), help_long.stdout_str());
}

#[test]
fn test_multiple_arguments() {
    new_ucmd!()
        .arg("call_graph.txt")
        .arg("invalid_file")
        .fails()
        .stderr_contains("extra operand 'invalid_file'");
}

#[test]
fn test_error_on_dir() {
    let (at, mut ucmd) = at_and_ucmd!();
    at.mkdir("tsort_test_dir");
    ucmd.arg("tsort_test_dir")
        .fails()
        .stderr_contains("tsort: tsort_test_dir: read error: Is a directory");
}

#[test]
fn test_split_on_any_whitespace() {
    new_ucmd!()
        .pipe_in("a\nb\n")
        .succeeds()
        .stdout_only("a\nb\n");
}

#[test]
fn test_cycle() {
    // The graph looks like:  a --> b <==> c --> d
    new_ucmd!()
        .pipe_in("a b b c c d c b")
        .fails_with_code(1)
        .stdout_is("a\nb\nc\nd\n")
        .stderr_is("tsort: -: input contains a loop:\ntsort: b\ntsort: c\n");
}

#[test]
fn test_two_cycles() {
    // The graph looks like:
    //
    //        a
    //        |
    //        V
    // c <==> b <==> d
    //
    new_ucmd!()
        .pipe_in("a b b c c b b d d b")
        .fails_with_code(1)
        .stdout_is("a\nb\nd\nc\n")
        .stderr_is("tsort: -: input contains a loop:\ntsort: b\ntsort: c\ntsort: -: input contains a loop:\ntsort: b\ntsort: d\n");
}

#[test]
fn test_long_loop_no_stack_overflow() {
    use std::fmt::Write;
    const N: usize = 100_000;
    let mut input = String::new();
    for v in 0..N {
        let next = (v + 1) % N;
        let _ = write!(input, "{v} {next} ");
    }
    new_ucmd!()
        .pipe_in(input)
        .fails_with_code(1)
        .stderr_contains("tsort: -: input contains a loop");
}

#[test]
fn test_loop_for_iterative_dfs_correctness() {
    let input = r"
        A B
        B C
        C B
        C D
        D A
    ";

    new_ucmd!()
        .pipe_in(input)
        .fails_with_code(1)
        .stderr_contains("tsort: -: input contains a loop:\ntsort: B\ntsort: C");
}

#[test]
fn test_warn_flag_accepted() {
    // Test that -w/--warn flag is accepted without error
    // This is for GNU test suite compatibility (cycle-3 test uses -w)
    // The flag doesn't change behavior, just needs to be parseable
    new_ucmd!()
        .arg("-w")
        .pipe_in("a b\nb c\nc a")
        .fails_with_code(1)
        .stderr_contains("input contains a loop:");
}

#[test]
fn test_warn_flag_long() {
    // Test that --warn is also accepted
    new_ucmd!()
        .arg("--warn")
        .pipe_in("a b\nb c")
        .succeeds()
        .stdout_is("a\nb\nc\n");
}

#[test]
fn test_cycle_detection_alphabetical_order() {
    // Verify cycles are reported starting from alphabetically first node
    // Input: t->b, t->s, s->t creates cycle [s,t]
    // Cycle should be detected starting from 's' (alphabetically first)
    new_ucmd!()
        .pipe_in("t b t s s t")
        .fails_with_code(1)
        .stdout_is("s\nt\nb\n")
        .stderr_contains("tsort: s\ntsort: t");
}

#[test]
fn test_tree_branching_order() {
    // Test the interleaving behavior when dependency chains branch
    // Input creates:  a→b→c→d→e→f→g
    //                      ↓
    //                      x→y→z
    // Should output: a, b, c, x, d, y, e, z, f, g
    new_ucmd!()
        .pipe_in("a b b c c d d e e f f g c x x y y z")
        .succeeds()
        .stdout_is("a\nb\nc\nx\nd\ny\ne\nz\nf\ng\n");
}

#[test]
fn test_multiple_cycles_alphabetical() {
    // Test multiple cycles are detected in alphabetical order
    // Input: a→b→a (cycle 1), a→c→a (cycle 2)
    // Both cycles should start from 'a' (alphabetically first in each)
    new_ucmd!()
        .pipe_in("a a a b a c c a b a")
        .fails_with_code(1)
        .stderr_contains("tsort: a\ntsort: b")
        .stderr_contains("tsort: a\ntsort: c");
}

#[test]
fn test_alphabetical_frontier_ordering() {
    // Test that initial independent nodes are processed alphabetically
    // All nodes have no dependencies, should output alphabetically
    new_ucmd!()
        .pipe_in("d d c c b b a a")
        .succeeds()
        .stdout_is("a\nb\nc\nd\n");
}
