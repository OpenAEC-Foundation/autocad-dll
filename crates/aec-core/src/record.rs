//! The object's own record in the drawing: where the object stands and, for some classes, its
//! sizes.
//!
//! A record is a stream of bits in the drawing's own number encodings (a short, a long and a
//! double each have a two-bit prefix that says how many bytes follow). The fields of an
//! AutoCAD Architecture object start, somewhere after the fields every drawing object has, with
//! a tag: a version marker (a small number per release of the application), two set bits and the
//! number of the class. Then follow, between further markers, the placement of the object (an
//! origin and three axes, or only its sizes) and the fields of the class.
//!
//! The tag is found by looking for it: a candidate counts only when a placement with three
//! perpendicular unit axes follows, which bits that are not a placement do not give by accident.
//! Texts and references to other objects are kept in other parts of the record (drawings from
//! the 2007 format on) and take no room between the numbers read here.

/// An origin and three perpendicular unit axes, in the coordinates of the drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

impl Frame {
    pub const WORLD: Frame = Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };

    pub fn axes(&self) -> [[f64; 3]; 3] {
        [self.x, self.y, self.z]
    }

    /// The point at `local` in this frame.
    pub fn point(&self, local: [f64; 3]) -> [f64; 3] {
        let mut out = self.origin;
        for (axis, distance) in self.axes().iter().zip(local) {
            for k in 0..3 {
                out[k] += axis[k] * distance;
            }
        }
        out
    }
}

/// Where an object stands, as far as its record says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// An origin and axes in the drawing.
    Frame(Frame),
    /// No frame, only the sizes of the object along its own axes: where the object stands and
    /// how it is turned is not in this part of the record.
    Sizes([f64; 3]),
}

/// The start of an object's own fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    /// Version marker of the application that wrote the object.
    pub marker: u8,
    /// Number of the object's class.
    pub class_number: i16,
    pub placement: Placement,
    /// Bit at which the fields of the class start, after the placement.
    pub fields_at: usize,
}

impl Header {
    pub fn frame(&self) -> Option<Frame> {
        match self.placement {
            Placement::Frame(frame) => Some(frame),
            Placement::Sizes(_) => None,
        }
    }
}

/// Finds the tag and the placement in a record.
pub fn header(record: &[u8]) -> Option<Header> {
    let bits = record.len().checked_mul(8)?;
    (0..bits.saturating_sub(MINIMUM_HEADER_BITS)).find_map(|at| header_at(record, at))
}

/// The tag, three markers around fixed values and twelve doubles of two bits at least.
const MINIMUM_HEADER_BITS: usize = 10 + 2 + 18 + 10 + 10 + 2 + 8 + 1 + 10 + 24;

fn header_at(record: &[u8], at: usize) -> Option<Header> {
    let mut reader = Bits::at(record, at);
    // A marker is a short in its one-byte form.
    if reader.bits(2)? != 0b01 {
        return None;
    }
    let marker = reader.bits(8)? as u8;
    if marker == 0 || !reader.bit()? || !reader.bit()? {
        return None;
    }
    let class_number = reader.short()?;
    // The markers that follow may be of other releases than the first: each part of an object
    // keeps the marker of the release that last changed its layout.
    if class_number <= 0 || reader.marker().is_none() || reader.marker().is_none() || reader.short()? != 0 {
        return None;
    }
    // A one and a marker announce a frame; a zero the extents of the object along its own axes.
    let framed = match reader.bits(8)? {
        1 => true,
        0 => false,
        _ => return None,
    };
    if reader.bit()? || (framed && reader.marker().is_none()) {
        return None;
    }
    let mut values = [0.0f64; 12];
    for value in &mut values {
        *value = reader.double()?;
        if !value.is_finite() || value.abs() > 1e15 {
            return None;
        }
    }
    let vectors = [[values[3], values[4], values[5]], [values[6], values[7], values[8]], [values[9], values[10], values[11]]];
    let placement = if framed {
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        for (index, axis) in vectors.iter().enumerate() {
            if (dot(*axis, *axis) - 1.0).abs() > 1e-6 || dot(*axis, vectors[(index + 1) % 3]).abs() > 1e-6 {
                return None;
            }
        }
        Placement::Frame(Frame { origin: [values[0], values[1], values[2]], x: vectors[0], y: vectors[1], z: vectors[2] })
    } else {
        // A corner and the three edges of a box along the object's axes: nine exact zeros at most
        // three sizes, which other bits do not give by accident.
        for (index, edge) in vectors.iter().enumerate() {
            if edge[index] < 0.0 || edge[(index + 1) % 3] != 0.0 || edge[(index + 2) % 3] != 0.0 {
                return None;
            }
        }
        if vectors.iter().enumerate().filter(|(index, edge)| edge[*index] > 0.0).count() < 2 {
            return None;
        }
        Placement::Sizes([vectors[0][0], vectors[1][1], vectors[2][2]])
    };
    Some(Header { marker, class_number, placement, fields_at: reader.at })
}

/// The first doubles of the class's own fields: after the placement a marker, then `N` doubles.
/// A wall starts with its height, a number and its width; a door, a window and an opening with
/// their width and height.
pub fn leading_numbers<const N: usize>(record: &[u8], header: &Header) -> Option<[f64; N]> {
    let mut reader = Bits::at(record, header.fields_at);
    reader.marker()?;
    let mut out = [0.0; N];
    for value in &mut out {
        *value = reader.double().filter(|value| value.is_finite())?;
    }
    Some(out)
}

/// Height, width and length of a wall whose fields have the plain layout: a straight wall
/// without changes to its roof and floor lines, its end caps or its body. `None` for any other.
///
/// After the three leading numbers: a number, two clear bits, five empty groups (marker,
/// marker, zero), the short 2, one more empty group, a marker and the length.
pub fn straight_wall(record: &[u8], header: &Header) -> Option<[f64; 3]> {
    let [height, _, width] = leading_numbers::<3>(record, header)?;
    // The leading marker and three doubles are read again to stand behind them.
    let mut reader = Bits::at(record, header.fields_at);
    reader.marker()?;
    for _ in 0..3 {
        reader.double()?;
    }
    reader.double()?;
    if reader.bit()? || reader.bit()? {
        return None;
    }
    let empty_group = |reader: &mut Bits<'_>| (reader.marker().is_some() && reader.marker().is_some() && reader.long()? == 0).then_some(());
    for _ in 0..5 {
        empty_group(&mut reader)?;
    }
    if reader.short()? != 2 {
        return None;
    }
    empty_group(&mut reader)?;
    reader.marker()?;
    let length = reader.double()?;
    let positive = |value: f64| value.is_finite() && value > 0.0;
    (positive(height) && positive(width) && positive(length)).then_some([height, width, length])
}

/// Reads a record bit by bit, the first bit of a byte being its highest.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bits<'a> {
    fn at(data: &'a [u8], at: usize) -> Self {
        Self { data, at }
    }

    fn bit(&mut self) -> Option<bool> {
        let byte = *self.data.get(self.at / 8)?;
        let bit = byte >> (7 - self.at % 8) & 1 == 1;
        self.at += 1;
        Some(bit)
    }

    /// Up to 32 bits, the first read the highest.
    fn bits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            value = value << 1 | u32::from(self.bit()?);
        }
        Some(value)
    }

    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let mut out = [0u8; N];
        for byte in &mut out {
            *byte = self.bits(8)? as u8;
        }
        Some(out)
    }

    /// A version marker: a short in its one-byte form, not zero.
    fn marker(&mut self) -> Option<u8> {
        if self.bits(2)? != 0b01 {
            return None;
        }
        Some(self.bits(8)? as u8).filter(|marker| *marker != 0)
    }

    /// A short: prefix 00 two bytes, 01 one byte, 10 the value 0, 11 the value 256.
    fn short(&mut self) -> Option<i16> {
        Some(match self.bits(2)? {
            0 => i16::from_le_bytes(self.bytes()?),
            1 => i16::from(self.bits(8)? as u8),
            2 => 0,
            _ => 256,
        })
    }

    /// A long: prefix 00 four bytes, 01 one byte, 10 the value 0.
    fn long(&mut self) -> Option<i32> {
        match self.bits(2)? {
            0 => Some(i32::from_le_bytes(self.bytes()?)),
            1 => Some(self.bits(8)? as i32),
            2 => Some(0),
            _ => None,
        }
    }

    /// A double: prefix 00 eight bytes, 01 the value 1, 10 the value 0.
    fn double(&mut self) -> Option<f64> {
        match self.bits(2)? {
            0 => Some(f64::from_le_bytes(self.bytes()?)),
            1 => Some(1.0),
            2 => Some(0.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{wall_record, Writer};

    fn turned() -> Frame {
        let (sin, cos) = 0.3f64.sin_cos();
        Frame { origin: [1200.5, -80.25, 3.0], x: [cos, sin, 0.0], y: [-sin, cos, 0.0], z: [0.0, 0.0, 1.0] }
    }

    #[test]
    fn the_header_is_found_at_any_bit_and_for_any_marker() {
        for marker in [50, 80] {
            let record = wall_record(marker, &turned(), 120.0, 6.0, 72.0);
            let header = header(&record).unwrap();
            assert_eq!(header.marker as i16, marker);
            assert_eq!(header.class_number, 569);
            assert_eq!(header.frame(), Some(turned()));
            assert_eq!(leading_numbers::<3>(&record, &header), Some([120.0, -1.0, 6.0]));
            assert_eq!(straight_wall(&record, &header), Some([120.0, 6.0, 72.0]));
        }
    }

    #[test]
    fn axes_that_are_not_a_frame_are_no_header() {
        let mut skewed = turned();
        skewed.y = [0.0, 0.9, 0.0];
        let mut writer = Writer::default();
        writer.header(80, 569, &skewed);
        assert_eq!(header(&writer.finish()), None);
        assert_eq!(header(&[]), None);
        assert_eq!(header(&[0x55; 64]), None);
    }

    #[test]
    fn a_wall_with_another_layout_gives_its_leading_numbers_only() {
        let mut writer = Writer::default();
        writer.header(80, 569, &Frame::WORLD).short(80).double(96.0).double(-1.0).double(4.0).double(12.0).bit(true).bit(false).double(55.0);
        let record = writer.finish();
        let header = header(&record).unwrap();
        assert_eq!(leading_numbers::<3>(&record, &header), Some([96.0, -1.0, 4.0]));
        assert_eq!(straight_wall(&record, &header), None);
    }

    #[test]
    fn a_record_without_a_frame_gives_the_sizes_of_the_object() {
        let mut writer = Writer::default();
        writer.short(80).bit(true).bit(true).short(566).short(80).short(80).short(0).raw(0, 8).bit(false);
        for value in [-45.0, -57.5, 0.0, 90.0, 0.0, 0.0, 0.0, 57.5, 0.0, 0.0, 0.0, 41.0] {
            writer.double(value);
        }
        let header = header(&writer.finish()).unwrap();
        assert_eq!(header.placement, Placement::Sizes([90.0, 57.5, 41.0]));
        assert_eq!(header.frame(), None);
    }

    #[test]
    fn a_frame_places_local_points() {
        let frame = Frame { origin: [10.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [-1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
        assert_eq!(frame.point([2.0, 3.0, 4.0]), [7.0, 2.0, 4.0]);
    }
}
