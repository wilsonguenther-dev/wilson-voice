//! YV123 — the one decompression path in the app, tested against archives this
//! file BUILDS rather than against a committed binary blob.
//!
//! The fixtures are hand-written ustar streams (the format is 512-byte headers
//! and 512-byte padded payloads) piped through `/usr/bin/bzip2`. Hand-writing
//! them is not stubbornness: `tar::Builder` refuses to emit a `../escape` entry,
//! and an extraction guard that can only be tested against archives a
//! well-behaved writer produces is a guard tested against the wrong input. The
//! happy-path test cross-checks the same fixture with the system `tar`, so a
//! bug in the fixture writer surfaces as a failure here rather than as a
//! green test proving my own mistake back to me.
//!
//! Non-vacuity: deleting the `ParentDir | RootDir | Prefix` arm of
//! `safe_entry_path` makes `rejects_a_parent_dir_escape_entry` and
//! `rejects_an_absolute_path_entry` fail (and the former writes a file outside
//! the destination, which the test also asserts about); deleting the
//! `!kind.is_file()` arm fails `rejects_a_symlink_entry`; returning the first
//! `.onnx` instead of the largest fails
//! `returns_the_full_precision_onnx_not_the_quantized_sibling`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use wilson_voice_lib::models::extract_tar_bz2;

// ---------------------------------------------------------------------------
// Fixture writer: a minimal ustar stream
// ---------------------------------------------------------------------------

const REGULAR: u8 = b'0';
const SYMLINK: u8 = b'2';
const DIRECTORY: u8 = b'5';

fn ustar_entry(name: &str, type_flag: u8, payload: &[u8], mode: u32) -> Vec<u8> {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    assert!(name_bytes.len() < 100, "fixture name too long: {name}");
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    write_octal(&mut header[100..108], mode as u64, 7); // mode
    write_octal(&mut header[108..116], 0, 7); // uid
    write_octal(&mut header[116..124], 0, 7); // gid
    write_octal(&mut header[124..136], payload.len() as u64, 11); // size
    write_octal(&mut header[136..148], 0, 11); // mtime
    header[148..156].fill(b' '); // checksum field is spaces while summing
    header[156] = type_flag;
    header[257..262].copy_from_slice(b"ustar");
    header[262] = 0;
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|b| *b as u32).sum();
    write_octal(&mut header[148..154], checksum as u64, 6);
    header[154] = 0;
    header[155] = b' ';

    let mut out = header.to_vec();
    out.extend_from_slice(payload);
    let padding = (512 - payload.len() % 512) % 512;
    out.extend(std::iter::repeat(0u8).take(padding));
    out
}

/// `digits` octal digits, zero-padded, NUL-terminated when the field has room —
/// the ustar convention. The checksum field is the one that does not (6 digits,
/// then NUL, then space), so its terminator is written by the caller.
fn write_octal(field: &mut [u8], value: u64, digits: usize) {
    let text = format!("{value:0digits$o}", digits = digits);
    assert!(text.len() <= digits, "octal field overflow: {value:o}");
    field[..text.len()].copy_from_slice(text.as_bytes());
    if field.len() > text.len() {
        field[text.len()] = 0;
    }
}

fn tar_bytes(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = entries.concat();
    out.extend(std::iter::repeat(0u8).take(1024)); // two zero blocks = EOF
    out
}

/// Compress with the system `bzip2`. It is a base-system tool on macOS (the
/// only platform this app builds for) and present on every CI image this repo
/// uses, so a missing binary is a loud failure, never a silent skip.
fn bzip2(bytes: &[u8]) -> Vec<u8> {
    let mut child = Command::new("bzip2")
        .arg("-c")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("`bzip2` must be on PATH to build the extraction fixtures");
    child
        .stdin
        .take()
        .expect("bzip2 stdin")
        .write_all(bytes)
        .expect("write fixture to bzip2");
    let out = child.wait_with_output().expect("bzip2 exits");
    assert!(out.status.success(), "bzip2 failed: {:?}", out.status);
    out.stdout
}

fn temp_dir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("yv123-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_archive(dir: &Path, name: &str, entries: &[Vec<u8>]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bzip2(&tar_bytes(entries))).expect("write archive");
    path
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

/// The shape of the real vendored archive: a top-level directory, a
/// full-precision `model.onnx`, a smaller quantized sibling, a LICENSE and an
/// executable helper script.
fn upstream_shaped_entries() -> Vec<Vec<u8>> {
    vec![
        ustar_entry("pkg/", DIRECTORY, b"", 0o755),
        ustar_entry("pkg/model.onnx", REGULAR, &vec![b'F'; 4096], 0o644),
        ustar_entry("pkg/model.int8.onnx", REGULAR, &vec![b'Q'; 1024], 0o644),
        ustar_entry("pkg/LICENSE", REGULAR, b"MIT License\n", 0o644),
        ustar_entry(
            "pkg/export-onnx.py",
            REGULAR,
            b"#!/usr/bin/env python3\n",
            0o755,
        ),
    ]
}

#[test]
fn the_fixture_writer_emits_an_archive_the_system_tar_agrees_with() {
    let dir = temp_dir("fixture-check");
    let archive = write_archive(&dir, "pkg.tar.bz2", &upstream_shaped_entries());
    let listing = Command::new("tar")
        .args(["-tjf", archive.to_str().unwrap()])
        .output()
        .expect("system tar runs");
    assert!(listing.status.success(), "system tar rejected the fixture");
    let names = String::from_utf8_lossy(&listing.stdout);
    for expected in [
        "pkg/",
        "pkg/model.onnx",
        "pkg/model.int8.onnx",
        "pkg/LICENSE",
        "pkg/export-onnx.py",
    ] {
        assert!(
            names.contains(expected),
            "system tar did not list {expected}:\n{names}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn returns_the_full_precision_onnx_not_the_quantized_sibling() {
    let dir = temp_dir("happy");
    let archive = write_archive(&dir, "pkg.tar.bz2", &upstream_shaped_entries());
    let dest = dir.join("out");

    let onnx = extract_tar_bz2(&archive, &dest).expect("extraction succeeds");

    assert_eq!(onnx, dest.join("pkg/model.onnx"));
    assert_eq!(std::fs::read(&onnx).unwrap(), vec![b'F'; 4096]);
    // Everything else in the archive lands too, unmodified.
    assert_eq!(
        std::fs::read(dest.join("pkg/model.int8.onnx")).unwrap(),
        vec![b'Q'; 1024]
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("pkg/LICENSE")).unwrap(),
        "MIT License\n"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn nothing_extracted_is_executable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("modes");
    let archive = write_archive(&dir, "pkg.tar.bz2", &upstream_shaped_entries());
    let dest = dir.join("out");

    // Re-extraction over an existing tree is the case that makes this a real
    // guard rather than a restatement of the umask: `File::create` truncates an
    // existing file and leaves its MODE alone, so an executable bit that got
    // there once would survive every later extraction. (The upstream archive
    // marks its `.py` helpers 0o755, and Yap has no reason to unpack anything
    // runnable.)
    let script = dest.join("pkg/export-onnx.py");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, b"stale").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    extract_tar_bz2(&archive, &dest).expect("extraction succeeds");

    for name in ["pkg/export-onnx.py", "pkg/model.onnx", "pkg/LICENSE"] {
        let mode = std::fs::metadata(dest.join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o111,
            0,
            "{name} is executable after extraction: {mode:o}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Rejections — the reason this path is written by hand rather than delegated
// to `Entry::unpack_in`
// ---------------------------------------------------------------------------

#[test]
fn rejects_a_parent_dir_escape_entry() {
    let dir = temp_dir("escape");
    let archive = write_archive(
        &dir,
        "evil.tar.bz2",
        &[ustar_entry("../escape.txt", REGULAR, b"pwned", 0o644)],
    );
    let dest = dir.join("out");

    let err = extract_tar_bz2(&archive, &dest).expect_err("`..` must be refused");
    assert!(
        err.contains("escapes the extraction directory"),
        "unexpected error: {err}"
    );
    // And the point of the guard: nothing was written outside `dest`.
    assert!(
        !dir.join("escape.txt").exists(),
        "the escaping entry was written to {}",
        dir.join("escape.txt").display()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_an_absolute_path_entry() {
    let dir = temp_dir("absolute");
    // Short on purpose: a ustar name field is 100 bytes, and the point is the
    // leading `/`, not the depth.
    let victim = PathBuf::from(format!("/tmp/yv123-victim-{}.txt", std::process::id()));
    assert!(!victim.exists(), "stale fixture at {}", victim.display());
    let archive = write_archive(
        &dir,
        "evil.tar.bz2",
        &[ustar_entry(
            victim.to_str().unwrap(),
            REGULAR,
            b"pwned",
            0o644,
        )],
    );
    let dest = dir.join("out");

    let err = extract_tar_bz2(&archive, &dest).expect_err("an absolute path must be refused");
    assert!(
        err.contains("escapes the extraction directory"),
        "unexpected error: {err}"
    );
    assert!(!victim.exists(), "the absolute-path entry was written");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_a_symlink_entry() {
    let dir = temp_dir("symlink");
    let archive = write_archive(
        &dir,
        "evil.tar.bz2",
        &[
            ustar_entry("pkg/model.onnx", REGULAR, &vec![b'F'; 512], 0o644),
            ustar_entry("pkg/link", SYMLINK, b"", 0o777),
        ],
    );
    let dest = dir.join("out");

    let err = extract_tar_bz2(&archive, &dest).expect_err("a symlink entry must be refused");
    assert!(
        err.contains("not a regular file or directory"),
        "unexpected error: {err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_an_archive_with_no_onnx() {
    let dir = temp_dir("no-onnx");
    let archive = write_archive(
        &dir,
        "pkg.tar.bz2",
        &[ustar_entry(
            "pkg/README.md",
            REGULAR,
            b"nothing here\n",
            0o644,
        )],
    );
    let dest = dir.join("out");

    let err = extract_tar_bz2(&archive, &dest).expect_err("an .onnx-less archive is an error");
    assert!(
        err.contains("contains no .onnx file"),
        "unexpected error: {err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A verified sha256 proves the bytes are the ones the catalog pinned. It says
/// nothing about what they expand to — so the extractor carries its own cap,
/// and it is applied to the size the ARCHIVE CLAIMS, before a single byte of
/// that entry is written.
#[test]
fn rejects_an_archive_that_claims_to_expand_past_the_cap() {
    let dir = temp_dir("bomb");
    // A header that claims 300 MB with no payload behind it: the cap has to
    // fire on the claim, not after the disk fills.
    let mut entry = ustar_entry("pkg/model.onnx", REGULAR, b"", 0o644);
    write_octal(&mut entry[124..136], 300 * 1024 * 1024, 11);
    let checksum: u32 = {
        let mut header = entry[..512].to_vec();
        header[148..156].fill(b' ');
        header.iter().take(512).map(|b| *b as u32).sum()
    };
    write_octal(&mut entry[148..154], checksum as u64, 6);
    entry[154] = 0;
    entry[155] = b' ';
    let archive = write_archive(&dir, "bomb.tar.bz2", &[entry]);

    let err = extract_tar_bz2(&archive, &dir.join("out"))
        .expect_err("a claimed 300 MB expansion must be refused");
    assert!(err.contains("extraction cap"), "unexpected error: {err}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_bytes_that_are_not_bzip2() {
    let dir = temp_dir("garbage");
    let archive = dir.join("not-really.tar.bz2");
    std::fs::write(&archive, b"this is not a bzip2 stream").unwrap();

    let err = extract_tar_bz2(&archive, &dir.join("out")).expect_err("garbage must not extract");
    assert!(!err.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_a_missing_archive() {
    let dir = temp_dir("missing");
    let err = extract_tar_bz2(&dir.join("nope.tar.bz2"), &dir.join("out"))
        .expect_err("a missing archive is an error, not an empty extraction");
    assert!(err.contains("open"), "unexpected error: {err}");
    std::fs::remove_dir_all(&dir).ok();
}
