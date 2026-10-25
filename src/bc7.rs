//! BC7 (BPTC, eight modes).
//!
//! Sources: Khronos Data Format Specification 1.3.1 (DFS) section 20.1 "BC7" (Tables 109 to 120)
//! and Microsoft's "BC7 Format" and "BC7 Format Mode Reference" (D3D11 BC7). Partition patterns
//! and anchor indices are generated into `tables.rs` by `tools/gen_tables.py` from DFS Tables 114
//! to 118.
//!
//! BC7 decoding is bit-exact by specification ("BC7 decompression hardware must be bit accurate",
//! D3D11 BC7), so there is no rounding choice here. A block whose low byte is zero (mode 8 or
//! above) is reserved and decodes to (0, 0, 0, 0) (DFS 20.1; D3D11 BC7 mode reference "Remarks").

use crate::tables::{ANCHOR2, ANCHOR3_1, ANCHOR3_2, PARTITION2, PARTITION3};

/// One row of DFS Table 109 (mode-dependent BPTC parameters).
struct Mode {
    /// NS: number of subsets.
    ns: usize,
    /// PB: partition selection bits.
    pb: u32,
    /// RB: rotation bits.
    rb: u32,
    /// ISB: index selection bit (0 or 1 bit).
    isb: u32,
    /// CB: color bits per channel and endpoint.
    cb: u32,
    /// AB: alpha bits per endpoint (0: no alpha in the endpoints, alpha is 255).
    ab: u32,
    /// EPB: 1 if each endpoint has its own P-bit.
    epb: bool,
    /// SPB: 1 if each subset has one shared P-bit.
    spb: bool,
    /// IB: index bits per texel.
    ib: u32,
    /// IB2: secondary index bits per texel (0: none).
    ib2: u32,
}

static MODES: [Mode; 8] = [
    Mode {
        ns: 3,
        pb: 4,
        rb: 0,
        isb: 0,
        cb: 4,
        ab: 0,
        epb: true,
        spb: false,
        ib: 3,
        ib2: 0,
    },
    Mode {
        ns: 2,
        pb: 6,
        rb: 0,
        isb: 0,
        cb: 6,
        ab: 0,
        epb: false,
        spb: true,
        ib: 3,
        ib2: 0,
    },
    Mode {
        ns: 3,
        pb: 6,
        rb: 0,
        isb: 0,
        cb: 5,
        ab: 0,
        epb: false,
        spb: false,
        ib: 2,
        ib2: 0,
    },
    Mode {
        ns: 2,
        pb: 6,
        rb: 0,
        isb: 0,
        cb: 7,
        ab: 0,
        epb: true,
        spb: false,
        ib: 2,
        ib2: 0,
    },
    Mode {
        ns: 1,
        pb: 0,
        rb: 2,
        isb: 1,
        cb: 5,
        ab: 6,
        epb: false,
        spb: false,
        ib: 2,
        ib2: 3,
    },
    Mode {
        ns: 1,
        pb: 0,
        rb: 2,
        isb: 0,
        cb: 7,
        ab: 8,
        epb: false,
        spb: false,
        ib: 2,
        ib2: 2,
    },
    Mode {
        ns: 1,
        pb: 0,
        rb: 0,
        isb: 0,
        cb: 7,
        ab: 7,
        epb: true,
        spb: false,
        ib: 4,
        ib2: 0,
    },
    Mode {
        ns: 2,
        pb: 6,
        rb: 0,
        isb: 0,
        cb: 5,
        ab: 5,
        epb: true,
        spb: false,
        ib: 2,
        ib2: 0,
    },
];

/// Interpolation weights for 2, 3 and 4 index bits (DFS Table 119).
const W2: [u32; 4] = [0, 21, 43, 64];
const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// DFS Equation 2: `((64 - weight) * E0 + weight * E1 + 32) >> 6`.
#[inline]
fn interpolate(e0: u32, e1: u32, index: u32, index_bits: u32) -> u32 {
    let w = match index_bits {
        2 => W2[(index & 3) as usize],
        3 => W3[(index & 7) as usize],
        _ => W4[(index & 15) as usize],
    };
    ((64 - w) * e0 + w * e1 + 32) >> 6
}

#[inline]
fn bits(v: u128, pos: u32, n: u32) -> u32 {
    // n is at most 8 and pos stays below 128 for every layout in DFS Table 111; the checked shift
    // keeps even a wrong table from panicking
    (v.checked_shr(pos).unwrap_or(0) & ((1u128 << n) - 1)) as u32
}

/// Decodes a BC7 block to sixteen RGBA8 texels in raster order.
pub fn decode_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let v = u128::from_le_bytes(*block);

    // The mode is the number of zero bits below the lowest set bit of the first byte (DFS 20.1:
    // "encoded as zero or more zeros followed by a one"). A zero byte is reserved: all channels 0.
    let low = v as u8;
    if low == 0 {
        return [[0; 4]; 16];
    }
    let mode = low.trailing_zeros() as usize;
    let m = &MODES[mode];
    let mut pos = mode as u32 + 1;

    // Fields in increasing bit order after the mode: partition, rotation, index selection,
    // colour, alpha, per-endpoint P-bits, shared P-bits, primary indices, secondary indices
    // (DFS 20.1).
    let part = bits(v, pos, m.pb) as usize;
    pos += m.pb;
    let rotation = bits(v, pos, m.rb);
    pos += m.rb;
    let index_select = bits(v, pos, m.isb) != 0;
    pos += m.isb;

    let n_ep = m.ns * 2;
    // Endpoint values are stored grouped by channel, then by endpoint (all R, then all G, ...)
    let mut ep = [[0u32; 4]; 6];
    for ch in 0..3 {
        for e in ep.iter_mut().take(n_ep) {
            e[ch] = bits(v, pos, m.cb);
            pos += m.cb;
        }
    }
    if m.ab > 0 {
        for e in ep.iter_mut().take(n_ep) {
            e[3] = bits(v, pos, m.ab);
            pos += m.ab;
        }
    }

    // P-bits are appended below the colour (and alpha, where the mode has it) value of each
    // endpoint, "so, for mode 1 with six red bits, the P-bit ends up in bit 1" (DFS 20.1).
    let mut cprec = m.cb;
    let mut aprec = m.ab;
    if m.epb || m.spb {
        let mut pbit = [0u32; 6];
        if m.epb {
            for p in pbit.iter_mut().take(n_ep) {
                *p = bits(v, pos, 1);
                pos += 1;
            }
        } else {
            for s in 0..m.ns {
                let b = bits(v, pos, 1);
                pos += 1;
                pbit[2 * s] = b;
                pbit[2 * s + 1] = b;
            }
        }
        cprec += 1;
        for (e, &p) in ep.iter_mut().zip(pbit.iter()).take(n_ep) {
            for c in e.iter_mut().take(3) {
                *c = (*c << 1) | p;
            }
            if m.ab > 0 {
                e[3] = (e[3] << 1) | p;
            }
        }
        if m.ab > 0 {
            aprec += 1;
        }
    }

    // Scale to 8 bits: shift the value's MSB to bit 7, then replicate the MSBs into the bits that
    // the shift revealed (D3D11 BC7 pseudocode, fully_decode_endpoints)
    for e in ep.iter_mut().take(n_ep) {
        for c in e.iter_mut().take(3) {
            *c = (*c << (8 - cprec)) | (*c >> (2 * cprec - 8));
        }
        e[3] = if m.ab > 0 {
            (e[3] << (8 - aprec)) | (e[3] >> (2 * aprec - 8))
        } else {
            255
        };
    }

    // Anchor texels: texel 0 for subset 0; Tables 118, 116 and 117 for the other subsets
    let anchors: [usize; 3] = match m.ns {
        1 => [0, usize::MAX, usize::MAX],
        2 => [0, ANCHOR2[part] as usize, usize::MAX],
        _ => [0, ANCHOR3_1[part] as usize, ANCHOR3_2[part] as usize],
    };
    let subset_of = |i: usize| -> usize {
        match m.ns {
            1 => 0,
            2 => PARTITION2[part][i] as usize,
            _ => PARTITION3[part][i] as usize,
        }
    };

    // Index sets. Each index has IB bits except the anchor of its subset, which has one fewer
    // (its top bit is implicitly zero). The secondary set (modes 4 and 5) has a single subset.
    let mut primary = [0u32; 16];
    for (i, idx) in primary.iter_mut().enumerate() {
        let s = subset_of(i);
        let n = if i == anchors[s] { m.ib - 1 } else { m.ib };
        *idx = bits(v, pos, n);
        pos += n;
    }
    let mut secondary = [0u32; 16];
    if m.ib2 > 0 {
        for (i, idx) in secondary.iter_mut().enumerate() {
            let n = if i == 0 { m.ib2 - 1 } else { m.ib2 };
            *idx = bits(v, pos, n);
            pos += n;
        }
    }

    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        let s = subset_of(i);
        let (e0, e1) = (&ep[2 * s], &ep[2 * s + 1]);

        // The colour index comes from the secondary set if the mode has an index selection bit
        // and it is one, else from the primary set; the alpha index from the secondary set if
        // there is one and the selection bit is absent or zero, else from the primary set
        // (DFS 20.1).
        let (cidx, cbits, aidx, abits) = if m.ib2 == 0 {
            (primary[i], m.ib, primary[i], m.ib)
        } else if index_select {
            (secondary[i], m.ib2, primary[i], m.ib)
        } else {
            (primary[i], m.ib, secondary[i], m.ib2)
        };

        let mut c = [
            interpolate(e0[0], e1[0], cidx, cbits),
            interpolate(e0[1], e1[1], cidx, cbits),
            interpolate(e0[2], e1[2], cidx, cbits),
            interpolate(e0[3], e1[3], aidx, abits),
        ];
        // Table 120: rotation swaps alpha with red, green or blue
        if rotation != 0 {
            c.swap(3, rotation as usize - 1);
        }
        *px = [c[0] as u8, c[1] as u8, c[2] as u8, c[3] as u8];
    }
    out
}
