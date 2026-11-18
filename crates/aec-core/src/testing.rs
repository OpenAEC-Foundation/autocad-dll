//! Builders of synthetic streams and records, for the tests of this workspace: sample drawings
//! are in no repository, so tests make their own bytes.

use crate::record::Frame;

/// A graphics stream of the given records (type and payload).
pub fn graphics(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    out[4..8].copy_from_slice(&(records.len() as u32).to_le_bytes());
    for (kind, payload) in records {
        out.extend(((payload.len() + 8) as u32).to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(payload);
    }
    let total = out.len() as u32;
    out[0..4].copy_from_slice(&total.to_le_bytes());
    out
}

/// The payload of a polyline or polygon record.
pub fn points(points: &[[f64; 3]]) -> Vec<u8> {
    let mut out = (points.len() as u32).to_le_bytes().to_vec();
    out.extend(points.iter().flatten().flat_map(|value| value.to_le_bytes()));
    out
}

/// The graphics an object carries when its application left only its extents: the edges of the
/// box from `low` to `high` as six polylines, or one rectangle when the box is flat.
pub fn extents(low: [f64; 3], high: [f64; 3]) -> Vec<u8> {
    let corner = |x: bool, y: bool, z: bool| [if x { high[0] } else { low[0] }, if y { high[1] } else { low[1] }, if z { high[2] } else { low[2] }];
    let ring = |z: bool| vec![corner(false, false, z), corner(true, false, z), corner(true, true, z), corner(false, true, z), corner(false, false, z)];
    let mut records = vec![(crate::graphics::record::POLYLINE, points(&ring(false)))];
    if high[2] > low[2] {
        records.push((crate::graphics::record::POLYLINE, points(&ring(true))));
        for (x, y) in [(false, false), (true, false), (true, true), (false, true)] {
            records.push((crate::graphics::record::POLYLINE, points(&[corner(x, y, false), corner(x, y, true)])));
        }
    }
    graphics(&records)
}

/// Writes the bit encodings of a record.
#[derive(Default)]
pub struct Writer {
    bits: Vec<bool>,
}

impl Writer {
    pub fn bit(&mut self, bit: bool) -> &mut Self {
        self.bits.push(bit);
        self
    }
    pub fn raw(&mut self, value: u64, count: usize) -> &mut Self {
        for index in (0..count).rev() {
            self.bits.push(value >> index & 1 == 1);
        }
        self
    }
    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        for byte in bytes {
            self.raw(u64::from(*byte), 8);
        }
        self
    }
    pub fn short(&mut self, value: i16) -> &mut Self {
        match value {
            0 => self.raw(0b10, 2),
            256 => self.raw(0b11, 2),
            1..=255 => self.raw(0b01, 2).raw(value as u64, 8),
            _ => self.raw(0b00, 2).bytes(&value.to_le_bytes()),
        }
    }
    pub fn long(&mut self, value: i32) -> &mut Self {
        match value {
            0 => self.raw(0b10, 2),
            1..=255 => self.raw(0b01, 2).raw(value as u64, 8),
            _ => self.raw(0b00, 2).bytes(&value.to_le_bytes()),
        }
    }
    pub fn double(&mut self, value: f64) -> &mut Self {
        if value == 1.0 {
            self.raw(0b01, 2)
        } else if value == 0.0 {
            self.raw(0b10, 2)
        } else {
            self.raw(0b00, 2).bytes(&value.to_le_bytes())
        }
    }
    /// The tag and the placement of an object.
    pub fn header(&mut self, marker: i16, class_number: i16, frame: &Frame) -> &mut Self {
        self.short(marker).bit(true).bit(true).short(class_number).short(marker).short(marker).short(0).raw(1, 8).bit(false).short(marker);
        for value in [frame.origin, frame.x, frame.y, frame.z].iter().flatten() {
            self.double(*value);
        }
        self
    }
    pub fn finish(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.bits.len().div_ceil(8) + 2];
        for (index, bit) in self.bits.iter().enumerate() {
            out[index / 8] |= u8::from(*bit) << (7 - index % 8);
        }
        out
    }
}

/// A record of a wall with the plain layout, behind some bits of other fields.
pub fn wall_record(marker: i16, frame: &Frame, height: f64, width: f64, length: f64) -> Vec<u8> {
    let mut writer = Writer::default();
    writer.raw(0b1011_0010_1, 9).short(513).double(3.5);
    writer.header(marker, 569, frame).short(marker).double(height).double(-1.0).double(width).double(0.0).bit(false).bit(false);
    for _ in 0..5 {
        writer.short(marker).short(marker).long(0);
    }
    writer.short(2).short(marker).short(marker).long(0).short(marker).double(length);
    writer.finish()
}
