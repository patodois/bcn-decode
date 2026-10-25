//! Walks every block-compressed texture of a local install and compares bcn-decode with
//! texture2ddecoder block by block, without keeping the data. Optionally stores a deterministic
//! sample (the raw blocks, no headers) in a directory that must stay out of git: it is game content.
//!
//!   bcn-corpus wf <Cache.Windows> [--store DIR --every N] [--limit N]
//!   bcn-corpus wt <game dir>      [--store DIR --every N] [--limit N]
//!
//! Memory: one texture at a time (Warframe) or one memory-mapped pack plus one texture (War Thunder).

use bcn_oracle::{compare_data, Fmt, Report};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Sink {
    dir: Option<PathBuf>,
    every: u64,
    game: &'static str,
    files: BTreeMap<Fmt, (u32, u64, fs::File)>,
    stored: BTreeMap<Fmt, u64>,
}

impl Sink {
    fn wants(&self, name: &str) -> bool {
        self.dir.is_some() && fnv(name.as_bytes()) % self.every == 0
    }
    fn store(&mut self, fmt: Fmt, data: &[u8]) {
        let Some(dir) = &self.dir else { return };
        let game = self.game;
        let entry = self.files.entry(fmt).or_insert_with(|| {
            (0, 0, fs::File::create(dir.join(format!("{game}-0.{}.bcn", fmt.name()))).unwrap())
        });
        if entry.1 > 256 << 20 {
            entry.0 += 1;
            entry.1 = 0;
            entry.2 = fs::File::create(dir.join(format!("{game}-{}.{}.bcn", entry.0, fmt.name()))).unwrap();
        }
        entry.2.write_all(data).unwrap();
        entry.1 += data.len() as u64;
        *self.stored.entry(fmt).or_default() += data.len() as u64;
    }
}

fn fnv(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn peak_rss_mb() -> u64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    (ru.ru_maxrss as u64) >> 20 // bytes on macOS
}

struct Totals {
    reports: BTreeMap<Fmt, Report>,
    textures: BTreeMap<Fmt, u64>,
    bytes: BTreeMap<Fmt, u64>,
    skipped: BTreeMap<String, u64>,
}

impl Totals {
    fn new() -> Self {
        Totals { reports: BTreeMap::new(), textures: BTreeMap::new(), bytes: BTreeMap::new(), skipped: BTreeMap::new() }
    }
    /// Compares the blocks of one texture; BC1 is compared both as opaque and with alpha.
    fn add(&mut self, fmts: &[Fmt], data: &[u8], key: Fmt) {
        *self.bytes.entry(key).or_default() += data.len() as u64;
        for &f in fmts {
            compare_data(f, data, self.reports.entry(f).or_default());
        }
    }
    fn print(&self, title: &str, secs: f64) {
        println!("\n######## {title}  ({secs:.0} s, peak RSS {} MB)", peak_rss_mb());
        for (f, n) in &self.textures {
            println!("{}: {n} textures, {} MB of blocks", f.name(), self.bytes[f] >> 20);
        }
        for (k, n) in &self.skipped {
            println!("skipped: {n} x {k}");
        }
        for (f, r) in &self.reports {
            let (b, i) = r.blocks();
            println!("\n== {}: {i} of {b} blocks identical ({:.5}%)", f.name(), 100.0 * i as f64 / b.max(1) as f64);
            r.print(f.name());
        }
    }
}

fn wf(dir: &Path, sink: &mut Sink, limit: usize) -> anyhow::Result<()> {
    use wf_unpack::cache::Package;
    use wf_unpack::texture::{Format, TextureHeader};
    let t0 = Instant::now();
    let mut tot = Totals::new();
    for family in ["Texture", "LightMap"] {
        let Some(mut head) = Package::open_in(dir, &format!("H.{family}"))? else { continue };
        let mut body = Package::open_in(dir, &format!("B.{family}"))?;
        let mut full = Package::open_in(dir, &format!("F.{family}"))?;
        let entries = head.toc.entries.clone();
        for (n, e) in entries.iter().enumerate() {
            if limit > 0 && n >= limit {
                break;
            }
            let raw = match head.read(e) {
                Ok(r) => r,
                Err(_) => {
                    *tot.skipped.entry("unreadable header".into()).or_default() += 1;
                    continue;
                }
            };
            let Ok(h) = TextureHeader::parse(&raw) else {
                *tot.skipped.entry("not a texture header".into()).or_default() += 1;
                continue;
            };
            let (fmts, key): (Vec<Fmt>, Fmt) = match h.format {
                Some(Format::Bc1) => (vec![Fmt::Bc1, Fmt::Bc1a], Fmt::Bc1),
                Some(Format::Bc2) => (vec![Fmt::Bc2], Fmt::Bc2),
                Some(Format::Bc3) => (vec![Fmt::Bc3], Fmt::Bc3),
                Some(Format::Bc4) => (vec![Fmt::Bc4], Fmt::Bc4),
                Some(Format::Bc5) => (vec![Fmt::Bc5], Fmt::Bc5),
                Some(Format::Bc6h) => (vec![Fmt::Bc6u], Fmt::Bc6u), // the game ships UF16 only
                Some(Format::Bc7) => (vec![Fmt::Bc7], Fmt::Bc7),
                _ => {
                    *tot.skipped.entry(format!("{family} format code {}", h.format_code)).or_default() += 1;
                    continue;
                }
            };
            *tot.textures.entry(key).or_default() += 1;
            // The B. and F. parts hold the mip levels back to back; whatever the texture type
            // (2D, array, cube map, probe) they are plain runs of blocks.
            for part in [&mut body, &mut full] {
                let Some(pkg) = part else { continue };
                let Ok(Some(data)) = pkg.read_path(&e.path) else { continue };
                let bb = fmts[0].block_bytes();
                let data = &data[..data.len() / bb * bb];
                tot.add(&fmts, data, key);
                if sink.wants(&e.path) {
                    sink.store(key, data);
                }
            }
            if n % 20000 == 0 && n > 0 {
                eprintln!("[wf {family}] {n}/{} entries, {:.0} s, peak RSS {} MB", entries.len(), t0.elapsed().as_secs_f64(), peak_rss_mb());
            }
        }
    }
    tot.print("Warframe", t0.elapsed().as_secs_f64());
    Ok(())
}

fn wt(dir: &Path, sink: &mut Sink, limit: usize) -> anyhow::Result<()> {
    use wt_blk::dxp_and_grp::dxp::DxpPack;
    let t0 = Instant::now();
    let mut packs = Vec::new();
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.to_string_lossy().ends_with(".dxp.bin") {
                out.push(p);
            }
        }
    }
    walk(dir, &mut packs);
    packs.sort();
    let mut tot = Totals::new();
    let fcc = |s: &[u8; 4]| u32::from_le_bytes(*s);
    for (pi, path) in packs.iter().enumerate() {
        if limit > 0 && pi >= limit {
            break;
        }
        let file = fs::File::open(path)?;
        let map = unsafe { memmap2::Mmap::map(&file)? };
        let Ok(pack) = DxpPack::parse(&map) else {
            *tot.skipped.entry("unparsable pack".into()).or_default() += 1;
            continue;
        };
        for (i, entry) in pack.entries.iter().enumerate() {
            let f = entry.header.d3d_format;
            let (fmts, key): (Vec<Fmt>, Fmt) = if f == fcc(b"DXT1") {
                (vec![Fmt::Bc1, Fmt::Bc1a], Fmt::Bc1)
            } else if f == fcc(b"DXT3") {
                (vec![Fmt::Bc2], Fmt::Bc2)
            } else if f == fcc(b"DXT5") {
                (vec![Fmt::Bc3], Fmt::Bc3)
            } else if f == fcc(b"ATI1") {
                (vec![Fmt::Bc4], Fmt::Bc4)
            } else if f == fcc(b"ATI2") {
                (vec![Fmt::Bc5], Fmt::Bc5)
            } else if f == fcc(b"BC7 ") {
                (vec![Fmt::Bc7], Fmt::Bc7)
            } else {
                *tot.skipped.entry(format!("d3d format {f:#x}")).or_default() += 1;
                continue;
            };
            let tex = match pack.texture(i) {
                Ok(t) => t,
                Err(e) => {
                    let msg = format!("{e}");
                    *tot.skipped.entry(msg.chars().take(60).collect()).or_default() += 1;
                    continue;
                }
            };
            *tot.textures.entry(key).or_default() += 1;
            let bb = fmts[0].block_bytes();
            let data = &tex.data[..tex.data.len() / bb * bb];
            tot.add(&fmts, data, key);
            if sink.wants(&entry.name) {
                sink.store(key, data);
            }
        }
        if pi % 50 == 0 {
            eprintln!("[wt] pack {pi}/{} {:.0} s, peak RSS {} MB", packs.len(), t0.elapsed().as_secs_f64(), peak_rss_mb());
        }
    }
    tot.print("War Thunder", t0.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let limit: usize = flag("--limit").and_then(|s| s.parse().ok()).unwrap_or(0);
    let every: u64 = flag("--every").and_then(|s| s.parse().ok()).unwrap_or(100);
    let dir = flag("--store").map(PathBuf::from);
    if let Some(d) = &dir {
        fs::create_dir_all(d)?;
    }
    let game = args.get(1).map(String::as_str).unwrap_or("");
    let path = PathBuf::from(args.get(2).cloned().unwrap_or_default());
    let mut sink = Sink {
        dir,
        every,
        game: if game == "wf" { "wf" } else { "wt" },
        files: BTreeMap::new(),
        stored: BTreeMap::new(),
    };
    match game {
        "wf" => wf(&path, &mut sink, limit)?,
        "wt" => wt(&path, &mut sink, limit)?,
        _ => eprintln!("usage: bcn-corpus wf|wt <dir> [--store DIR --every N] [--limit N]"),
    }
    for (f, n) in &sink.stored {
        println!("stored {} MB of {}", n >> 20, f.name());
    }
    Ok(())
}
