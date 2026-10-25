//! bcn-oracle random <blocks-per-mode>   random blocks of every format and mode
//! bcn-oracle corpus <dir>               every *.<format>.bcn file of a corpus directory
//! bcn-oracle bench <dir|random>         throughput of both decoders

use bcn_oracle::*;
use std::collections::BTreeMap;
use std::time::Instant;

fn random_blocks(fmt: Fmt, per_mode: usize, rng: &mut Rng) -> Vec<u8> {
    let bb = fmt.block_bytes();
    let mut v = vec![0u8; per_mode * bb];
    rng.fill(&mut v);
    v
}

fn cmd_random(per_mode: usize) {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for fmt in Fmt::ALL {
        let mut stats = BTreeMap::new();
        let mut reasons = BTreeMap::new();
        let bb = fmt.block_bytes();
        // Several "classes" so that every mode gets the same number of blocks
        let classes: Vec<Box<dyn Fn(&mut [u8])>> = match fmt {
            Fmt::Bc7 => (0..8u8)
                .map(|m| Box::new(move |b: &mut [u8]| b[0] = (b[0] & !((1u16 << (m + 1)) as u8).wrapping_sub(1)) | (1 << m)) as Box<dyn Fn(&mut [u8])>)
                .collect(),
            Fmt::Bc6u | Fmt::Bc6s => [0u8, 1, 2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15]
                .into_iter()
                .map(|m| {
                    Box::new(move |b: &mut [u8]| {
                        if m < 2 {
                            b[0] = (b[0] & !3) | m
                        } else {
                            b[0] = (b[0] & !31) | m
                        }
                    }) as Box<dyn Fn(&mut [u8])>
                })
                .collect(),
            _ => vec![Box::new(|_: &mut [u8]| {}) as Box<dyn Fn(&mut [u8])>],
        };
        let per_class = per_mode;
        for class in &classes {
            let mut data = random_blocks(fmt, per_class, &mut rng);
            for b in data.chunks_mut(bb) {
                class(b);
            }
            for chunk in data.chunks(bb * 4096) {
                compare(fmt, chunk, &mut stats);
                if matches!(fmt, Fmt::Bc6u | Fmt::Bc6s) {
                    compare_by(fmt, chunk, &mut reasons, &reason_key);
                }
            }
        }
        report(fmt.name(), &stats);
        if !reasons.is_empty() {
            report(&format!("{} by known deviation", fmt.name()), &reasons);
        }
    }
}

fn cmd_corpus(dir: &str) {
    let mut files: Vec<_> = std::fs::read_dir(dir).expect("corpus dir").flatten().map(|e| e.path()).collect();
    files.sort();
    let mut per_fmt: BTreeMap<Fmt, Report> = BTreeMap::new();
    let mut bytes: BTreeMap<Fmt, u64> = BTreeMap::new();
    for path in files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        // <anything>.<format>.bcn : raw blocks of one format
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() < 3 || *parts.last().unwrap() != "bcn" {
            continue;
        }
        let Some(fmt) = Fmt::from_name(parts[parts.len() - 2]) else { continue };
        let data = std::fs::read(&path).expect("read");
        *bytes.entry(fmt).or_default() += data.len() as u64;
        compare_data(fmt, &data, per_fmt.entry(fmt).or_default());
    }
    for (fmt, rep) in &per_fmt {
        println!("== {} ({} MB of blocks)", fmt.name(), bytes[fmt] >> 20);
        rep.print(fmt.name());
    }
}

fn bench_one(fmt: Fmt, data: &[u8]) {
    let bb = fmt.block_bytes();
    let n = data.len() / bb;
    let (w, h) = (n * 4, 4);
    let px = (n * 16) as f64;
    let reps = 5;
    let mut best_o = f64::MAX;
    let mut best_t = f64::MAX;
    let mut buf = vec![0u8; w * h * 4];
    let mut px32 = vec![0u32; w * h];
    for _ in 0..reps {
        let t = Instant::now();
        bcn_decode::decode_rgba8(fmt.ours_format(), data, w, h, &mut buf).unwrap();
        best_o = best_o.min(t.elapsed().as_secs_f64());
        std::hint::black_box(&buf);
        let t = Instant::now();
        theirs_into(fmt, data, w, h, &mut px32);
        best_t = best_t.min(t.elapsed().as_secs_f64());
        std::hint::black_box(&px32);
    }
    println!(
        "{:<10} {:>9} blocks   ours {:>8.1} Mpx/s   texture2ddecoder {:>8.1} Mpx/s   ratio {:.2}x",
        fmt.name(),
        n,
        px / best_o / 1e6,
        px / best_t / 1e6,
        best_t / best_o
    );
}

fn cmd_bench(src: &str) {
    for fmt in Fmt::ALL {
        let mut data = Vec::new();
        if src == "random" {
            let mut rng = Rng(12345);
            data = random_blocks(fmt, 1 << 20, &mut rng);
        } else {
            for e in std::fs::read_dir(src).expect("dir").flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(&format!(".{}.bcn", fmt.name())) {
                    data.extend(std::fs::read(e.path()).unwrap());
                    if data.len() > 256 << 20 {
                        break;
                    }
                }
            }
            if data.is_empty() {
                continue;
            }
        }
        bench_one(fmt, &data);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("random") => cmd_random(args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000)),
        Some("corpus") => cmd_corpus(args.get(2).expect("dir")),
        Some("bench") => cmd_bench(args.get(2).map(String::as_str).unwrap_or("random")),
        _ => eprintln!("usage: bcn-oracle random [blocks-per-mode] | corpus <dir> | bench <dir|random>"),
    }
}
