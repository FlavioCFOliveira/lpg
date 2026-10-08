//! Spatial values (TS-21, TS-18; FR-TYPE-002, FR-TYPE-004, FR-TYPE-005):
//! points in four coordinate reference systems, their distance, and the
//! Hilbert keys of the point index.
//!
//! - **Points** carry an SRID: 7203 (Cartesian 2D), 9157 (Cartesian 3D),
//!   4326 (WGS-84 2D) or 4979 (WGS-84 3D). WGS-84 coordinates are
//!   longitude, latitude and height in metres.
//! - **Distance** (TS-21): Cartesian, Euclidean; WGS-84 2D, the haversine
//!   great-circle distance on a sphere of radius 6 378 140 m; WGS-84 3D,
//!   that distance at the mean height of the two points, combined with
//!   the height difference by Pythagoras; points in different systems,
//!   none (null). These are Neo4j's formulas (R-NEO
//!   `storable/CRSCalculator.java:130-166`, GPL, studied only), written
//!   here from TS-21's description.
//! - **Hilbert keys** (TS-18 Hilbert keys): 60 bits, 30 per axis in 2D and
//!   20 in 3D, over fixed extents: longitude −180 to 180, latitude −90 to
//!   90, every other axis −1 000 000 to 1 000 000. A coordinate outside its
//!   extent maps to the edge cell; the exact-distance post-filter keeps
//!   results correct. Neo4j uses the same bit budget, extents and clamping
//!   (R-NEO `SpaceFillingCurveSettings.java:53-60, 69`,
//!   `EnvelopeSettings.java:30-35`, studied only). The curve is computed
//!   with Skilling's transpose method for any dimension (J. Skilling,
//!   "Programming the Hilbert curve", AIP Conf. Proc. 707, 381 (2004)),
//!   written here from the published algorithm; `fast_hilbert` and `lindel`
//!   were studied only (TS-18).
//! - **Box queries**: [`ranges`] covers the cells of a query box with
//!   Hilbert key ranges, refining the curve's aligned blocks breadth first
//!   up to a budget of ranges; a block refined no further is kept whole,
//!   which only adds candidates for the post-filter.

use std::cmp::Ordering;

use super::float_cmp;

/// A coordinate reference system (FR-TYPE-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Srid {
    /// Cartesian 2D, SRID 7203.
    Cartesian2D,
    /// Cartesian 3D, SRID 9157.
    Cartesian3D,
    /// WGS-84 2D, SRID 4326.
    Wgs84_2D,
    /// WGS-84 3D, SRID 4979.
    Wgs84_3D,
}

impl Srid {
    /// The SRID code.
    pub(crate) fn code(self) -> u32 {
        match self {
            Srid::Cartesian2D => 7203,
            Srid::Cartesian3D => 9157,
            Srid::Wgs84_2D => 4326,
            Srid::Wgs84_3D => 4979,
        }
    }

    /// The system with SRID `code`.
    pub(crate) fn from_code(code: u32) -> Option<Srid> {
        Some(match code {
            7203 => Srid::Cartesian2D,
            9157 => Srid::Cartesian3D,
            4326 => Srid::Wgs84_2D,
            4979 => Srid::Wgs84_3D,
            _ => return None,
        })
    }

    /// Number of coordinates.
    pub(crate) fn dims(self) -> usize {
        match self {
            Srid::Cartesian2D | Srid::Wgs84_2D => 2,
            Srid::Cartesian3D | Srid::Wgs84_3D => 3,
        }
    }

    fn geographic(self) -> bool {
        matches!(self, Srid::Wgs84_2D | Srid::Wgs84_3D)
    }

    /// The extent of axis `i` for Hilbert keys (TS-18).
    fn extent(self, i: usize) -> (f64, f64) {
        match (self.geographic(), i) {
            (true, 0) => (-180.0, 180.0),
            (true, 1) => (-90.0, 90.0),
            _ => (-1_000_000.0, 1_000_000.0),
        }
    }

    /// Bits per axis of its Hilbert keys: 60 bits in all (TS-18).
    pub(crate) fn bits(self) -> u32 {
        60 / self.dims() as u32
    }
}

/// A point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    srid: Srid,
    /// x, y and z; z is 0 in 2D.
    coords: [f64; 3],
}

impl Point {
    /// A point in `srid` with `coords`, one per dimension; `None` if their
    /// number does not match the system.
    pub(crate) fn new(srid: Srid, coords: &[f64]) -> Option<Point> {
        if coords.len() != srid.dims() {
            return None;
        }
        let mut c = [0.0; 3];
        c[..coords.len()].copy_from_slice(coords);
        Some(Point { srid, coords: c })
    }

    /// The coordinate reference system.
    pub(crate) fn srid(&self) -> Srid {
        self.srid
    }

    /// The coordinates, one per dimension.
    pub(crate) fn coords(&self) -> &[f64] {
        &self.coords[..self.srid.dims()]
    }

    /// The approved order (TS-21): by SRID code, then dimension, then
    /// coordinates in axis order, each as numbers order (NaN highest,
    /// −0.0 = +0.0).
    pub(crate) fn total_cmp(&self, o: &Point) -> Ordering {
        self.srid
            .code()
            .cmp(&o.srid.code())
            .then(self.srid.dims().cmp(&o.srid.dims()))
            .then_with(|| {
                for (a, b) in self.coords().iter().zip(o.coords()) {
                    let c = float_cmp(*a, *b);
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                Ordering::Equal
            })
    }

    /// The lexical form `{srid: S, x: X, y: Y[, z: Z]}` (TS-21).
    pub(crate) fn lexical(&self) -> String {
        let c = self.coords();
        match c {
            [x, y] => format!("{{srid: {}, x: {x:?}, y: {y:?}}}", self.srid.code()),
            [x, y, z] => format!(
                "{{srid: {}, x: {x:?}, y: {y:?}, z: {z:?}}}",
                self.srid.code()
            ),
            _ => unreachable!("2 or 3 dimensions"),
        }
    }

    /// Parses a lexical form of [`Point::lexical`].
    pub(crate) fn parse(text: &str) -> Option<Point> {
        let body = text.trim().strip_prefix('{')?.strip_suffix('}')?;
        let mut srid = None;
        let mut c = [None; 3];
        for part in body.split(',') {
            let (k, v) = part.split_once(':')?;
            let v = v.trim();
            match k.trim() {
                "srid" => srid = Srid::from_code(v.parse().ok()?),
                "x" => c[0] = Some(v.parse().ok()?),
                "y" => c[1] = Some(v.parse().ok()?),
                "z" => c[2] = Some(v.parse().ok()?),
                _ => return None,
            }
        }
        let srid = srid?;
        let coords: Vec<f64> = c[..srid.dims()].iter().copied().collect::<Option<_>>()?;
        if c[srid.dims()..].iter().any(Option::is_some) {
            return None;
        }
        Point::new(srid, &coords)
    }
}

/// Earth radius of the WGS-84 distance, in metres (TS-21).
pub(crate) const EARTH_RADIUS: f64 = 6_378_140.0;

/// The distance between `a` and `b` (TS-21, FR-TYPE-004); `None` (null)
/// for points in different coordinate reference systems.
pub(crate) fn distance(a: &Point, b: &Point) -> Option<f64> {
    if a.srid != b.srid {
        return None;
    }
    let (p, q) = (a.coords(), b.coords());
    if !a.srid.geographic() {
        return Some(
            p.iter()
                .zip(q)
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f64>()
                .sqrt(),
        );
    }
    let (lon1, lat1) = (p[0].to_radians(), p[1].to_radians());
    let (lon2, lat2) = (q[0].to_radians(), q[1].to_radians());
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    let angle = 2.0 * h.sqrt().atan2((1.0 - h).sqrt());
    if a.srid.dims() == 2 {
        return Some(EARTH_RADIUS * angle);
    }
    let mean_height = (p[2] + q[2]) / 2.0;
    let surface = (EARTH_RADIUS + mean_height) * angle;
    let dh = p[2] - q[2];
    Some((surface * surface + dh * dh).sqrt())
}

/// The cell of coordinate `v` on an axis of extent `(lo, hi)` with `bits`
/// bits: outside values, and NaN, clamp to an edge cell (TS-18).
fn cell(v: f64, (lo, hi): (f64, f64), bits: u32) -> u32 {
    let n = 1u64 << bits;
    if v.is_nan() || v <= lo {
        return 0;
    }
    if v >= hi {
        return (n - 1) as u32;
    }
    (((v - lo) / (hi - lo) * n as f64) as u64).min(n - 1) as u32
}

/// The cells of point `p` on its system's grid.
pub(crate) fn cells(p: &Point) -> Vec<u32> {
    let s = p.srid;
    (0..s.dims())
        .map(|i| cell(p.coords[i], s.extent(i), s.bits()))
        .collect()
}

/// The Hilbert key of point `p` (TS-18).
pub(crate) fn key(p: &Point) -> u64 {
    encode(&cells(p), p.srid.bits())
}

/// Skilling's axes-to-transpose step, in place, for `bits` bits per axis.
fn axes_to_transpose(x: &mut [u32], bits: u32) {
    let n = x.len();
    let m = 1u32 << (bits - 1);
    let mut q = m;
    while q > 1 {
        let p = q - 1;
        for i in 0..n {
            if x[i] & q != 0 {
                x[0] ^= p;
            } else {
                let t = (x[0] ^ x[i]) & p;
                x[0] ^= t;
                x[i] ^= t;
            }
        }
        q >>= 1;
    }
    for i in 1..n {
        x[i] ^= x[i - 1];
    }
    let mut t = 0;
    let mut q = m;
    while q > 1 {
        if x[n - 1] & q != 0 {
            t ^= q - 1;
        }
        q >>= 1;
    }
    for v in x.iter_mut() {
        *v ^= t;
    }
}

/// Skilling's transpose-to-axes step, the inverse of
/// [`axes_to_transpose`].
fn transpose_to_axes(x: &mut [u32], bits: u32) {
    let n = x.len();
    let top = 2u64 << (bits - 1);
    let t = x[n - 1] >> 1;
    for i in (1..n).rev() {
        x[i] ^= x[i - 1];
    }
    x[0] ^= t;
    let mut q = 2u64;
    while q != top {
        let p = (q - 1) as u32;
        let qb = q as u32;
        for i in (0..n).rev() {
            if x[i] & qb != 0 {
                x[0] ^= p;
            } else {
                let t = (x[0] ^ x[i]) & p;
                x[0] ^= t;
                x[i] ^= t;
            }
        }
        q <<= 1;
    }
}

/// The Hilbert index of the cell `axes` (each below `2^bits`), with
/// `bits × axes.len()` at most 64.
pub(crate) fn encode(axes: &[u32], bits: u32) -> u64 {
    let mut x = axes.to_vec();
    axes_to_transpose(&mut x, bits);
    let mut h = 0u64;
    for level in (0..bits).rev() {
        for v in &x {
            h = (h << 1) | u64::from((v >> level) & 1);
        }
    }
    h
}

/// The cell of Hilbert index `h` in `dims` dimensions: the inverse of
/// [`encode`].
pub(crate) fn decode(h: u64, dims: usize, bits: u32) -> Vec<u32> {
    let mut x = vec![0u32; dims];
    let mut at = u64::from(bits) * dims as u64;
    for level in (0..bits).rev() {
        for v in x.iter_mut() {
            at -= 1;
            *v |= (((h >> at) & 1) as u32) << level;
        }
    }
    transpose_to_axes(&mut x, bits);
    x
}

/// Most key ranges [`ranges`] returns before it stops refining.
pub(crate) const MAX_RANGES: usize = 64;

/// Hilbert key ranges `[a, b]`, sorted and disjoint, whose union holds the
/// key of every cell `c` with `lo[i] <= c[i] <= hi[i]` on every axis, for a
/// grid of `bits` bits per axis. At most about `max_ranges` ranges: blocks
/// of the curve partly inside the box are refined breadth first while the
/// budget allows, and kept whole after that.
pub(crate) fn ranges(lo: &[u32], hi: &[u32], bits: u32, max_ranges: usize) -> Vec<(u64, u64)> {
    let dims = lo.len();
    let mut out = Vec::new();
    // Blocks: (start index, level); a block of level l holds 2^(dims × l)
    // consecutive indices and is an aligned cube of side 2^l.
    let mut frontier = vec![(0u64, bits)];
    while !frontier.is_empty() {
        let mut partial = Vec::new();
        for (start, level) in frontier {
            let side = 1u64 << level;
            let origin: Vec<u64> = decode(start, dims, bits)
                .iter()
                .map(|&c| u64::from(c) & !(side - 1))
                .collect();
            let mut inside = true;
            let mut disjoint = false;
            for i in 0..dims {
                let (a, b) = (origin[i], origin[i] + side - 1);
                let (l, h) = (u64::from(lo[i]), u64::from(hi[i]));
                if b < l || a > h {
                    disjoint = true;
                }
                if a < l || b > h {
                    inside = false;
                }
            }
            if disjoint {
                continue;
            }
            let size = 1u64 << (dims as u32 * level);
            if inside {
                out.push((start, start + (size - 1)));
            } else {
                partial.push((start, level));
            }
        }
        let children = partial.len() << dims;
        if partial.iter().any(|&(_, l)| l == 0) || out.len() + children > max_ranges {
            for (start, level) in partial {
                let size = 1u64 << (dims as u32 * level);
                out.push((start, start + (size - 1)));
            }
            break;
        }
        frontier = partial
            .into_iter()
            .flat_map(|(start, level)| {
                let child = 1u64 << (dims as u32 * (level - 1));
                (0..1u64 << dims).map(move |k| (start + k * child, level - 1))
            })
            .collect();
    }
    out.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(out.len());
    for (a, b) in out {
        match merged.last_mut() {
            Some(last) if last.1.checked_add(1) == Some(a) => last.1 = b,
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// The Hilbert key ranges of the box between points `lo` and `hi` of one
/// system (TS-18 point index).
pub(crate) fn box_ranges(lo: &Point, hi: &Point) -> Option<Vec<(u64, u64)>> {
    if lo.srid != hi.srid {
        return None;
    }
    let (a, b) = (cells(lo), cells(hi));
    let l: Vec<u32> = a.iter().zip(&b).map(|(x, y)| *x.min(y)).collect();
    let h: Vec<u32> = a.iter().zip(&b).map(|(x, y)| *x.max(y)).collect();
    Some(ranges(&l, &h, lo.srid.bits(), MAX_RANGES))
}
