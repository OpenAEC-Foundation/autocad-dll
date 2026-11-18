//! `aec`: prints what an AutoCAD Architecture drawing holds and writes an OBJ for checking.

#![forbid(unsafe_code)]

use acadrust::entities::ExtendedEntityData;
use acadrust::{CadDocument, DwgReader, DxfReader, EntityType};
use aec_read::{Class, Geometry, Object, Shape, Source};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  aec info <drawing>             the objects per class, and what they show
  aec list <drawing>             one line per object: class, handle, layer, what it shows
  aec records <drawing>          the graphics record types per class
  aec obj <drawing> <out.obj>    the triangles of the objects as an OBJ file";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["info", path] => open(path).map(|document| info(&document)),
        ["list", path] => open(path).map(|document| list(&document)),
        ["records", path] => open(path).map(|document| records(&document)),
        ["obj", path, out] => open(path).and_then(|document| obj(&document, out)),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn open(path: &str) -> Result<CadDocument, String> {
    let data = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    let dxf = Path::new(path).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dxf"));
    if dxf { DxfReader::from_reader(Cursor::new(data)).and_then(DxfReader::read) } else { DwgReader::from_stream(Cursor::new(data)).read() }.map_err(|error| format!("{path}: {error}"))
}

/// One custom object of the drawing, as the codec hands it over.
struct Found<'a> {
    name: &'a str,
    handle: String,
    layer: &'a str,
    in_model_space: bool,
    graphics: Option<&'a [u8]>,
    record: Option<&'a [u8]>,
}

impl Found<'_> {
    fn read(&self) -> Option<Object> {
        aec_read::read_object(&Source { class_name: self.name, graphics: self.graphics, record: self.record })
    }
}

/// The custom objects of a drawing that are of AutoCAD Architecture.
fn objects(document: &CadDocument) -> Vec<Found<'_>> {
    let model_space: std::collections::BTreeSet<_> = document.model_space_entities().map(|entity| entity.as_entity().common().handle).collect();
    let mut found = Vec::new();
    for entity in document.entities() {
        let EntityType::Extended(extended) = entity else { continue };
        let (name, record) = match &extended.data {
            ExtendedEntityData::Arch(arch) => (arch.dxf_name.as_str(), arch.raw_dwg_data.as_deref()),
            ExtendedEntityData::Proxy(proxy) => (proxy.dxf_subclass.as_str(), None),
            ExtendedEntityData::RegisteredClass(class) => (class.dxf_name.as_str(), None),
            _ => continue,
        };
        if Class::from_name(name).is_none() {
            continue;
        }
        let common = &extended.common;
        found.push(Found {
            name,
            handle: format!("{:X}", common.handle),
            layer: &common.layer,
            in_model_space: model_space.contains(&common.handle),
            graphics: common.graphic_data.as_deref(),
            record,
        });
    }
    found
}

fn info(document: &CadDocument) {
    #[derive(Default)]
    struct Row {
        objects: usize,
        shapes: BTreeMap<Shape, usize>,
        triangles: usize,
        segments: usize,
    }
    let mut rows: BTreeMap<(bool, String), Row> = BTreeMap::new();
    for found in objects(document) {
        let Some(object) = found.read() else { continue };
        let row = rows.entry((!found.in_model_space, format!("{} ({})", found.name, object.class.label()))).or_default();
        row.objects += 1;
        *row.shapes.entry(object.shape).or_default() += 1;
        row.triangles += object.geometry.triangle_count();
        row.segments += object.geometry.segment_count();
    }
    println!("{:<50} {:>7} {:>9} {:>8}  shapes", "class", "objects", "triangles", "segments");
    for ((in_block, name), row) in rows {
        let name = if in_block { format!("{name} [in a block]") } else { name };
        let shapes: Vec<String> = row.shapes.iter().map(|(shape, count)| format!("{count} {}", shape.label())).collect();
        println!("{:<50} {:>7} {:>9} {:>8}  {}", name, row.objects, row.triangles, row.segments, shapes.join(", "));
    }
}

fn list(document: &CadDocument) {
    for found in objects(document) {
        let Some(object) = found.read() else { continue };
        let sizes = object.sizes.map(|s| format!(" sizes=[{:.2} {:.2} {:.2}]", s[0], s[1], s[2])).unwrap_or_default();
        let bounds = object.geometry.bounds().map(|(low, high)| format!(" [{:.1} {:.1} {:.1}]..[{:.1} {:.1} {:.1}]", low[0], low[1], low[2], high[0], high[1], high[2])).unwrap_or_default();
        println!("{:<18} {:>8} {} layer={:?} {:?}{}{}", object.class.label(), found.handle, if found.in_model_space { "model" } else { "block" }, found.layer, object.shape.label(), sizes, bounds);
    }
}

fn records(document: &CadDocument) {
    let mut rows: BTreeMap<&str, (usize, usize, BTreeMap<u32, usize>)> = BTreeMap::new();
    for found in objects(document) {
        let row = rows.entry(found.name).or_default();
        row.0 += 1;
        let Some(graphics) = found.graphics else { continue };
        row.1 += graphics.len();
        for (kind, _) in aec_core::records(graphics).unwrap_or_default() {
            *row.2.entry(kind).or_default() += 1;
        }
    }
    for (name, (count, bytes, kinds)) in rows {
        let kinds: Vec<String> = kinds.iter().map(|(kind, count)| format!("{kind}x{count}")).collect();
        println!("{name:<28} objects={count:<5} bytes={bytes:<9} {}", kinds.join(" "));
    }
}

fn obj(document: &CadDocument, out: &str) -> Result<(), String> {
    let mut text = Vec::new();
    let mut corner = 1usize;
    for found in objects(document).into_iter().filter(|found| found.in_model_space) {
        let Some(object) = found.read() else { continue };
        let Geometry { corners, .. } = &object.geometry;
        if corners.is_empty() {
            continue;
        }
        let _ = writeln!(text, "o {}_{}", object.class.label().replace(' ', "_"), found.handle);
        for p in corners {
            let _ = writeln!(text, "v {} {} {}", p[0], p[1], p[2]);
        }
        for _ in 0..corners.len() / 3 {
            let _ = writeln!(text, "f {} {} {}", corner, corner + 1, corner + 2);
            corner += 3;
        }
    }
    std::fs::write(out, text).map_err(|error| format!("{out}: {error}"))
}
