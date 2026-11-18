//! Reads the objects of AutoCAD Architecture drawings.
//!
//! The caller opens the drawing with its own DWG codec and hands over, per custom object, the
//! class name, the graphics the object carries and the object's record; this crate says what the
//! object is and gives its geometry:
//!
//! ```
//! let source = aec_read::Source { class_name: "AEC_WALL", graphics: None, record: None };
//! let object = aec_read::read_object(&source).unwrap();
//! assert_eq!(object.class.label(), "wall");
//! assert_eq!(object.shape, aec_read::Shape::Nothing);
//! ```
//!
//! What a drawing keeps of an object for other programs is, in the drawings seen so far, only
//! its extents: the edges of a box along the axes of the drawing. The record says along which
//! axes the object itself stands. From the two follows the box the object is, for the classes
//! that are boxes when they are plain: walls, doors, windows, slabs, roof slabs, curtain walls,
//! members. [`Shape`] tells per object what its geometry is.

#![forbid(unsafe_code)]

use aec_core::record::{self, Placement};
pub use aec_core::{Frame, Geometry, GraphicsError, Paint, Polyline};

/// What an object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Wall,
    Door,
    Window,
    /// A door and window assembly: a framed grid of infills placed in a wall.
    WindowAssembly,
    Opening,
    Slab,
    RoofSlab,
    Roof,
    CurtainWall,
    CurtainWallUnit,
    Stair,
    Railing,
    /// A structural member: beam, column or brace.
    Member,
    MassElement,
    MassGroup,
    Space,
    /// A block that shows a different drawing per view: furniture, fixtures, equipment, symbols.
    MultiViewBlock,
    /// Any other object of the application: grids, annotation, section and elevation marks,
    /// schedules, cameras.
    Other,
}

/// DXF name and class name of the classes that have a [`Class`] of their own.
const CLASSES: &[(&str, &str, Class)] = &[
    ("AEC_WALL", "AecDbWall", Class::Wall),
    ("AEC_DOOR", "AecDbDoor", Class::Door),
    ("AEC_WINDOW", "AecDbWindow", Class::Window),
    ("AEC_WINDOW_ASSEMBLY", "AecDbWindowAssembly", Class::WindowAssembly),
    ("AEC_OPENING", "AecDbOpening", Class::Opening),
    ("AEC_SLAB", "AecDbSlab", Class::Slab),
    ("AEC_ROOFSLAB", "AecDbRoofSlab", Class::RoofSlab),
    ("AEC_ROOF", "AecDbRoof", Class::Roof),
    ("AEC_CURTAIN_WALL_LAYOUT", "AecDbCurtainWallLayout", Class::CurtainWall),
    ("AEC_CURTAIN_WALL_UNIT", "AecDbCurtainWallUnit", Class::CurtainWallUnit),
    ("AEC_STAIR", "AecDbStair", Class::Stair),
    ("AEC_RAILING", "AecDbRailing", Class::Railing),
    ("AECS_MEMBER", "AecsDbMember", Class::Member),
    ("AEC_MASS_ELEM", "AecDbMassElem", Class::MassElement),
    ("AEC_MASS_GROUP", "AecDbMassGroup", Class::MassGroup),
    ("AEC_SPACE", "AecDbSpace", Class::Space),
    ("AEC_MVBLOCK_REF", "AecDbMvBlockRef", Class::MultiViewBlock),
];

impl Class {
    /// The class behind a name, by DXF name (`AEC_WALL`) or class name (`AecDbWall`), in any
    /// case. `None` for a name that is not of an AutoCAD Architecture object.
    pub fn from_name(name: &str) -> Option<Class> {
        if let Some((_, _, class)) = CLASSES.iter().find(|(dxf, class, _)| name.eq_ignore_ascii_case(dxf) || name.eq_ignore_ascii_case(class)) {
            return Some(*class);
        }
        let upper = name.to_ascii_uppercase();
        ["AEC_", "AECS_", "AECDB", "AECSDB"].iter().any(|prefix| upper.starts_with(prefix)).then_some(Class::Other)
    }

    /// The class in plain words.
    pub fn label(self) -> &'static str {
        match self {
            Class::Wall => "wall",
            Class::Door => "door",
            Class::Window => "window",
            Class::WindowAssembly => "door and window assembly",
            Class::Opening => "opening",
            Class::Slab => "slab",
            Class::RoofSlab => "roof slab",
            Class::Roof => "roof",
            Class::CurtainWall => "curtain wall",
            Class::CurtainWallUnit => "curtain wall unit",
            Class::Stair => "stair",
            Class::Railing => "railing",
            Class::Member => "member",
            Class::MassElement => "mass element",
            Class::MassGroup => "mass group",
            Class::Space => "space",
            Class::MultiViewBlock => "multi-view block",
            Class::Other => "other",
        }
    }

    /// Whether objects of this class are parts of the building, as opposed to annotation, grids,
    /// symbols and spaces.
    pub fn is_building_part(self) -> bool {
        !matches!(self, Class::Other | Class::MultiViewBlock | Class::Space | Class::MassGroup | Class::Opening)
    }

    /// Whether a plain object of this class is a box along its own axes.
    pub fn is_box(self) -> bool {
        matches!(
            self,
            Class::Wall | Class::Door | Class::Window | Class::WindowAssembly | Class::Slab | Class::RoofSlab | Class::CurtainWall | Class::CurtainWallUnit | Class::Member | Class::MassElement
        )
    }

    /// The own axis a plate of this class is thin along: 2 for what lies (slabs, with the third
    /// axis square to their face), 1 for what stands in a wall line. `None` for other classes.
    fn thin_axis(self) -> Option<usize> {
        match self {
            Class::Slab | Class::RoofSlab => Some(2),
            Class::CurtainWall | Class::CurtainWallUnit | Class::WindowAssembly => Some(1),
            _ => None,
        }
    }
}

/// What the geometry of an object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    /// The triangles and lines the object's own graphics hold.
    Graphics,
    /// A box along the object's own axes, with the extents the object carries. The object itself
    /// when it is plain (a straight wall, a rectangular slab); the block around it otherwise.
    Box,
    /// As [`Shape::Box`], without thickness: the object is turned half a right angle about an
    /// axis, where its extents give its thickness and its other size only as their sum.
    Plate,
    /// Lines only: the edges of the object's extents, along the axes of the drawing.
    Extents,
    /// Nothing: the object carries no graphics, or none that are read.
    Nothing,
}

impl Shape {
    /// The shape in plain words.
    pub fn label(self) -> &'static str {
        match self {
            Shape::Graphics => "own graphics",
            Shape::Box => "box from extents",
            Shape::Plate => "plate from extents",
            Shape::Extents => "extents as lines",
            Shape::Nothing => "nothing",
        }
    }
}

/// What the caller's codec hands over of one custom object.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    /// DXF name (`AEC_WALL`) or class name (`AecDbWall`).
    pub class_name: &'a str,
    /// The graphics the object carries for other programs, if any.
    pub graphics: Option<&'a [u8]>,
    /// The object's record as the drawing stores it, from its type code on, if the codec keeps
    /// it. Without it the object's own axes are unknown and only its extents are drawn.
    pub record: Option<&'a [u8]>,
}

/// One object: its class and its geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub class: Class,
    pub shape: Shape,
    /// Triangles and lines in the drawing's coordinates and units.
    pub geometry: Geometry,
    /// Where the object stands, when its record says so.
    pub frame: Option<Frame>,
    /// Sizes of the box along the object's own axes, for [`Shape::Box`] and [`Shape::Plate`]. For
    /// a wall: length, thickness, height.
    pub sizes: Option<[f64; 3]>,
}

/// Reads one object. `None` when the class name is not of an AutoCAD Architecture object.
pub fn read_object(source: &Source<'_>) -> Option<Object> {
    let class = Class::from_name(source.class_name)?;
    let drawn = source.graphics.and_then(|data| aec_core::decode(data).ok()).unwrap_or_default();
    let header = source.record.and_then(record::header);
    let frame = header.and_then(|header| header.frame());
    if drawn.triangle_count() > 0 {
        return Some(Object { class, shape: Shape::Graphics, geometry: drawn, frame, sizes: None });
    }
    if let Some(solid) = header.filter(|_| class.is_box()).and_then(|header| solid(class, source.record?, &header, drawn.bounds())) {
        let mut geometry = Geometry::default();
        geometry.add_box(solid.center, solid.axes, solid.sizes, Paint::Object);
        if geometry.triangle_count() > 0 {
            let shape = if solid.plate { Shape::Plate } else { Shape::Box };
            return Some(Object { class, shape, geometry, frame, sizes: Some(solid.sizes) });
        }
    }
    let shape = if drawn.is_empty() { Shape::Nothing } else { Shape::Extents };
    Some(Object { class, shape, geometry: drawn, frame, sizes: None })
}

/// Decodes the graphics of any custom object, whatever its class.
pub fn read_graphics(graphics: &[u8]) -> Result<Geometry, GraphicsError> {
    aec_core::decode(graphics)
}

struct Solid {
    center: [f64; 3],
    axes: [[f64; 3]; 3],
    sizes: [f64; 3],
    plate: bool,
}

fn solid(class: Class, record: &[u8], header: &record::Header, bounds: Option<([f64; 3], [f64; 3])>) -> Option<Solid> {
    let wall = (class == Class::Wall).then(|| record::leading_numbers::<3>(record, header)).flatten();
    let spatial = bounds.filter(|(low, high)| high[2] > low[2] && (high[0] > low[0] || high[1] > low[1]));
    let Some((low, high)) = spatial else {
        // Extents without height: the drawing was saved in a plan view. A plain straight wall is
        // then drawn from its own numbers, in the middle of its base line.
        let (frame, [height, width, length]) = (header.frame()?, record::straight_wall(record, header).filter(|_| class == Class::Wall)?);
        return Some(Solid { center: frame.point([length / 2.0, 0.0, height / 2.0]), axes: frame.axes(), sizes: [length, width, height], plate: false });
    };
    let extent = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    let mut center = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0, (low[2] + high[2]) / 2.0];
    let scale = extent.iter().fold(0.0f64, |a, b| a.max(*b));
    let frame = match header.placement {
        Placement::Frame(frame) => frame,
        Placement::Sizes(sizes) => {
            // No axes in the record. When the extents are the sizes of the object along its own
            // axes, the object stands along the axes of the drawing; otherwise it is turned an
            // unknown way.
            let same = sizes.iter().zip(extent).all(|(size, extent)| (size - extent).abs() <= 1e-6 * scale + 1e-9);
            return same.then_some(Solid { center, axes: Frame::WORLD.axes(), sizes: extent, plate: false });
        }
    };
    let positive = |value: f64| value.is_finite() && value > 0.0;
    let known = match class {
        // The width a wall is drawn with comes from its style; the width in its record is the
        // nearest number the object itself has.
        Class::Wall => [record::straight_wall(record, header).map(|[_, _, length]| length), wall.map(|[_, _, width]| width).filter(|width| positive(*width)), None],
        Class::Door | Class::Window => [record::leading_numbers::<2>(record, header).map(|[width, _]| width).filter(|width| positive(*width)), None, None],
        _ => [None; 3],
    };
    let axes = frame.axes();
    let (mut sizes, mut plate) = (aec_core::fit::box_sizes(axes, extent, known), false);
    if sizes.is_none() {
        if let Some(thin) = class.thin_axis() {
            let mut flat = [None; 3];
            flat[thin] = Some(0.0);
            sizes = aec_core::fit::box_sizes(axes, extent, flat);
            plate = sizes.is_some();
        }
    }
    let mut sizes = sizes?;
    // A wall whose roof or floor line was changed reaches higher or lower than its base height;
    // the box keeps the base height, from the base line up.
    if let Some([height, _, _]) = wall.filter(|[height, _, _]| positive(*height) && frame.z[2] > 0.999_999 && (sizes[2] - height).abs() > 1e-6 * scale) {
        center[2] = frame.origin[2] + height / 2.0;
        sizes[2] = height;
    }
    Some(Solid { center, axes, sizes, plate })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aec_core::testing;

    #[test]
    fn names_resolve_by_dxf_name_and_class_name() {
        assert_eq!(Class::from_name("AEC_WALL"), Some(Class::Wall));
        assert_eq!(Class::from_name("AecDbRoofSlab"), Some(Class::RoofSlab));
        assert_eq!(Class::from_name("aecs_member"), Some(Class::Member));
        assert_eq!(Class::from_name("AEC_SCHEDULE_TABLE"), Some(Class::Other));
        assert_eq!(Class::from_name("ACAD_PROXY_ENTITY"), None);
        assert_eq!(Class::from_name("LINE"), None);
    }

    #[test]
    fn every_class_has_its_own_label() {
        let mut labels: Vec<&str> = CLASSES.iter().map(|(_, _, class)| class.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), CLASSES.len());
    }

    fn source<'a>(class_name: &'a str, graphics: &'a [u8], record: &'a [u8]) -> Source<'a> {
        Source { class_name, graphics: Some(graphics), record: Some(record) }
    }

    fn turned(angle: f64, origin: [f64; 3]) -> Frame {
        let (sin, cos) = angle.to_radians().sin_cos();
        Frame { origin, x: [cos, sin, 0.0], y: [-sin, cos, 0.0], z: [0.0, 0.0, 1.0] }
    }

    /// The graphics an application leaves of a box of `sizes` in `frame`, its middle at `center`.
    fn extents_of(frame: &Frame, center: [f64; 3], sizes: [f64; 3]) -> Vec<u8> {
        let mut geometry = Geometry::default();
        geometry.add_box(center, frame.axes(), sizes, Paint::Object);
        let (low, high) = geometry.bounds().unwrap();
        testing::extents(low, high)
    }

    fn volume(geometry: &Geometry) -> f64 {
        geometry
            .corners
            .chunks_exact(3)
            .map(|t| (t[0][0] * (t[1][1] * t[2][2] - t[1][2] * t[2][1]) - t[0][1] * (t[1][0] * t[2][2] - t[1][2] * t[2][0]) + t[0][2] * (t[1][0] * t[2][1] - t[1][1] * t[2][0])) / 6.0)
            .sum()
    }

    #[test]
    fn an_object_without_graphics_or_record_is_nothing() {
        let object = read_object(&Source { class_name: "AEC_DOOR", graphics: None, record: None }).unwrap();
        assert_eq!((object.class, object.shape), (Class::Door, Shape::Nothing));
        assert_eq!(read_object(&Source { class_name: "AEC_DOOR", graphics: Some(&[1, 2, 3]), record: Some(&[4, 5]) }).unwrap().shape, Shape::Nothing);
        assert!(read_object(&Source { class_name: "CIRCLE", graphics: None, record: None }).is_none());
    }

    #[test]
    fn a_turned_wall_is_the_box_along_its_own_axes() {
        let frame = turned(30.0, [100.0, 50.0, 0.0]);
        // Thicker than the width in its record, and beside its base line: as its style has it.
        let (center, sizes) = (frame.point([150.0, 2.0, 60.0]), [300.0, 11.0, 120.0]);
        let record = testing::wall_record(80, &frame, 120.0, 6.0, 300.0);
        let object = read_object(&source("AEC_WALL", &extents_of(&frame, center, sizes), &record)).unwrap();
        assert_eq!(object.shape, Shape::Box);
        assert_eq!(object.geometry.triangle_count(), 12);
        assert!(object.sizes.unwrap().iter().zip(sizes).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!((volume(&object.geometry) - 300.0 * 11.0 * 120.0).abs() < 1e-3);
        assert_eq!(object.frame, Some(frame));
    }

    #[test]
    fn a_wall_at_half_a_right_angle_takes_the_width_of_its_record() {
        let frame = turned(45.0, [0.0; 3]);
        let record = testing::wall_record(80, &frame, 120.0, 6.0, 300.0);
        let object = read_object(&source("AEC_WALL", &extents_of(&frame, frame.point([150.0, 0.0, 60.0]), [300.0, 6.0, 120.0]), &record)).unwrap();
        assert_eq!(object.shape, Shape::Box);
        assert!(object.sizes.unwrap().iter().zip([300.0, 6.0, 120.0]).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn a_wall_with_a_raised_roof_line_keeps_its_base_height() {
        let frame = turned(0.0, [0.0, 0.0, 10.0]);
        let record = testing::wall_record(80, &frame, 120.0, 6.0, 300.0);
        let object = read_object(&source("AEC_WALL", &extents_of(&frame, frame.point([150.0, 0.0, 100.0]), [300.0, 6.0, 200.0]), &record)).unwrap();
        let (low, high) = object.geometry.bounds().unwrap();
        assert_eq!((low[2], high[2]), (10.0, 130.0));
    }

    #[test]
    fn a_wall_saved_in_a_plan_view_is_drawn_from_its_record() {
        let frame = turned(90.0, [10.0, 0.0, 0.0]);
        let record = testing::wall_record(50, &frame, 120.0, 6.0, 72.0);
        let object = read_object(&source("AEC_WALL", &testing::extents([0.0, -7.5, 0.0], [20.0, 80.0, 0.0]), &record)).unwrap();
        assert_eq!(object.shape, Shape::Box);
        let (low, high) = object.geometry.bounds().unwrap();
        assert!(low.iter().zip([7.0, 0.0, 0.0]).chain(high.iter().zip([13.0, 72.0, 120.0])).all(|(a, b)| (a - b).abs() < 1e-9), "{low:?} {high:?}");
        // A door has no such numbers: its flat extents stay lines.
        assert_eq!(read_object(&source("AEC_DOOR", &testing::extents([0.0, 0.0, 0.0], [36.0, 6.0, 0.0]), &record)).unwrap().shape, Shape::Extents);
    }

    #[test]
    fn a_roof_slab_pitched_half_a_right_angle_is_a_plate() {
        let pitch = std::f64::consts::FRAC_1_SQRT_2;
        let frame = Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, pitch, pitch], z: [0.0, -pitch, pitch] };
        let mut writer = testing::Writer::default();
        writer.header(80, 563, &frame);
        let object = read_object(&source("AEC_ROOFSLAB", &extents_of(&frame, [350.0, 60.0, 60.0], [700.0, 160.0, 0.0]), &writer.finish())).unwrap();
        assert_eq!(object.shape, Shape::Plate);
        // Seen from both sides.
        assert_eq!(object.geometry.triangle_count(), 4);
        assert!(object.geometry.is_spatial());
    }

    #[test]
    fn classes_that_are_no_boxes_and_objects_without_axes_keep_their_extents_as_lines() {
        let frame = turned(20.0, [0.0; 3]);
        let record = testing::wall_record(80, &frame, 120.0, 6.0, 300.0);
        let graphics = testing::extents([0.0, 0.0, 0.0], [90.0, 57.5, 41.0]);
        let stair = read_object(&source("AEC_STAIR", &graphics, &record)).unwrap();
        assert_eq!((stair.shape, stair.geometry.triangle_count(), stair.geometry.segment_count()), (Shape::Extents, 0, 12));
        assert_eq!(read_object(&Source { class_name: "AEC_SLAB", graphics: Some(&graphics), record: None }).unwrap().shape, Shape::Extents);
        // A record without axes, with the sizes of the object: a box when the extents are those
        // sizes, lines when the object is turned.
        let mut writer = testing::Writer::default();
        writer.short(80).bit(true).bit(true).short(563).short(80).short(80).short(0).raw(0, 8).bit(false);
        for value in [-90.0, 0.0, 0.0, 90.0, 0.0, 0.0, 0.0, 57.5, 0.0, 0.0, 0.0, 41.0] {
            writer.double(value);
        }
        assert_eq!(read_object(&source("AEC_SLAB", &graphics, &writer.finish())).unwrap().shape, Shape::Box);
        assert_eq!(read_object(&source("AEC_SLAB", &testing::extents([0.0; 3], [100.0, 80.0, 41.0]), &writer.finish())).unwrap().shape, Shape::Extents);
    }

    #[test]
    fn graphics_with_faces_are_taken_as_they_are() {
        let triangle = testing::graphics(&[(7, testing::points(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 2.0]]))]);
        let object = read_object(&Source { class_name: "AEC_MASS_ELEM", graphics: Some(&triangle), record: None }).unwrap();
        assert_eq!((object.shape, object.geometry.triangle_count()), (Shape::Graphics, 1));
    }
}
