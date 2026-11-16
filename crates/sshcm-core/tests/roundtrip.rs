//! The most important property of the parser: reading a config and writing
//! it back without changes reproduces the input byte for byte.

use std::fs;
use std::path::{Path, PathBuf};

use sshcm_core::parser::{parse_lines, render};

fn fixtures() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("fixtures directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &mut out,
    );
    out.sort();
    assert!(out.len() >= 7, "expected the fixture files to be present");
    out
}

#[test]
fn parser_round_trips_every_fixture() {
    for path in fixtures() {
        let input = fs::read_to_string(&path).expect("fixture is UTF-8");
        let output = render(&parse_lines(&input));
        assert_eq!(output.as_bytes(), input.as_bytes(), "{}", path.display());
    }
}

#[test]
fn fixtures_keep_their_special_bytes() {
    // Guards against an editor or git normalising the files that exist to
    // test exactly these cases.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let read = |name: &str| fs::read_to_string(dir.join(name)).expect("fixture");
    assert!(read("crlf.config").contains("\r\n"));
    assert!(!read("no-final-newline.config").ends_with('\n'));
    assert!(read("tabs.config").contains("\tHostName=gitlab.example.com"));
    assert!(read("tabs.config").contains("legacy.example.com   \n"));
}
