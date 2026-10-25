//! Test oracle for bcn-decode: runs `texture2ddecoder` as a black box next to bcn-decode and
//! compares the outputs block by block. It is not part of the crate.

use bcn_decode::Format;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Fmt {
    Bc1,
    Bc1a,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6u,
    Bc6s,
    Bc7,
}

impl Fmt {
    pub const ALL: [Fmt; 9] =
        [Fmt::Bc1, Fmt::Bc1a, Fmt::Bc2, Fmt::Bc3, Fmt::Bc4, Fmt::Bc5, Fmt::Bc6u, Fmt::Bc6s, Fmt::Bc7];
    pub fn name(self) -> &'static str {
        match self {
            Fmt::Bc1 => "bc1",
            Fmt::Bc1a => "bc1a",
            Fmt::Bc2 => "bc2",
            Fmt::Bc3 => "bc3",
            Fmt::Bc4 => "bc4",
            Fmt::Bc5 => "bc5",
            Fmt::Bc6u => "bc6h-uf16",
            Fmt::Bc6s => "bc6h-sf16",
            Fmt::Bc7 => "bc7",
        }
    }
    pub fn from_name(s: &str) -> Option<Fmt> {
        Fmt::ALL.iter().copied().find(|f| f.name() == s)
    }
    pub fn block_bytes(self) -> usize {
        match self {
            Fmt::Bc1 | Fmt::Bc1a | Fmt::Bc4 => 8,
            _ => 16,
        }
    }
    pub fn ours_format(self) -> Format {
        match self {
            Fmt::Bc1 => Format::Bc1,
            Fmt::Bc1a => Format::Bc1Alpha,
            Fmt::Bc2 => Format::Bc2,
            Fmt::Bc3 => Format::Bc3,
            Fmt::Bc4 => Format::Bc4,
            Fmt::Bc5 => Format::Bc5,
            Fmt::Bc6u => Format::Bc6hUf16,
            Fmt::Bc6s => Format::Bc6hSf16,
            Fmt::Bc7 => Format::Bc7,
        }
    }
}

/// Decodes `data` (any number of blocks) as one strip with our crate: RGBA8.
pub fn ours(fmt: Fmt, data: &[u8], w: usize, h: usize) -> Vec<u8> {
    bcn_decode::decode_rgba8_to_vec(fmt.ours_format(), data, w, h).expect("ours")
}

/// Decodes with texture2ddecoder; pixels come out as BGRA in a u32, converted to RGBA bytes.
pub fn theirs(fmt: Fmt, data: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut px = vec![0u32; w * h];
    let r = match fmt {
        Fmt::Bc1 => texture2ddecoder::decode_bc1(data, w, h, &mut px),
        Fmt::Bc1a => texture2ddecoder::decode_bc1a(data, w, h, &mut px),
        Fmt::Bc2 => texture2ddecoder::decode_bc2(data, w, h, &mut px),
        Fmt::Bc3 => texture2ddecoder::decode_bc3(data, w, h, &mut px),
        Fmt::Bc4 => texture2ddecoder::decode_bc4(data, w, h, &mut px),
        Fmt::Bc5 => texture2ddecoder::decode_bc5(data, w, h, &mut px),
        Fmt::Bc6u => texture2ddecoder::decode_bc6_unsigned(data, w, h, &mut px),
        Fmt::Bc6s => texture2ddecoder::decode_bc6_signed(data, w, h, &mut px),
        Fmt::Bc7 => texture2ddecoder::decode_bc7(data, w, h, &mut px),
    };
    r.expect("theirs");
    let mut out = Vec::with_capacity(w * h * 4);
    for p in px {
        let [b, g, r, a] = p.to_le_bytes();
        out.extend_from_slice(&[r, g, b, a]);
    }
    out
}

/// xorshift64* — a fixed seed makes every run reproducible
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn fill(&mut self, buf: &mut [u8]) {
        for c in buf.chunks_mut(8) {
            let v = self.next().to_le_bytes();
            c.copy_from_slice(&v[..c.len()]);
        }
    }
}

/// A key that groups blocks for the report: the BC7 mode, the BC6H mode, or the format alone.
pub fn mode_key(fmt: Fmt, block: &[u8]) -> String {
    match fmt {
        Fmt::Bc7 => {
            if block[0] == 0 {
                "reserved".into()
            } else {
                format!("mode {}", block[0].trailing_zeros())
            }
        }
        Fmt::Bc6u | Fmt::Bc6s => {
            let m = if block[0] & 3 < 2 { block[0] & 3 } else { block[0] & 31 };
            format!("mode {:02}", m)
        }
        Fmt::Bc1 | Fmt::Bc1a => {
            let c0 = u16::from_le_bytes([block[0], block[1]]);
            let c1 = u16::from_le_bytes([block[2], block[3]]);
            if c0 > c1 { "4-colour".into() } else { "3-colour".into() }
        }
        Fmt::Bc2 | Fmt::Bc3 => {
            let c = &block[8..];
            let c0 = u16::from_le_bytes([c[0], c[1]]);
            let c1 = u16::from_le_bytes([c[2], c[3]]);
            if c0 > c1 { "colour c0>c1".into() } else { "colour c0<=c1".into() }
        }
        Fmt::Bc4 | Fmt::Bc5 => {
            if block[0] > block[1] { "r0>r1".into() } else { "r0<=r1".into() }
        }
    }
}

/// For BC6H: which of the known deviations of texture2ddecoder from the specification apply to a
/// block (see the report). "none" means we expect both decoders to agree.
pub fn reason_key(fmt: Fmt, block: &[u8]) -> String {
    let signed = match fmt {
        Fmt::Bc6u => false,
        Fmt::Bc6s => true,
        _ => return String::new(),
    };
    let Ok(b16) = <&[u8; 16]>::try_from(block) else { return String::new() };
    let Some(info) = bcn_decode::bc6h::parse_block(b16, signed) else { return "reserved".into() };
    let mut r: Vec<&str> = vec![];
    let n_ep = if info.partition.is_some() { 4 } else { 2 };
    let epb = info.endpoint_bits;
    if info.mode == 15 {
        r.push("16-bit mode");
    }
    // Unsigned: texture2ddecoder saturates the half-way value 1 << (epb - 1) instead of the
    // all-ones value (1 << epb) - 1. Signed: endpoints whose magnitude saturates in the DFS
    // pseudocode (|x| >= (1 << (epb - 1)) - 1), listed to see whether they matter there.
    let extreme = |v: i32| {
        if signed {
            v.abs() >= (1 << (epb - 1)) - 1
        } else {
            v == 1 << (epb - 1) || v == (1 << epb) - 1
        }
    };
    if epb < 15 && info.endpoints[..n_ep].iter().any(|e| e.iter().any(|&v| extreme(v))) {
        r.push(if signed { "saturating endpoint" } else { "half-way or all-ones endpoint" });
    }
    if signed {
        let mixed = (0..n_ep / 2).any(|s| {
            (0..3).any(|c| {
                let (a, b) = (info.endpoints[2 * s][c], info.endpoints[2 * s + 1][c]);
                (a < 0) != (b < 0)
            })
        });
        if mixed {
            r.push("mixed signs");
        }
    }
    if r.is_empty() { "none".into() } else { r.join(" + ") }
}

#[derive(Default, Clone)]
pub struct Stat {
    pub blocks: u64,
    pub identical: u64,
    pub max_diff: u32,
    /// histogram of the largest per-channel difference of a block (0..=255)
    pub hist: BTreeMap<u32, u64>,
    pub first_diffs: Vec<(Vec<u8>, usize, [u8; 4], [u8; 4])>,
}

/// Compares `n` blocks (decoded as one strip), updating `stats` keyed by mode
pub fn compare(fmt: Fmt, data: &[u8], stats: &mut BTreeMap<String, Stat>) {
    compare_by(fmt, data, stats, &mode_key)
}

pub fn compare_by(fmt: Fmt, data: &[u8], stats: &mut BTreeMap<String, Stat>, key: &dyn Fn(Fmt, &[u8]) -> String) {
    let bb = fmt.block_bytes();
    let n = data.len() / bb;
    let (w, h) = (n * 4, 4);
    let a = ours(fmt, data, w, h);
    let b = theirs(fmt, data, w, h);
    for i in 0..n {
        let blk = &data[i * bb..(i + 1) * bb];
        let st = stats.entry(key(fmt, blk)).or_default();
        st.blocks += 1;
        let mut worst = 0u32;
        let mut first: Option<(usize, [u8; 4], [u8; 4])> = None;
        for y in 0..4 {
            for x in 0..4 {
                let o = (y * w + i * 4 + x) * 4;
                let (pa, pb) = (&a[o..o + 4], &b[o..o + 4]);
                if pa != pb {
                    let d = (0..4).map(|c| (pa[c] as i32 - pb[c] as i32).unsigned_abs()).max().unwrap_or(0);
                    worst = worst.max(d);
                    if first.is_none() {
                        first = Some((y * 4 + x, [pa[0], pa[1], pa[2], pa[3]], [pb[0], pb[1], pb[2], pb[3]]));
                    }
                }
            }
        }
        if worst == 0 {
            st.identical += 1;
        } else {
            *st.hist.entry(worst).or_default() += 1;
            st.max_diff = st.max_diff.max(worst);
            if st.first_diffs.len() < 3 {
                let (px, pa, pb) = first.unwrap();
                st.first_diffs.push((blk.to_vec(), px, pa, pb));
            }
        }
    }
}

pub fn report(title: &str, stats: &BTreeMap<String, Stat>) {
    let (mut tb, mut ti) = (0u64, 0u64);
    for s in stats.values() {
        tb += s.blocks;
        ti += s.identical;
    }
    println!("{title}: {ti}/{tb} blocks identical ({:.4}%)", 100.0 * ti as f64 / tb.max(1) as f64);
    for (k, s) in stats {
        let pct = 100.0 * s.identical as f64 / s.blocks.max(1) as f64;
        let hist: Vec<String> = s.hist.iter().take(6).map(|(d, c)| format!("d{d}:{c}")).collect();
        println!(
            "    {k:<14} {:>10} blocks  {:>10} identical  {pct:>8.4}%  max diff {:>3}  {}",
            s.blocks,
            s.identical,
            s.max_diff,
            hist.join(" ")
        );
        for (blk, px, pa, pb) in &s.first_diffs {
            let hex: String = blk.iter().map(|b| format!("{b:02x}")).collect();
            println!("        e.g. {hex} texel {px}: ours {pa:?} theirs {pb:?}");
        }
    }
}


/// Statistics of one format, by mode and (BC6H only) by known deviation.
#[derive(Default)]
pub struct Report {
    pub by_mode: BTreeMap<String, Stat>,
    pub by_reason: BTreeMap<String, Stat>,
}

impl Report {
    pub fn print(&self, name: &str) {
        report(name, &self.by_mode);
        if !self.by_reason.is_empty() {
            report(&format!("{name} by known deviation"), &self.by_reason);
        }
    }
    pub fn blocks(&self) -> (u64, u64) {
        let b = self.by_mode.values().map(|s| s.blocks).sum();
        let i = self.by_mode.values().map(|s| s.identical).sum();
        (b, i)
    }
}

/// Compares any number of blocks of one format, in pieces of 4096 blocks.
pub fn compare_data(fmt: Fmt, data: &[u8], rep: &mut Report) {
    let bb = fmt.block_bytes();
    for chunk in data.chunks(bb * 4096) {
        let chunk = &chunk[..chunk.len() / bb * bb];
        if chunk.is_empty() {
            continue;
        }
        compare(fmt, chunk, &mut rep.by_mode);
        if matches!(fmt, Fmt::Bc6u | Fmt::Bc6s) {
            compare_by(fmt, chunk, &mut rep.by_reason, &reason_key);
        }
    }
}

/// Decodes with texture2ddecoder into an existing buffer (for benchmarks).
pub fn theirs_into(fmt: Fmt, data: &[u8], w: usize, h: usize, px: &mut [u32]) {
    let _ = match fmt {
        Fmt::Bc1 => texture2ddecoder::decode_bc1(data, w, h, px),
        Fmt::Bc1a => texture2ddecoder::decode_bc1a(data, w, h, px),
        Fmt::Bc2 => texture2ddecoder::decode_bc2(data, w, h, px),
        Fmt::Bc3 => texture2ddecoder::decode_bc3(data, w, h, px),
        Fmt::Bc4 => texture2ddecoder::decode_bc4(data, w, h, px),
        Fmt::Bc5 => texture2ddecoder::decode_bc5(data, w, h, px),
        Fmt::Bc6u => texture2ddecoder::decode_bc6_unsigned(data, w, h, px),
        Fmt::Bc6s => texture2ddecoder::decode_bc6_signed(data, w, h, px),
        Fmt::Bc7 => texture2ddecoder::decode_bc7(data, w, h, px),
    };
}
