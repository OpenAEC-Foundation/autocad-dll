//! Affine transforms as the graphics stream stores them: four rows of four, translation in the
//! last column.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub rows: [[f64; 4]; 4],
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix { rows: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };

    /// `self` applied after `other`.
    pub fn mul(&self, other: &Matrix) -> Matrix {
        let mut rows = [[0.0; 4]; 4];
        for (r, row) in rows.iter_mut().enumerate() {
            for (c, value) in row.iter_mut().enumerate() {
                *value = (0..4).map(|k| self.rows[r][k] * other.rows[k][c]).sum();
            }
        }
        Matrix { rows }
    }

    pub fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.rows;
        [m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3], m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3], m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3]]
    }
}
