#!/bin/bash
# Test script to verify that chown/chmod/chgrp use only safe traversal syscalls
# This script uses strace to monitor syscalls and ensures no unsafe operations are performed

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TEMP_DIR=$(mktemp -d)
STRACE_LOG="$TEMP_DIR/strace.log"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if strace is available
if ! command -v strace &> /dev/null; then
    log_error "strace is not installed. Please install it: sudo apt-get install strace"
    exit 1
fi

# Check if the binary exists
BINARY="$PROJECT_DIR/target/debug/coreutils"
if [[ ! -f "$BINARY" ]]; then
    log_error "Binary not found at $BINARY. Please build first: cargo build"
    exit 1
fi

# Create comprehensive test directory structure
create_test_structure() {
    local base_dir="$1"

    log_info "Creating test directory structure in $base_dir"

    # Create directories
    mkdir -p "$base_dir/testdir/subdir1/subsubdir"
    mkdir -p "$base_dir/testdir/subdir2"
    mkdir -p "$base_dir/testdir/emptydir"

    # Create regular files
    echo "file1 content" > "$base_dir/testdir/file1.txt"
    echo "file2 content" > "$base_dir/testdir/subdir1/file2.txt"
    echo "file3 content" > "$base_dir/testdir/subdir1/subsubdir/file3.txt"
    echo "file4 content" > "$base_dir/testdir/subdir2/file4.txt"

    # Create symlinks
    ln -s file1.txt "$base_dir/testdir/symlink_to_file"
    ln -s subdir1 "$base_dir/testdir/symlink_to_dir"
    ln -s ../file1.txt "$base_dir/testdir/subdir1/symlink_to_parent"
    ln -s nonexistent "$base_dir/testdir/dangling_symlink"

    # Create a FIFO (named pipe) if possible
    mkfifo "$base_dir/testdir/test_fifo" 2>/dev/null || log_warn "Could not create FIFO"

    # Create a hard link
    ln "$base_dir/testdir/file1.txt" "$base_dir/testdir/hardlink_to_file1" 2>/dev/null || log_warn "Could not create hard link"

    log_info "Test structure created successfully"
}

# Test a specific command with strace
test_command_safety() {
    local cmd="$1"
    local args="$2"
    local test_dir="$3"
    local test_name="$4"

    log_info "Testing $test_name: $cmd $args"

    # Debug: Check if binary and test directory exist
    if [[ ! -x "$BINARY" ]]; then
        log_error "Binary not found or not executable: $BINARY"
        return 1
    fi

    if [[ ! -d "$test_dir" ]]; then
        log_error "Test directory not found: $test_dir"
        return 1
    fi

    # Debug: Show the full command that will be run (only in verbose mode)
    if [[ "${DEBUG:-}" == "1" ]]; then
        log_info "Test directory contents:"
        ls -la "$test_dir" >&2 || true
        log_info "Full command: $BINARY $cmd $args $test_dir"
    fi

    # Test if strace is working at all
    if ! strace -V >/dev/null 2>&1; then
        log_error "strace is not available or not working"
        return 1
    fi

    # Test if the command itself works without strace (only in debug mode)
    if [[ "${DEBUG:-}" == "1" ]]; then
        log_info "Testing command without strace first..."
        if "$BINARY" "$cmd" $args "$test_dir" >/dev/null 2>&1; then
            log_info "Command executed successfully without strace"
        else
            local cmd_exit_code=$?
            log_info "Command failed with exit code: $cmd_exit_code (this may be expected due to permissions)"
        fi
    fi

    # Run command under strace
    local strace_exit_code=0
    strace -f \
           -e trace=openat,openat2,open,fchownat,lchown,chown,fchmod,fchmodat,chmod,readdir,getdents,getdents64,newfstatat,statx,lstat,stat \
           -e signal=none \
           -o "$STRACE_LOG" \
           "$BINARY" "$cmd" $args "$test_dir" 2>"$TEMP_DIR/strace_stderr.log" || strace_exit_code=$?

    # Debug: Check if strace had issues
    if [[ $strace_exit_code -ne 0 ]]; then
        log_error "Strace exited with code: $strace_exit_code"
        if [[ -f "$TEMP_DIR/strace_stderr.log" ]]; then
            log_error "Strace stderr output:"
            cat "$TEMP_DIR/strace_stderr.log" >&2
        fi
    fi

    # Debug: Check what was created (only in debug mode)
    if [[ "${DEBUG:-}" == "1" ]]; then
        log_info "Strace log file info:"
        if [[ -f "$STRACE_LOG" ]]; then
            log_info "Strace log exists, size: $(wc -c < "$STRACE_LOG") bytes"
            log_info "First few lines of strace log:"
            head -5 "$STRACE_LOG" >&2 || true
        else
            log_error "Strace log file was not created: $STRACE_LOG"
            log_info "Files in temp directory:"
            ls -la "$TEMP_DIR" >&2 || true
        fi
    fi

    # Note: We allow non-zero exit codes because the command might fail due to permissions,
    # but we still want to analyze what syscalls were attempted

    # Define unsafe syscall patterns that indicate direct filesystem access
    local unsafe_patterns=(
        "^[0-9]+ chown("           # Direct chown() syscall (not fchownat)
        "^[0-9]+ lchown("          # Direct lchown() syscall (not fchownat)
        "^[0-9]+ chmod("           # Direct chmod() syscall (not fchmodat)
        "^[0-9]+ readdir("         # Old readdir() syscall
        "^[0-9]+ open("            # Old open() syscall (should use openat)
        "^[0-9]+ lstat("           # Direct lstat() (should use fstatat)
        "^[0-9]+ stat("            # Direct stat() (should use fstatat)
    )

    # Define expected safe syscall patterns
    local safe_patterns=(
        "openat.*O_RDONLY"         # Safe directory opening
        "fchownat"                 # Safe chown operation (for chown/chgrp)
        "fchmodat"                 # Safe chmod operation (for chmod)
        "getdents64"               # Modern directory reading
        "newfstatat.*AT_SYMLINK_NOFOLLOW"  # Safe stat with proper flags
        "statx.*AT_SYMLINK_NOFOLLOW"       # Modern safe stat
    )

    local failed=false

    # Check that strace log exists and is not empty
    if [[ ! -f "$STRACE_LOG" ]]; then
        log_error "Strace log not found: $STRACE_LOG"
        log_error "Command may have failed to run or strace failed to capture output"
        return 1
    fi

    if [[ ! -s "$STRACE_LOG" ]]; then
        log_error "Strace log is empty: $STRACE_LOG"
        log_error "This suggests the command didn't make any relevant syscalls or strace failed"
        return 1
    fi

    # Check for unsafe syscalls
    log_info "Analyzing syscall trace for unsafe patterns..."
    for pattern in "${unsafe_patterns[@]}"; do
        if grep -E "$pattern" "$STRACE_LOG" >/dev/null 2>&1; then
            log_error "Found unsafe syscall pattern in $test_name: $pattern"
            log_error "Matching lines:"
            grep -E "$pattern" "$STRACE_LOG" | head -5
            failed=true
        fi
    done

    # Check that at least some safe syscalls are present (if the command should have done something)
    if [[ "$args" == *"-R"* ]] || [[ "$cmd" == "chown" ]] || [[ "$cmd" == "chmod" ]] || [[ "$cmd" == "chgrp" ]]; then
        log_info "Verifying safe syscalls are present..."
        local found_safe=false

        for pattern in "${safe_patterns[@]}"; do
            if grep -E "$pattern" "$STRACE_LOG" >/dev/null 2>&1; then
                found_safe=true
                break
            fi
        done

        if [[ "$found_safe" == false ]]; then
            log_warn "No expected safe syscalls found in $test_name - this might indicate the command didn't work as expected"
            log_warn "First 20 lines of strace log:"
            if [[ -f "$STRACE_LOG" ]] && [[ -s "$STRACE_LOG" ]]; then
                head -20 "$STRACE_LOG"
            else
                log_warn "Strace log is missing or empty"
            fi
        fi
    fi

    if [[ "$failed" == true ]]; then
        log_error "$test_name FAILED - unsafe syscalls detected"
        return 1
    else
        log_info "$test_name PASSED - only safe syscalls detected"
        return 0
    fi
}

# Main test execution
main() {
    log_info "Starting safe traversal syscall tests"
    log_info "Using binary: $BINARY"
    log_info "Temporary directory: $TEMP_DIR"

    # Create test structure
    create_test_structure "$TEMP_DIR"

    local test_dir="$TEMP_DIR/testdir"
    local overall_result=0

    # Test chown with recursive flag
    if ! test_command_safety "chown" "-R 1000:1000" "$test_dir" "chown recursive"; then
        overall_result=1
    fi

    # Test chown with verbose and recursive flags
    if ! test_command_safety "chown" "-Rv 1000:1000" "$test_dir" "chown recursive verbose"; then
        overall_result=1
    fi

    # Test chown with symlink handling
    if ! test_command_safety "chown" "-RH 1000:1000" "$test_dir" "chown recursive follow command-line symlinks"; then
        overall_result=1
    fi

    # Test chmod with recursive flag
    if ! test_command_safety "chmod" "-R 755" "$test_dir" "chmod recursive"; then
        overall_result=1
    fi

    # Test chmod with verbose and recursive flags
    if ! test_command_safety "chmod" "-Rv 644" "$test_dir" "chmod recursive verbose"; then
        overall_result=1
    fi

    # Test chgrp with recursive flag (if chgrp is available)
    if "$BINARY" --help 2>&1 | grep -q chgrp; then
        if ! test_command_safety "chgrp" "-R 1000" "$test_dir" "chgrp recursive"; then
            overall_result=1
        fi
    fi

    # Summary
    echo
    echo "=============================================="
    if [[ $overall_result -eq 0 ]]; then
        log_info "✅ ALL TESTS PASSED - Safe traversal is working correctly"
    else
        log_error "❌ TESTS FAILED - Unsafe syscalls detected"
        log_error "Check the output above for details"
    fi
    echo "=============================================="

    return $overall_result
}

# Run main function
main "$@"
