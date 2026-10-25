#!/usr/bin/env python3
"""Generates src/tables.rs from the Khronos Data Format Specification (HTML build).

    python3 -I tools/gen_tables.py path/to/dataformat.1.3.html tests/common/spec_tables.rs > src/tables.rs

Source: Khronos Data Format Specification v1.3.1, section 20 (BPTC Compressed Texture Image
Formats): Table 114 (2-subset partitions), Table 115 (3-subset partitions), Tables 116-118
(anchor indices), Tables 122-124 (BC6H block layouts). The specification itself is not
part of this repository; download it from
https://registry.khronos.org/DataFormat/specs/1.3/dataformat.1.3.html

The script cross-checks the specification against itself (anchors printed in bold in
Tables 114/115 versus Tables 116-118; the BC6H bit-by-bit Tables 123/124 versus the
"vector" notation of Table 122) and aborts if the two disagree.
"""
import html
import re
import sys

NBSP = "\xa0"
src = open(sys.argv[1], encoding="utf-8").read()


def table_html(anchor_id=None, title=None):
    if anchor_id:
        i = src.find('id="%s"' % anchor_id)
    else:
        i = src.find("<strong>Table" + NBSP + title)
    assert i >= 0, (anchor_id, title)
    j = src.find("</table>", i)
    return src[i:j]


def cell_text(c):
    return html.unescape(re.sub(r"<[^>]+>", "", c)).strip()


# ---- Tables 114 / 115: partitions, 8 partitions side by side -------------------------------
def partitions(anchor_id):
    t = table_html(anchor_id)
    bands = []
    for r in re.findall(r"<tr>(.*?)</tr>", t, re.S):
        tds = re.findall(r"<td([^>]*)>(.*?)</td>", r, re.S)
        tds = [(a, b) for a, b in tds if "rowspan" not in a]  # drop the spacer columns
        txt = [cell_text(b) for a, b in tds]
        if any("colspan" in a for a, b in tds):
            bands.append({"ids": [int(x) for x in txt], "rows": []})
        else:
            bands[-1]["rows"].append((txt, [("<em>" in b or "<strong>" in b) for a, b in tds]))
    grids, bold, allbold = {}, {}, {}
    for b in bands:
        assert len(b["rows"]) == 4
        for k, pid in enumerate(b["ids"]):
            g, an, ab = [], {}, []
            for y, (txt, bl) in enumerate(b["rows"]):
                assert len(txt) == 32
                for x in range(4):
                    v = int(txt[k * 4 + x])
                    g.append(v)
                    if bl[k * 4 + x]:
                        an[v] = y * 4 + x
                        ab.append(y * 4 + x)
            grids[pid], bold[pid], allbold[pid] = g, an, ab
    assert sorted(grids) == list(range(64))
    return [grids[i] for i in range(64)], [bold[i] for i in range(64)], [allbold[i] for i in range(64)]


# ---- Tables 116-118: anchors, 8 per row pair --------------------------------------------
def anchors(title_prefix):
    t = table_html(title=title_prefix)
    rows = [[cell_text(c) for c in re.findall(r"<t[dh][^>]*>(.*?)</t[dh]>", r, re.S)]
            for r in re.findall(r"<tr>(.*?)</tr>", t, re.S)]
    out = {}
    for ids, vals in zip(rows[0::2], rows[1::2]):
        for a, b in zip(ids, vals):
            out[int(a)] = int(b)
    assert sorted(out) == list(range(64))
    return [out[i] for i in range(64)]


p2, b2, bold2 = partitions("bptcP2subset")
p3, b3, bold3 = partitions("bptcP3subset")
a3_2 = anchors("116.")
a3_3 = anchors("117.")
a2 = anchors("118.")
# The cells printed in bold in Tables 114/115 mark the anchors. The printed specification has a
# few slips there, which Tables 116-118 (the explicit anchor lists, used below) do not share:
#   Table 114, partition 23: texel 2 is bold as well as the anchor 15;
#   Table 114, partition 35: texel 2 is bold instead of the anchor 8 that Table 118 lists;
#   Table 115, partition 1: texel 15 is bold as well as the anchors 3 and 8.
# The check below therefore only requires every listed anchor to be printed in bold, bar these.
BOLD_SLIPS = {("2", 35)}
for p in range(64):
    # The anchor of each subset must lie in that subset
    assert p2[p][a2[p]] == 1 and p3[p][a3_2[p]] == 1 and p3[p][a3_3[p]] == 2, p
    assert p2[p][0] == 0 and p3[p][0] == 0
    assert set(p2[p]) == {0, 1} and set(p3[p]) == {0, 1, 2}
    # Anchors must not precede the first texel of their subset
    assert a2[p] >= p2[p].index(1) and a3_2[p] >= p3[p].index(1) and a3_3[p] >= p3[p].index(2), p
    # Bold cells (value -> last bold texel) versus the lists
    if ("2", p) not in BOLD_SLIPS:
        assert b2[p].get(1) == a2[p] or a2[p] in bold2[p], ("2-subset anchor not bold", p)
    assert a3_2[p] in bold3[p] and a3_3[p] in bold3[p], ("3-subset anchor not bold", p)

# ---- Tables 123 / 124: BC6H layout, bit by bit ---------------------------------------------
MODES = [0, 1, 2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15]  # column order of the tables
layout = {m: {} for m in MODES}
for tit in ("123.", "124."):
    t = table_html(title=tit)
    for r in re.findall(r"<tr>(.*?)</tr>", t, re.S):
        cells = [cell_text(c) for c in re.findall(r"<t[dh][^>]*>(.*?)</t[dh]>", r, re.S)]
        if not cells or not cells[0].isdigit() or len(cells) != 15:
            continue
        bit = int(cells[0])
        for m, c in zip(MODES, cells[1:]):
            layout[m][bit] = c
for m in MODES:
    assert sorted(layout[m]) == list(range(82)), (m, len(layout[m]))


def parse_field(s):
    """'G24' -> ('G', 2, 4); 'R010' -> ('R', 0, 10); 'M1: 0', 'PB3', 'IB0,02' -> other."""
    mt = re.fullmatch(r"([RGB])([0-3])(\d+)", s)
    if mt:
        return mt.group(1), int(mt.group(2)), int(mt.group(3))
    if re.fullmatch(r"M[0-4]: [01]", s) or re.fullmatch(r"PB[0-4]", s) or re.fullmatch(r"IB\d,\d+", s):
        return None
    raise SystemExit("unparsed field " + s)


# Cross-check against the vector notation of Table 122: 'v a..b' reads b-a+1 bits and puts them
# in bits a..b of v (reversed when a < b); a single 'va' is one bit.
t122 = table_html(title="122.")
desc = {}
for r in re.findall(r"<tr>(.*?)</tr>", t122, re.S):
    cells = [cell_text(c) for c in re.findall(r"<t[dh][^>]*>(.*?)</t[dh]>", r, re.S)]
    if len(cells) == 2 and cells[0].isdigit():
        desc[int(cells[0])] = re.sub(r"\s+", " ", cells[1])
assert sorted(desc) == sorted(MODES)
for m in MODES:
    pos = 0
    from_122 = {}
    for item in desc[m].split(","):
        item = item.strip()
        mt = re.fullmatch(r"([A-Z]+)([0-3]?)(\d+)(?:\.\.(\d+))?", item)
        assert mt, (m, item)
        name, ep, a, b = mt.group(1), mt.group(2), int(mt.group(3)), mt.group(4)
        if name in ("R", "G", "B") and ep == "" :
            raise SystemExit("bad item %r" % item)
        # items like 'R09..0': name R, ep 0, a=9, b=0  (regex above splits 'R' '0' '9')
        if name == "M" or name == "PB":
            n = (a - int(b) + 1) if b is not None else 1
            if name == "M":
                pos += n
                continue
        if name == "PB":
            pos += n
            continue
        if name in ("R", "G", "B"):
            hi = a
            lo = int(b) if b is not None else a
            n = abs(hi - lo) + 1
            for k in range(n):
                bitidx = (lo + k) if hi >= lo else (lo - k)
                # bit position pos+k holds bit (lo + k) of v when a>=b? The first block bit goes to
                # the LSB-most listed index: for 'v9..0' block bit pos+0 -> v bit 0.
                if hi >= lo:
                    vb = lo + k
                else:
                    vb = lo - k
                from_122[pos + k] = (name, int(ep), vb)
            pos += n
            continue
        raise SystemExit("unhandled item %r in mode %d" % (item, m))
    for bit in range(82):
        f = parse_field(layout[m][bit])
        if f is None:
            assert bit not in from_122 or False, (m, bit, layout[m][bit], from_122.get(bit))
        else:
            assert from_122.get(bit) == f, ("Table 122 vs 123/124", m, bit, layout[m][bit], from_122.get(bit))
    # and every field bit listed in 122 appears in 123/124
    for bit, f in from_122.items():
        assert parse_field(layout[m][bit]) == f

# ---- Emit ------------------------------------------------------------------------------
def runs_for(mode):
    """Groups consecutive block bits that land on consecutive bits of one endpoint value."""
    out = []
    cur = None
    for bit in range(82):
        f = parse_field(layout[mode][bit])
        if f is None:
            cur = None
            continue
        ch, ep, vb = f
        slot = "RGB".index(ch) * 4 + ep
        if cur and cur[2] == slot and cur[4] + cur[1] == vb and cur[0] + cur[1] == bit:
            cur[1] += 1
        else:
            cur = [bit, 1, slot, None, vb]
            cur[3] = None
            out.append(cur)
    # (src, len, slot, _, dst)
    return [(r[0], r[1], r[2], r[4]) for r in out]


print("// @generated by tools/gen_tables.py from the Khronos Data Format Specification 1.3.1,")
print("// section 20 (BPTC): Tables 114-118 and 122-124. Do not edit by hand.")
print()
print("/// Table 114 (partitions 0..=63 of two subsets): subset of each texel in raster order.")
print("pub(crate) static PARTITION2: [[u8; 16]; 64] = [")
for g in p2:
    print("    %s," % g)
print("];\n")
print("/// Table 115 (partitions 0..=63 of three subsets).")
print("pub(crate) static PARTITION3: [[u8; 16]; 64] = [")
for g in p3:
    print("    %s," % g)
print("];\n")
print("/// Table 118: anchor texel of subset 1 for two subsets (subset 0 is always texel 0).")
print("pub(crate) static ANCHOR2: [u8; 64] = %s;\n" % a2)
print("/// Table 116: anchor texel of subset 1 for three subsets.")
print("pub(crate) static ANCHOR3_1: [u8; 64] = %s;\n" % a3_2)
print("/// Table 117: anchor texel of subset 2 for three subsets.")
print("pub(crate) static ANCHOR3_2: [u8; 64] = %s;\n" % a3_3)
print("/// One run of block bits of a BC6H mode: `len` bits starting at block bit `src` go to bits")
print("/// `dst..dst+len` of endpoint value `slot` (channel * 4 + endpoint: R0..R3, G0..G3, B0..B3).")
print("#[derive(Clone, Copy)]")
print("pub(crate) struct Bc6hRun {")
print("    pub src: u8,")
print("    pub len: u8,")
print("    pub slot: u8,")
print("    pub dst: u8,")
print("}\n")
print("/// Tables 122-124, bits 0..=81 of each BC6H mode in the order of `BC6H_MODE_NUMBERS`; the mode")
print("/// bits, the partition bits (PB) and the index bits (IB) are not included.")
print("pub(crate) static BC6H_RUNS: [&[Bc6hRun]; 14] = [")
for m in MODES:
    print("    // mode %d" % m)
    print("    &[")
    for (s, l, sl, d) in runs_for(m):
        print("        Bc6hRun { src: %d, len: %d, slot: %d, dst: %d }," % (s, l, sl, d))
    print("    ],")
print("];\n")
print("/// The mode numbers of Table 121 in the column order of Tables 123/124.")
print("pub(crate) static BC6H_MODE_NUMBERS: [u8; 14] = %s;" % MODES)


# ---- tests/spec_tables.rs: BC7 Tables 109, 111, 112 and 113 for the table-driven tests ----------
def bc7_tables():
    # Table 109: NS PB RB ISB CB AB EPB SPB IB IB2 per mode
    t = table_html(title="109.")
    params = {}
    for r in re.findall(r"<tr>(.*?)</tr>", t, re.S):
        c = cell_text_list(r)
        if len(c) >= 11 and all(x.isdigit() for x in c[:11]) and int(c[0]) < 8 and len(c) == 18:
            params[int(c[0])] = [int(x) for x in c[1:11]]
    assert sorted(params) == list(range(8)), params

    # Table 111: bit layout per mode
    t = table_html(title="111.")
    layout = {}
    mode = None
    seq = {}
    for r in re.findall(r"<tr>(.*?)</tr>", t, re.S):
        for c in cell_text_list(r):
            m = re.fullmatch(r"Mode\s+(\d)", c.replace(NBSP, " "))
            if m:
                mode = int(m.group(1))
                layout[mode] = []
                seq = {"R": 0, "G": 0, "B": 0}
                continue
            if not c:
                continue
            m = re.fullmatch(r"(\d+)(?:\.\.(\d+))?: (M|PB|RB|ISB|IB2|IB|EPB\d|SPB\d|[RGBA]\d)(.*)", c)
            assert m, (mode, c)
            lo = int(m.group(1))
            hi = int(m.group(2)) if m.group(2) else lo
            name, rest = m.group(3), m.group(4)
            if name[0] in "RGB" and len(name) == 2 and name[1].isdigit():
                # The printed index of mode 2's fields 3 and 4 is "4" twice (a slip in the
                # specification: the fields are R3, R4, R5 in order). Name them by position.
                name = name[0] + str(seq[name[0]])
                seq[name[0]] += 1
            layout[mode].append((name, lo, hi))
    assert sorted(layout) == list(range(8))
    # Table 112/113: where every bit of every endpoint comes from
    ends = {}
    for tit in ("112.", "113."):
        t = table_html(title=tit)
        mode = None
        for r in re.findall(r"<tr>(.*?)</tr>", t, re.S):
            c = cell_text_list(r)
            if len(c) == 1 and c[0].startswith("Mode "):
                mode = int(c[0].split()[1])
                ends[mode] = []
            elif c and not c[0].startswith("ER") and mode is not None and all(x.isdigit() for x in c):
                v = [int(x) for x in c]
                if len(v) == 25:  # alpha is the single constant 255
                    assert v[24] == 255
                    v = v[:24] + [255] * 8
                assert len(v) == 32, (mode, len(v))
                ends[mode].append(v)
    for m in range(8):
        assert len(ends[m]) == 2 * params[m][0], (m, len(ends[m]))
    return params, layout, ends


def cell_text_list(r):
    return [cell_text(c) for c in re.findall(r"<t[dh][^>]*>(.*?)</t[dh]>", r, re.S)]


if len(sys.argv) > 2:
    params, layout, ends = bc7_tables()
    with open(sys.argv[2], "w") as f:
        f.write("// @generated by tools/gen_tables.py from the Khronos Data Format Specification 1.3.1,\n")
        f.write("// section 20.1 (BC7): Tables 109, 111, 112 and 113. Do not edit by hand.\n")
        f.write("#![allow(dead_code)]\n\n")
        f.write("/// One field of Table 111: block bits `lo..=hi` (LSB first) hold the field `name`.\n")
        f.write("pub struct Field {\n    pub name: &'static str,\n    pub lo: u8,\n    pub hi: u8,\n}\n\n")
        f.write("/// Table 109: NS, PB, RB, ISB, CB, AB, EPB, SPB, IB, IB2 for each mode.\n")
        f.write("pub static BC7_PARAMS: [[u8; 10]; 8] = %s;\n\n" % [params[m] for m in range(8)])
        f.write("/// Table 111 for each mode.\n")
        f.write("pub static BC7_LAYOUT: [&[Field]; 8] = [\n")
        for m in range(8):
            f.write("    &[\n")
            for (n, lo, hi) in layout[m]:
                f.write('        Field { name: "%s", lo: %d, hi: %d },\n' % (n, lo, hi))
            f.write("    ],\n")
        f.write("];\n\n")
        f.write("/// Tables 112 and 113: for each mode and endpoint, the block bit that gives each of the\n")
        f.write("/// 8 bits (MSB first) of R, G, B and A of the decoded 8-bit endpoint; 255 means a constant 1.\n")
        f.write("pub static BC7_ENDPOINTS: [&[[u8; 32]]; 8] = [\n")
        for m in range(8):
            f.write("    &[\n")
            for e in ends[m]:
                f.write("        %s,\n" % e)
            f.write("    ],\n")
        f.write("];\n\n")
        f.write("/// Table 122: the block description of every BC6H mode (LSB first), as printed.\n")
        f.write("pub static BC6H_DESCRIPTIONS: [(u8, &str); 14] = [\n")
        for m in MODES:
            f.write('    (%d, "%s"),\n' % (m, desc[m]))
        f.write("];\n")
