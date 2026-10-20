//! Test inputs shaped like a UART's RX: shell text, a TUI redrawing itself, random bytes.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A boot log: timestamps, a few recurring subsystems and words.
pub fn text(len: usize, seed: u64) -> Vec<u8> {
    const SUBSYS: &[&str] = &["usb 1-1", "systemd[1]", "kernel", "NetworkManager[812]", "pci 0000:00:1f.3"];
    const WORDS: &[&str] = &[
        "Started", "device", "new", "full-speed", "USB", "number", "using", "xhci_hcd", "Reached", "target", "Mounting",
        "configuration", "interface", "driver", "registered", "link", "is", "up", "carrier", "acquired", "Finished",
        "of", "the", "for", "with", "service", "socket", "0x1f", "enabled", "ok",
    ];
    let mut r = Rng::new(seed);
    let mut out = Vec::with_capacity(len + 128);
    let mut t = 0u64;
    while out.len() < len {
        t += r.below(40_000) as u64;
        out.extend_from_slice(format!("[{:5}.{:06}] {}: ", t / 1_000_000, t % 1_000_000, SUBSYS[r.below(SUBSYS.len())]).as_bytes());
        for i in 0..3 + r.below(9) {
            if i > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(WORDS[r.below(WORDS.len())].as_bytes());
        }
        out.extend_from_slice(b"\r\n");
    }
    out.truncate(len);
    out
}

/// `top`-like redraws: home the cursor, colour escapes, the same rows with a few numbers changed each frame.
pub fn tui(len: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng::new(seed);
    let mut out = Vec::with_capacity(len + 4096);
    while out.len() < len {
        out.extend_from_slice(b"\x1b[H\x1b[1mtop - load average: ");
        out.extend_from_slice(format!("{}.{:02}\x1b[m\x1b[K\r\n", r.below(4), r.below(100)).as_bytes());
        out.extend_from_slice(b"\x1b[7m    PID USER      PR  NI    VIRT    RES  %CPU  %MEM COMMAND\x1b[m\x1b[K\r\n");
        for row in 0..30 {
            out.extend_from_slice(
                format!(
                    "\x1b[{};1H{:7} roman     20   0 {:7} {:6} {:5.1} {:5.1} proc{}\x1b[K",
                    row + 3,
                    1000 + row * 37,
                    100_000 + row * 4096,
                    r.below(90_000),
                    r.below(1000) as f32 / 10.0,
                    row as f32 / 3.0,
                    row
                )
                .as_bytes(),
            );
        }
    }
    out.truncate(len);
    out
}

pub fn random(len: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng::new(seed);
    (0..len).map(|_| r.next() as u8).collect()
}
