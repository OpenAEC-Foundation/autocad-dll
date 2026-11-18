//! Faces to triangles.

/// Triangles of a planar face: `loops[0]` is the outline, the others are holes in it. The
/// triangles keep the turning direction of the outline. A face without area gives nothing.
pub fn triangulate(loops: &[Vec<[f64; 3]>]) -> Vec<[[f64; 3]; 3]> {
    let mut rings = loops.iter().map(|points| clean(points));
    let Some(outline) = rings.next().filter(|outline| outline.len() >= 3) else { return Vec::new() };
    let holes: Vec<Vec<[f64; 3]>> = rings.filter(|ring| ring.len() >= 3).collect();
    let origin = outline[0];
    let normal = newell(&outline, origin);
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length.is_nan() || length <= 1e-18 || length.is_infinite() {
        return Vec::new();
    }
    if holes.is_empty() && outline.len() == 3 {
        return vec![[outline[0], outline[1], outline[2]]];
    }
    // Drop the axis the face looks along most, and triangulate in the other two.
    let axis = if normal[0].abs() >= normal[1].abs() && normal[0].abs() >= normal[2].abs() {
        0
    } else if normal[1].abs() >= normal[2].abs() {
        1
    } else {
        2
    };
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let mut flat = Vec::new();
    let mut points = Vec::new();
    let mut starts = Vec::new();
    for (index, ring) in std::iter::once(&outline).chain(&holes).enumerate() {
        if index > 0 {
            starts.push(points.len());
        }
        for p in ring {
            flat.push(p[u] - origin[u]);
            flat.push(p[v] - origin[v]);
            points.push(*p);
        }
    }
    let Ok(indices) = earcutr::earcut(&flat, &starts, 2) else { return Vec::new() };
    let outward = normal[axis] > 0.0;
    indices
        .chunks_exact(3)
        .map(|t| {
            let mut triangle = [points[t[0]], points[t[1]], points[t[2]]];
            let turn = (flat[2 * t[1]] - flat[2 * t[0]]) * (flat[2 * t[2] + 1] - flat[2 * t[0] + 1]) - (flat[2 * t[1] + 1] - flat[2 * t[0] + 1]) * (flat[2 * t[2]] - flat[2 * t[0]]);
            if (turn > 0.0) != outward {
                triangle.swap(1, 2);
            }
            triangle
        })
        .collect()
}

/// Without repeated neighbours and without a closing point equal to the first.
fn clean(points: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() != Some(p) {
            out.push(*p);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Normal of a loop, its length twice the area.
fn newell(points: &[[f64; 3]], origin: [f64; 3]) -> [f64; 3] {
    let mut normal = [0.0; 3];
    for (index, a) in points.iter().enumerate() {
        let b = points[(index + 1) % points.len()];
        let (a, b) = ([a[0] - origin[0], a[1] - origin[1], a[2] - origin[2]], [b[0] - origin[0], b[1] - origin[1], b[2] - origin[2]]);
        normal[0] += a[1] * b[2] - a[2] * b[1];
        normal[1] += a[2] * b[0] - a[0] * b[2];
        normal[2] += a[0] * b[1] - a[1] * b[0];
    }
    normal
}
