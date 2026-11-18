//! A box from its extents.
//!
//! When an object carries only its extents (the lowest and highest corner of what it shows, along
//! the axes of the drawing) and its record gives the axes it stands along, the box along the
//! object's own axes that has those extents is the object itself, for every object that is a box:
//! a straight wall, a door, a rectangular slab, a straight member.
//!
//! A box of sizes `s` along axes `a` reaches `sum |a[j][i]| * s[j]` along drawing axis `i`: three
//! equations for three sizes. They do not settle the sizes when the object is turned half a right
//! angle (two sizes then only show as their sum); a size known from elsewhere settles them.

/// Below this determinant the three equations are too close to dependent to trust.
const MINIMUM_DETERMINANT: f64 = 0.2;

/// The sizes, along `axes`, of the box that reaches `extent` along the axes of the drawing.
///
/// `known` are sizes read from elsewhere; they are used only where the extents do not settle the
/// sizes. `None` when they stay unsettled, or when no box along these axes has these extents.
pub fn box_sizes(axes: [[f64; 3]; 3], extent: [f64; 3], known: [Option<f64>; 3]) -> Option<[f64; 3]> {
    if axes.iter().flatten().chain(&extent).any(|value| !value.is_finite()) || extent.iter().any(|value| *value < 0.0) {
        return None;
    }
    // reach[i][j]: how far a unit along own axis j reaches along drawing axis i.
    let mut reach = [[0.0; 3]; 3];
    for (i, row) in reach.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = axes[j][i].abs();
        }
    }
    let sizes = if determinant(&reach).abs() >= MINIMUM_DETERMINANT {
        solve(&reach, extent)?
    } else {
        // Least squares over the three equations and one more per known size.
        let mut normal = [[0.0; 3]; 3];
        let mut right = [0.0; 3];
        for (row, value) in reach.iter().zip(extent) {
            for j in 0..3 {
                right[j] += row[j] * value;
                for k in 0..3 {
                    normal[j][k] += row[j] * row[k];
                }
            }
        }
        for (j, size) in known.iter().enumerate() {
            if let Some(size) = size.filter(|size| size.is_finite() && *size >= 0.0) {
                normal[j][j] += 1.0;
                right[j] += size;
            }
        }
        if determinant(&normal).abs() < MINIMUM_DETERMINANT * MINIMUM_DETERMINANT {
            return None;
        }
        solve(&normal, right)?
    };
    let scale = extent.iter().fold(0.0f64, |a, b| a.max(*b));
    let tolerance = 0.02 * scale + 1e-9;
    if sizes.iter().any(|size| !size.is_finite() || *size < -tolerance) {
        return None;
    }
    let sizes = sizes.map(|size| size.max(0.0));
    for (row, value) in reach.iter().zip(extent) {
        if ((0..3).map(|j| row[j] * sizes[j]).sum::<f64>() - value).abs() > tolerance {
            return None;
        }
    }
    Some(sizes)
}

fn determinant(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Cramer's rule.
fn solve(m: &[[f64; 3]; 3], right: [f64; 3]) -> Option<[f64; 3]> {
    let d = determinant(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let mut out = [0.0; 3];
    for (column, value) in out.iter_mut().enumerate() {
        let mut replaced = *m;
        for row in 0..3 {
            replaced[row][column] = right[row];
        }
        *value = determinant(&replaced) / d;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turned(angle: f64) -> [[f64; 3]; 3] {
        let (sin, cos) = angle.to_radians().sin_cos();
        [[cos, sin, 0.0], [-sin, cos, 0.0], [0.0, 0.0, 1.0]]
    }

    /// The extents of a box of `sizes` along `axes`.
    fn extent(axes: [[f64; 3]; 3], sizes: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| (0..3).map(|j| axes[j][i].abs() * sizes[j]).sum())
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    #[test]
    fn a_turned_box_comes_back_from_its_extents() {
        let sizes = [300.0, 6.5, 120.0];
        for angle in [0.0, 17.0, 90.0, 123.0, 200.0, -30.0] {
            let axes = turned(angle);
            assert!(close(box_sizes(axes, extent(axes, sizes), [None; 3]).unwrap(), sizes), "{angle}");
        }
    }

    #[test]
    fn half_a_right_angle_needs_a_known_size() {
        let (axes, sizes) = (turned(45.0), [300.0, 6.5, 120.0]);
        assert_eq!(box_sizes(axes, extent(axes, sizes), [None; 3]), None);
        assert!(close(box_sizes(axes, extent(axes, sizes), [None, Some(6.5), None]).unwrap(), sizes));
        // Near it too, where the extents alone would magnify every rounding.
        let axes = turned(44.0);
        assert!(box_sizes(axes, extent(axes, sizes), [None, Some(6.5), None]).is_some_and(|found| (found[0] - 300.0).abs() < 1.0));
    }

    #[test]
    fn a_pitched_plate_without_thickness_is_settled_by_a_zero() {
        let pitch = std::f64::consts::FRAC_1_SQRT_2;
        let axes = [[1.0, 0.0, 0.0], [0.0, pitch, pitch], [0.0, -pitch, pitch]];
        assert!(close(box_sizes(axes, extent(axes, [700.0, 160.0, 0.0]), [None, None, Some(0.0)]).unwrap(), [700.0, 160.0, 0.0]));
    }

    #[test]
    fn extents_no_box_along_the_axes_has_are_refused() {
        // Turned 30 degrees, a box is at least half as deep as it is wide.
        assert_eq!(box_sizes(turned(30.0), [100.0, 1.0, 10.0], [None; 3]), None);
        assert_eq!(box_sizes(turned(0.0), [f64::NAN, 1.0, 1.0], [None; 3]), None);
    }
}
