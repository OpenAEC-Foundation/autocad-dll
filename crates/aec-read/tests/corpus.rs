//! Runs over the drawings in the folder `ACA_SAMPLES` points at (and its subfolders), and reports
//! per file how many AutoCAD Architecture objects it holds and how many gave geometry. Without
//! the variable the test passes without doing anything: sample drawings are in no repository.

use acadrust::entities::ExtendedEntityData;
use acadrust::{DwgReader, EntityType};
use aec_read::{Shape, Source};
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn drawings(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            drawings(&path, out);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dwg")) {
            out.push(path);
        }
    }
}

#[test]
fn every_sample_drawing_is_read() {
    let Some(folder) = std::env::var_os("ACA_SAMPLES") else {
        eprintln!("ACA_SAMPLES is not set: no sample drawings read");
        return;
    };
    let mut paths = Vec::new();
    drawings(Path::new(&folder), &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "no .dwg files under {}", Path::new(&folder).display());
    let (mut files_with_objects, mut files_with_bodies, mut unreadable) = (0, 0, 0);
    let (mut all_objects, mut all_with_geometry) = (0, 0);
    println!("{:>7} {:>8} {:>7} {:>8} {:>9} {:>9}  file", "objects", "geometry", "bodies", "extents", "triangles", "segments");
    for path in &paths {
        let shown = path.strip_prefix(&folder).unwrap_or(path).display();
        let document = match std::fs::read(path).map_err(|error| error.to_string()).and_then(|data| DwgReader::from_stream(Cursor::new(data)).read().map_err(|error| error.to_string())) {
            Ok(document) => document,
            Err(error) => {
                unreadable += 1;
                println!("the drawing does not open ({error})  {shown}");
                continue;
            }
        };
        let (mut objects, mut bodies, mut flat, mut triangles, mut segments) = (0, 0, 0, 0, 0);
        for entity in document.entities() {
            let EntityType::Extended(extended) = entity else { continue };
            let (class_name, record) = match &extended.data {
                ExtendedEntityData::Arch(arch) => (arch.dxf_name.as_str(), arch.raw_dwg_data.as_deref()),
                ExtendedEntityData::Proxy(proxy) => (proxy.dxf_subclass.as_str(), None),
                ExtendedEntityData::RegisteredClass(class) => (class.dxf_name.as_str(), None),
                _ => continue,
            };
            let Some(object) = aec_read::read_object(&Source { class_name, graphics: extended.common.graphic_data.as_deref(), record }) else { continue };
            objects += 1;
            match object.shape {
                Shape::Graphics | Shape::Box | Shape::Plate => bodies += 1,
                Shape::Extents => flat += 1,
                Shape::Nothing => {}
            }
            triangles += object.geometry.triangle_count();
            segments += object.geometry.segment_count();
            // Whatever a drawing holds, what comes out is finite.
            assert!(object.geometry.corners.iter().flatten().all(|value| value.is_finite()), "{shown}");
            assert_eq!(object.geometry.corners.len(), 3 * object.geometry.paints.len(), "{shown}");
        }
        println!("{objects:>7} {:>8} {bodies:>7} {flat:>8} {triangles:>9} {segments:>9}  {shown}", bodies + flat);
        files_with_objects += usize::from(objects > 0);
        files_with_bodies += usize::from(bodies > 0);
        all_objects += objects;
        all_with_geometry += bodies + flat;
    }
    println!("{} drawings: {unreadable} do not open, {files_with_objects} hold objects, {files_with_bodies} hold bodies; {all_with_geometry} of {all_objects} objects gave geometry", paths.len());
}
