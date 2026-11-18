//! What decoding gives: triangles and lines in the drawing's own units and coordinates.

/// The colour of a triangle or a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paint {
    /// The colour of the object itself: nothing in its graphics says otherwise.
    #[default]
    Object,
    /// The colour of the layer the object is on.
    ByLayer,
    /// The colour of the block reference the object is drawn through.
    ByBlock,
    /// A number of the drawing colour table, 1..=255.
    Index(u8),
    /// Red, green, blue.
    Rgb([u8; 3]),
}

/// A run of connected line segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub points: Vec<[f64; 3]>,
    pub paint: Paint,
}

/// Triangles and lines of one object.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    /// Corners of the triangles, three per triangle.
    pub corners: Vec<[f64; 3]>,
    /// One colour per triangle.
    pub paints: Vec<Paint>,
    pub lines: Vec<Polyline>,
}

impl Geometry {
    pub fn triangle_count(&self) -> usize {
        self.paints.len()
    }

    /// Number of line segments over all polylines.
    pub fn segment_count(&self) -> usize {
        self.lines.iter().map(|line| line.points.len().saturating_sub(1)).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.paints.is_empty() && self.lines.is_empty()
    }

    /// Lowest and highest corner over triangles and lines; `None` when there is nothing.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut points = self.corners.iter().chain(self.lines.iter().flat_map(|line| line.points.iter()));
        let first = *points.next()?;
        Some(points.fold((first, first), |(mut low, mut high), p| {
            for axis in 0..3 {
                low[axis] = low[axis].min(p[axis]);
                high[axis] = high[axis].max(p[axis]);
            }
            (low, high)
        }))
    }

    /// Whether the triangles rise out of one horizontal plane: a body, not a plan drawing.
    pub fn is_spatial(&self) -> bool {
        let mut heights = self.corners.iter().map(|p| p[2]);
        let Some(first) = heights.next() else { return false };
        let (low, high) = heights.fold((first, first), |(low, high), z| (low.min(z), high.max(z)));
        high - low > 1e-9 * (1.0 + low.abs().max(high.abs()))
    }

    /// Adds a face: an outer loop and the holes in it. A face with a corner that is not a finite
    /// number is dropped.
    pub(crate) fn add_face(&mut self, loops: &[Vec<[f64; 3]>], paint: Paint) {
        if loops.iter().flatten().flatten().any(|value| !value.is_finite()) {
            return;
        }
        for triangle in crate::triangulate::triangulate(loops) {
            self.corners.extend(triangle);
            self.paints.push(paint);
        }
    }

    /// Adds a polyline of at least two finite points.
    pub(crate) fn add_line(&mut self, line: Polyline) {
        if line.points.len() >= 2 && line.points.iter().flatten().all(|value| value.is_finite()) {
            self.lines.push(line);
        }
    }

    /// Adds the box with its middle at `center` and `sizes` along `axes`, turned outward. A box
    /// without thickness becomes a plate seen from both sides.
    pub fn add_box(&mut self, center: [f64; 3], axes: [[f64; 3]; 3], sizes: [f64; 3], paint: Paint) {
        let corner = |signs: [f64; 3]| {
            let mut p = center;
            for j in 0..3 {
                for k in 0..3 {
                    p[k] += axes[j][k] * sizes[j] * 0.5 * signs[j];
                }
            }
            p
        };
        // Per pair of faces: the axis they are across, and the two axes that span them, in the
        // order that turns the far face outward.
        for (across, u, v) in [(0, 1, 2), (1, 2, 0), (2, 0, 1)] {
            for side in [1.0, -1.0] {
                let at = |a: f64, b: f64| {
                    let mut signs = [0.0; 3];
                    signs[across] = side;
                    signs[u] = a;
                    signs[v] = b;
                    corner(signs)
                };
                let mut face = vec![at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)];
                if side < 0.0 {
                    face.reverse();
                }
                self.add_face(&[face], paint);
            }
        }
    }

    /// Appends another geometry.
    pub fn extend(&mut self, other: Geometry) {
        self.corners.extend(other.corners);
        self.paints.extend(other.paints);
        self.lines.extend(other.lines);
    }
}
