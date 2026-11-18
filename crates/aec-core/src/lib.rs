//! Decoding layer of the AutoCAD Architecture reader.
//!
//! An AutoCAD Architecture object (a wall, a door, a slab) is a custom object in the drawing.
//! Next to its own record it carries the graphics its application left of it for other programs.
//! This crate decodes both: the graphics into triangles and lines ([`graphics`]), the record into
//! where the object stands and some of its sizes ([`record`]), and the two together into the box
//! the object is ([`fit`]). It takes bytes and gives geometry: finding the objects in a drawing
//! is the caller's side.

#![forbid(unsafe_code)]

pub mod fit;
pub mod geometry;
pub mod graphics;
pub mod matrix;
pub mod record;
pub mod testing;
pub mod triangulate;

pub use geometry::{Geometry, Paint, Polyline};
pub use graphics::{decode, records, GraphicsError};
pub use matrix::Matrix;
pub use record::{Frame, Header, Placement};
