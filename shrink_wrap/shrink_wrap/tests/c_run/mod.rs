//! Builds a C test program of the generated readers and runs it on cases from the Rust side: with
//! AddressSanitizer and UBSan where the compiler has them, so a read past the end of a buffer fails the test.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `tests/<c_file>` built with every warning an error (and the sanitizers, when they link), into the test's own
/// target directory.
pub fn build(c_file: &str) -> PathBuf {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let exe = Path::new(env!("CARGO_TARGET_TMPDIR")).join(c_file.trim_end_matches(".c"));
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let compile = |sanitize: bool| {
        let mut cmd = Command::new(&cc);
        cmd.args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-Wconversion",
            "-Wsign-conversion",
            "-g",
        ]);
        if sanitize {
            cmd.args(["-fsanitize=address,undefined", "-fno-sanitize-recover=all"]);
        }
        cmd.arg("-I")
            .arg(&tests)
            .arg("-o")
            .arg(&exe)
            .arg(tests.join(c_file));
        cmd.output()
            .unwrap_or_else(|e| panic!("{cc}: {e} (these tests need a C compiler, `CC` or `cc`)"))
    };
    let out = compile(true);
    if !out.status.success() {
        let plain = compile(false);
        assert!(
            plain.status.success(),
            "{c_file} doesn't build:\n{}",
            String::from_utf8_lossy(&plain.stderr)
        );
        eprintln!(
            "note: {c_file} built without sanitizers, reads past the end are not caught here: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    exe
}

/// Runs `exe` with no arguments (its own checks) and with `-` on `input` (one case per line); its output.
pub fn run(exe: &Path, input: &str) -> String {
    let own = Command::new(exe).output().unwrap();
    assert!(
        own.status.success(),
        "{} failed its own checks:\n{}{}",
        exe.display(),
        String::from_utf8_lossy(&own.stdout),
        String::from_utf8_lossy(&own.stderr)
    );
    let mut child = Command::new(exe)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // fed from a thread: the program answers while it reads, a full pipe either way would stall both
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_string();
    let feed = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().unwrap();
    feed.join().unwrap().unwrap();
    assert!(
        out.status.success(),
        "{} failed on the cases:\n{}",
        exe.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A case line: the C name of the type and the bytes in hex (`-` for none).
pub fn line(ty: &str, bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{ty} {}\n", if hex.is_empty() { "-".into() } else { hex })
}

/// What the C program must print for a case: `-1` refused, else the length of the value.
pub fn expect(len: Option<usize>) -> String {
    match len {
        Some(n) => format!("{n}"),
        None => "-1".into(),
    }
}

/// xorshift, so the cases cover every byte without a dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// From `valid` (encodings of values): each, every truncation of it, copies with bytes changed or added, and
/// random buffers up to `max` bytes; `(type, bytes)`.
pub fn cases(
    ty: &'static str,
    valid: &[Vec<u8>],
    max: usize,
    rng: &mut Rng,
) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    for v in valid {
        for cut in 0..=v.len() {
            out.push((ty, v[..cut].to_vec()));
        }
        let mut longer = v.clone();
        longer.push(rng.next() as u8);
        out.push((ty, longer));
        for _ in 0..4 {
            let mut m = v.clone();
            if !m.is_empty() {
                let i = rng.next() as usize % m.len();
                m[i] ^= 1 << (rng.next() % 8);
            }
            let cut = rng.next() as usize % (m.len() + 1);
            out.push((ty, m[..cut].to_vec()));
            out.push((ty, m));
        }
    }
    for _ in 0..200 {
        let len = rng.next() as usize % (max + 1);
        out.push((ty, (0..len).map(|_| rng.next() as u8).collect()));
    }
    out
}
