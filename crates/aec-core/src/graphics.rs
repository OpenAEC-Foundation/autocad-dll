//! The graphics an object carries with it: what its owning application drew of it, kept in the
//! drawing so that other programs can show the object. A stream of records: primitives (shells,
//! meshes, polygons, polylines, circles, arcs), the traits that apply to the primitives after them
//! (colour, fill) and a stack of transforms.
//!
//! Layout: `u32` total size, `u32` record count, then per record `u32` size (header included),
//! `u32` type and the payload. All numbers little-endian; points are three `f64`.

use crate::geometry::{Geometry, Paint, Polyline};
use crate::matrix::Matrix;

/// Why a graphics stream was not decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphicsError {
    /// Shorter than its own header, or a record runs past the end.
    Truncated,
    /// A record says it is smaller than a record header.
    BadRecord,
}

impl std::fmt::Display for GraphicsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "graphics stream is truncated"),
            Self::BadRecord => write!(f, "graphics record with an impossible size"),
        }
    }
}

impl std::error::Error for GraphicsError {}

/// Record types of the stream.
pub mod record {
    pub const EXTENTS: u32 = 1;
    pub const CIRCLE: u32 = 2;
    pub const CIRCLE_3_POINTS: u32 = 3;
    pub const ARC: u32 = 4;
    pub const ARC_3_POINTS: u32 = 5;
    pub const POLYLINE: u32 = 6;
    pub const POLYGON: u32 = 7;
    pub const MESH: u32 = 8;
    pub const SHELL: u32 = 9;
    pub const TEXT: u32 = 10;
    pub const TEXT_2: u32 = 11;
    pub const XLINE: u32 = 12;
    pub const RAY: u32 = 13;
    pub const COLOR: u32 = 14;
    pub const LAYER: u32 = 16;
    pub const LINETYPE: u32 = 18;
    pub const MARKER: u32 = 19;
    pub const FILL: u32 = 20;
    pub const TRUE_COLOR: u32 = 22;
    pub const LINEWEIGHT: u32 = 23;
    pub const LINETYPE_SCALE: u32 = 24;
    pub const THICKNESS: u32 = 25;
    pub const PLOT_STYLE: u32 = 26;
    pub const PUSH_CLIP: u32 = 27;
    pub const POP_CLIP: u32 = 28;
    pub const PUSH_TRANSFORM: u32 = 29;
    pub const PUSH_TRANSFORM_2: u32 = 30;
    pub const POP_TRANSFORM: u32 = 31;
    pub const POLYLINE_WITH_NORMAL: u32 = 32;
    pub const LIGHTWEIGHT_POLYLINE: u32 = 33;
    pub const MATERIAL: u32 = 34;
    pub const MAPPER: u32 = 35;
    pub const UNICODE_TEXT: u32 = 36;
    pub const UNICODE_TEXT_2: u32 = 38;
}

/// The records of a stream: type and payload. Stops at the first record that does not fit.
pub fn records(data: &[u8]) -> Result<Vec<(u32, &[u8])>, GraphicsError> {
    let mut cursor = Cursor::new(data);
    let total = cursor.u32().ok_or(GraphicsError::Truncated)? as usize;
    let _count = cursor.u32().ok_or(GraphicsError::Truncated)?;
    if total < 8 || total > data.len() {
        return Err(GraphicsError::Truncated);
    }
    let mut out = Vec::new();
    let mut at = 8usize;
    while at < total {
        let mut head = Cursor::new(&data[at..total]);
        let (Some(size), Some(kind)) = (head.u32(), head.u32()) else { return Err(GraphicsError::Truncated) };
        let size = size as usize;
        if size < 8 {
            return Err(GraphicsError::BadRecord);
        }
        let end = at.checked_add(size).filter(|end| *end <= total).ok_or(GraphicsError::Truncated)?;
        out.push((kind, &data[at + 8..end]));
        at = end;
    }
    Ok(out)
}

/// How many segments a full circle is drawn with.
const CIRCLE_SEGMENTS: usize = 32;
/// Deepest transform stack a stream may build.
const MAX_STACK: usize = 64;

/// Decodes a graphics stream into triangles and lines, transforms applied.
///
/// Shells, meshes and polygons become triangles; polylines, circles and arcs become lines. Text,
/// clip boundaries, lightweight polylines, rays and construction lines are skipped. A primitive
/// whose own payload is damaged is skipped, the rest of the stream is still read.
pub fn decode(data: &[u8]) -> Result<Geometry, GraphicsError> {
    let mut out = Geometry::default();
    let mut paint = Paint::Object;
    let mut stack: Vec<Matrix> = Vec::new();
    for (kind, payload) in records(data)? {
        let transform = stack.last().copied().unwrap_or(Matrix::IDENTITY);
        let mut cursor = Cursor::new(payload);
        match kind {
            record::COLOR => {
                if let Some(index) = cursor.i32() {
                    paint = match index {
                        0 => Paint::ByBlock,
                        1..=255 => Paint::Index(index as u8),
                        _ => Paint::ByLayer,
                    };
                }
            }
            record::TRUE_COLOR => {
                if let Some(value) = cursor.u32() {
                    // The top byte says what the lower three hold.
                    paint = match value >> 24 {
                        0xC0 => Paint::ByLayer,
                        0xC1 => Paint::ByBlock,
                        0xC3 => Paint::Index((value & 0xFF) as u8),
                        _ => Paint::Rgb([(value >> 16) as u8, (value >> 8) as u8, value as u8]),
                    };
                }
            }
            record::PUSH_TRANSFORM | record::PUSH_TRANSFORM_2 => {
                let local = read_matrix(&mut cursor).unwrap_or(Matrix::IDENTITY);
                if stack.len() < MAX_STACK {
                    stack.push(transform.mul(&local));
                }
            }
            record::POP_TRANSFORM => {
                stack.pop();
            }
            record::SHELL => {
                let _ = shell(&mut cursor, &transform, paint, &mut out);
            }
            record::MESH => {
                let _ = mesh(&mut cursor, &transform, paint, &mut out);
            }
            record::POLYGON => {
                if let Some(points) = counted_points(&mut cursor) {
                    let loops = [points.iter().map(|p| transform.point(*p)).collect::<Vec<_>>()];
                    out.add_face(&loops, paint);
                }
            }
            record::POLYLINE | record::POLYLINE_WITH_NORMAL => {
                if let Some(points) = counted_points(&mut cursor) {
                    out.add_line(Polyline { points: points.iter().map(|p| transform.point(*p)).collect(), paint });
                }
            }
            record::CIRCLE => {
                if let (Some(center), Some(radius), Some(normal)) = (cursor.point(), cursor.f64(), cursor.point()) {
                    let start = scale(perpendicular(normal), radius);
                    out.add_line(Polyline { points: sweep(center, normal, start, std::f64::consts::TAU, &transform), paint });
                }
            }
            record::ARC => {
                if let (Some(center), Some(_radius), Some(normal), Some(start), Some(angle)) = (cursor.point(), cursor.f64(), cursor.point(), cursor.point(), cursor.f64()) {
                    // The start vector already has the length of the radius.
                    out.add_line(Polyline { points: sweep(center, normal, start, angle, &transform), paint });
                }
            }
            record::CIRCLE_3_POINTS | record::ARC_3_POINTS => {
                if let (Some(a), Some(b), Some(c)) = (cursor.point(), cursor.point(), cursor.point()) {
                    if let Some(points) = through_three(a, b, c, kind == record::CIRCLE_3_POINTS, &transform) {
                        out.add_line(Polyline { points, paint });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A shell: `u32` vertex count, the vertices, `u32` length of the face list, the face list (per
/// loop a count and that many vertex numbers; a negative count is a hole in the face before it),
/// then optional data per edge, face and vertex, which is not read.
fn shell(cursor: &mut Cursor<'_>, transform: &Matrix, paint: Paint, out: &mut Geometry) -> Option<()> {
    let vertices = counted_points(cursor)?;
    let entries = cursor.u32()? as usize;
    if entries > cursor.remaining() / 4 {
        return None;
    }
    let mut list = Vec::with_capacity(entries);
    for _ in 0..entries {
        list.push(cursor.i32()?);
    }
    let vertices: Vec<[f64; 3]> = vertices.iter().map(|p| transform.point(*p)).collect();
    let mut loops: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut at = 0usize;
    while at < list.len() {
        let count = list[at];
        let length = count.unsigned_abs() as usize;
        at += 1;
        if length == 0 || at + length > list.len() {
            break;
        }
        let mut points = Vec::with_capacity(length);
        for index in &list[at..at + length] {
            points.push(*vertices.get(usize::try_from(*index).ok()?)?);
        }
        at += length;
        if count > 0 && !loops.is_empty() {
            out.add_face(&loops, paint);
            loops.clear();
        }
        // A hole without a face before it is drawn as a face of its own.
        loops.push(points);
    }
    if !loops.is_empty() {
        out.add_face(&loops, paint);
    }
    Some(())
}

/// A mesh: `u32` rows, `u32` columns, rows x columns vertices; drawn as a grid of quads.
fn mesh(cursor: &mut Cursor<'_>, transform: &Matrix, paint: Paint, out: &mut Geometry) -> Option<()> {
    let rows = cursor.u32()? as usize;
    let columns = cursor.u32()? as usize;
    let count = rows.checked_mul(columns)?;
    if rows < 2 || columns < 2 || count > cursor.remaining() / 24 {
        return None;
    }
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        points.push(transform.point(cursor.point()?));
    }
    for row in 0..rows - 1 {
        for column in 0..columns - 1 {
            let at = |r: usize, c: usize| points[r * columns + c];
            let quad = [vec![at(row, column), at(row + 1, column), at(row + 1, column + 1), at(row, column + 1)]];
            out.add_face(&quad, paint);
        }
    }
    Some(())
}

fn counted_points(cursor: &mut Cursor<'_>) -> Option<Vec<[f64; 3]>> {
    let count = cursor.u32()? as usize;
    if count > cursor.remaining() / 24 {
        return None;
    }
    (0..count).map(|_| cursor.point()).collect()
}

/// Sixteen `f64`, row by row; the translation is the last column.
fn read_matrix(cursor: &mut Cursor<'_>) -> Option<Matrix> {
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row.iter_mut() {
            *value = cursor.f64()?;
        }
    }
    Some(Matrix { rows })
}

fn sweep(center: [f64; 3], normal: [f64; 3], start: [f64; 3], angle: f64, transform: &Matrix) -> Vec<[f64; 3]> {
    let Some(axis) = unit(normal) else { return Vec::new() };
    if !angle.is_finite() || !start.iter().chain(&center).all(|v| v.is_finite()) {
        return Vec::new();
    }
    let side = cross(axis, start);
    let steps = ((angle.abs() / std::f64::consts::TAU * CIRCLE_SEGMENTS as f64).ceil() as usize).clamp(1, CIRCLE_SEGMENTS);
    (0..=steps)
        .map(|step| {
            let (sin, cos) = (angle * step as f64 / steps as f64).sin_cos();
            transform.point([center[0] + start[0] * cos + side[0] * sin, center[1] + start[1] * cos + side[1] * sin, center[2] + start[2] * cos + side[2] * sin])
        })
        .collect()
}

/// The circle through three points, or the arc from the first through the second to the third.
fn through_three(a: [f64; 3], b: [f64; 3], c: [f64; 3], full: bool, transform: &Matrix) -> Option<Vec<[f64; 3]>> {
    let (ab, ac) = (sub(b, a), sub(c, a));
    let normal = cross(ab, ac);
    let n2 = dot(normal, normal);
    if n2 <= 1e-24 || !n2.is_finite() {
        return None;
    }
    // Circumcentre relative to `a`.
    let to_center = scale(add(scale(cross(normal, ab), dot(ac, ac)), scale(cross(ac, normal), dot(ab, ab))), 1.0 / (2.0 * n2));
    let center = add(a, to_center);
    let start = sub(a, center);
    let angle = if full {
        std::f64::consts::TAU
    } else {
        let axis = unit(normal)?;
        let end = sub(c, center);
        let angle = dot(cross(start, end), axis).atan2(dot(start, end));
        if angle <= 0.0 {
            angle + std::f64::consts::TAU
        } else {
            angle
        }
    };
    Some(sweep(center, normal, start, angle, transform))
}

fn perpendicular(normal: [f64; 3]) -> [f64; 3] {
    let Some(n) = unit(normal) else { return [1.0, 0.0, 0.0] };
    // The arbitrary axis rule drawings use for the x axis of a plane.
    let helper = if n[0].abs() < 1.0 / 64.0 && n[1].abs() < 1.0 / 64.0 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
    unit(cross(helper, n)).unwrap_or([1.0, 0.0, 0.0])
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 1e-12).then(|| scale(a, 1.0 / length))
}

/// Little-endian reads that fail instead of running past the end.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.at
    }
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let bytes = self.data.get(self.at..self.at.checked_add(N)?)?;
        self.at += N;
        bytes.try_into().ok()
    }
    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    pub(crate) fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }
    pub(crate) fn f64(&mut self) -> Option<f64> {
        self.take().map(f64::from_le_bytes)
    }
    pub(crate) fn point(&mut self) -> Option<[f64; 3]> {
        Some([self.f64()?, self.f64()?, self.f64()?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{graphics as stream, points};

    fn shell_record(vertices: &[[f64; 3]], faces: &[i32]) -> (u32, Vec<u8>) {
        let mut out = points(vertices);
        out.extend((faces.len() as u32).to_le_bytes());
        out.extend(faces.iter().flat_map(|value| value.to_le_bytes()));
        (record::SHELL, out)
    }

    fn translation(x: f64, y: f64, z: f64) -> (u32, Vec<u8>) {
        let rows = [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z], [0.0, 0.0, 0.0, 1.0]];
        (record::PUSH_TRANSFORM, rows.iter().flatten().flat_map(|value: &f64| value.to_le_bytes()).collect())
    }

    const BOX: [[f64; 3]; 8] = [[0., 0., 0.], [4., 0., 0.], [4., 1., 0.], [0., 1., 0.], [0., 0., 3.], [4., 0., 3.], [4., 1., 3.], [0., 1., 3.]];
    const BOX_FACES: [i32; 30] = [4, 3, 2, 1, 0, 4, 4, 5, 6, 7, 4, 0, 1, 5, 4, 4, 1, 2, 6, 5, 4, 2, 3, 7, 6, 4, 3, 0, 4, 7];

    #[test]
    fn a_shell_of_six_quads_is_a_box_of_twelve_triangles() {
        let geometry = decode(&stream(&[shell_record(&BOX, &BOX_FACES)])).unwrap();
        assert_eq!(geometry.triangle_count(), 12);
        assert!(geometry.is_spatial());
        assert_eq!(geometry.bounds(), Some(([0., 0., 0.], [4., 1., 3.])));
        // Closed and turned outward: the signed volume is the volume of the box.
        let volume: f64 = geometry.corners.chunks_exact(3).map(|t| dot(t[0], cross(t[1], t[2])) / 6.0).sum();
        assert!((volume - 12.0).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn a_negative_loop_is_a_hole_in_the_face_before_it() {
        let vertices = [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.], [4., 4., 0.], [6., 4., 0.], [6., 6., 0.], [4., 6., 0.]];
        let geometry = decode(&stream(&[shell_record(&vertices, &[4, 0, 1, 2, 3, -4, 4, 5, 6, 7])])).unwrap();
        let area: f64 = geometry.corners.chunks_exact(3).map(|t| cross(sub(t[1], t[0]), sub(t[2], t[0]))[2] / 2.0).sum();
        assert!((area - 96.0).abs() < 1e-9, "{area}");
        assert!(!geometry.is_spatial());
    }

    #[test]
    fn traits_and_transforms_apply_to_what_follows_them() {
        let triangle = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let geometry = decode(&stream(&[
            (record::POLYGON, points(&triangle)),
            (record::COLOR, 1i32.to_le_bytes().to_vec()),
            translation(10.0, 20.0, 30.0),
            translation(1.0, 0.0, 0.0),
            (record::POLYGON, points(&triangle)),
            (record::POP_TRANSFORM, Vec::new()),
            (record::TRUE_COLOR, 0xC2_10_20_30u32.to_le_bytes().to_vec()),
            (record::POLYLINE, points(&triangle)),
            (record::POP_TRANSFORM, Vec::new()),
            (record::COLOR, 256i32.to_le_bytes().to_vec()),
            (record::POLYLINE, points(&triangle)),
        ]))
        .unwrap();
        assert_eq!(geometry.paints, [Paint::Object, Paint::Index(1)]);
        assert_eq!(geometry.corners[0], [0., 0., 0.]);
        assert_eq!(geometry.corners[3], [11., 20., 30.]);
        assert_eq!(geometry.lines[0].paint, Paint::Rgb([0x10, 0x20, 0x30]));
        assert_eq!(geometry.lines[0].points[1], [11., 20., 30.]);
        assert_eq!(geometry.lines[1].paint, Paint::ByLayer);
        assert_eq!(geometry.lines[1].points[1], [1., 0., 0.]);
        assert_eq!(geometry.segment_count(), 4);
    }

    #[test]
    fn a_mesh_is_a_grid_of_quads() {
        let mut payload = 2u32.to_le_bytes().to_vec();
        payload.extend(3u32.to_le_bytes());
        for p in [[0., 0., 0.], [1., 0., 0.], [2., 0., 1.], [0., 1., 0.], [1., 1., 0.], [2., 1., 1.]] {
            payload.extend(p.iter().flat_map(|value: &f64| value.to_le_bytes()));
        }
        let geometry = decode(&stream(&[(record::MESH, payload)])).unwrap();
        assert_eq!(geometry.triangle_count(), 4);
    }

    #[test]
    fn circles_and_arcs_are_lines_on_their_radius() {
        let mut circle = Vec::new();
        for value in [5.0, 5.0, 0.0, 2.0, 0.0, 0.0, 1.0] {
            circle.extend(f64::to_le_bytes(value));
        }
        let mut arc = Vec::new();
        for value in [0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 1.0, 3.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2] {
            arc.extend(f64::to_le_bytes(value));
        }
        arc.extend(0u32.to_le_bytes());
        let geometry = decode(&stream(&[(record::CIRCLE, circle), (record::ARC, arc)])).unwrap();
        assert_eq!(geometry.lines[0].points.len(), CIRCLE_SEGMENTS + 1);
        assert!(geometry.lines[0].points.iter().all(|p| (((p[0] - 5.0).powi(2) + (p[1] - 5.0).powi(2)).sqrt() - 2.0).abs() < 1e-9));
        let last = *geometry.lines[1].points.last().unwrap();
        assert!((last[0]).abs() < 1e-9 && (last[1] - 3.0).abs() < 1e-9, "{last:?}");
    }

    #[test]
    fn damaged_streams_are_refused_and_damaged_primitives_skipped() {
        assert_eq!(decode(&[]), Err(GraphicsError::Truncated));
        assert_eq!(decode(&[200, 0, 0, 0, 1, 0, 0, 0]), Err(GraphicsError::Truncated));
        let mut bad = stream(&[(record::POLYGON, Vec::new())]);
        bad[8..12].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(decode(&bad), Err(GraphicsError::BadRecord));
        // A vertex count far larger than the payload, an index outside the vertices, a point that
        // is not a number: each primitive is left out, the next one is still read.
        let mut huge = points(&[[0., 0., 0.]]);
        huge[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        let geometry = decode(&stream(&[
            (record::SHELL, huge),
            shell_record(&BOX, &[3, 0, 1, 99]),
            (record::POLYGON, points(&[[0., 0., 0.], [f64::NAN, 0., 0.], [0., 1., 0.]])),
            (record::POLYGON, points(&[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]])),
        ]))
        .unwrap();
        assert_eq!(geometry.triangle_count(), 1);
    }
}
