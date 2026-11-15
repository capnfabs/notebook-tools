//! The macOS binary links only what ships with the OS, so the `.app` needs no bundled libraries.
//! See `decisions/2026-11-01-heic-through-imageio.md`.
#![cfg(target_os = "macos")]

use std::process::Command;

#[test]
fn links_only_system_libraries() {
    let binary = env!("CARGO_BIN_EXE_notebook-tools");
    let output = Command::new("otool")
        .arg("-L")
        .arg(binary)
        .output()
        .unwrap();
    assert!(output.status.success(), "otool -L failed: {output:?}");
    // The first line names the binary; each other is `\t<path> (compatibility version …)`.
    let libraries: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect();
    assert!(!libraries.is_empty());
    let outside: Vec<&String> = libraries
        .iter()
        .filter(|lib| !lib.starts_with("/System/Library/") && !lib.starts_with("/usr/lib/"))
        .collect();
    assert!(outside.is_empty(), "links {outside:?}");
}
