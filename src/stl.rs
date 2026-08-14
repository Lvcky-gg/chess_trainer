use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

pub const DEFAULT_CLUSTER_RESOLUTION: u32 = 160;

#[derive(Debug)]
pub enum StlError {
    Io(std::io::Error),
    Malformed(String),
}

impl std::fmt::Display for StlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StlError::Io(e) => write!(f, "io error: {e}"),
            StlError::Malformed(m) => write!(f, "malformed STL: {m}"),
        }
    }
}

impl From<std::io::Error> for StlError {
    fn from(e: std::io::Error) -> Self {
        StlError::Io(e)
    }
}

fn read_binary_stl(path: &Path) -> Result<Vec<Vec3>, StlError> {
    let bytes = std::fs::read(path)?;

    if bytes.len() < 84 {
        return Err(StlError::Malformed(format!(
            "{} is {} bytes, too short for an 84-byte header",
            path.display(),
            bytes.len()
        )));
    }

    let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let expected = 84 + count * 50;
    if bytes.len() < expected {
        return Err(StlError::Malformed(format!(
            "{} declares {count} triangles ({expected} bytes) but is only {} bytes",
            path.display(),
            bytes.len()
        )));
    }

    let mut corners = Vec::with_capacity(count * 3);
    for t in 0..count {
        let base = 84 + t * 50 + 12;
        for v in 0..3 {
            let o = base + v * 12;
            let x = f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
            let y = f32::from_le_bytes([bytes[o + 4], bytes[o + 5], bytes[o + 6], bytes[o + 7]]);
            let z = f32::from_le_bytes([bytes[o + 8], bytes[o + 9], bytes[o + 10], bytes[o + 11]]);
            // Z-up (model) -> Y-up (Bevy).
            corners.push(Vec3::new(x, z, -y));
        }
    }

    Ok(corners)
}

fn cluster_decimate(corners: &[Vec3], resolution: u32) -> (Vec<Vec3>, Vec<u32>) {
    let (min, max) = bounds(corners);
    let extent = (max - min).max_element().max(f32::EPSILON);
    let cell = extent / resolution as f32;

    let key_of = |p: Vec3| -> (i32, i32, i32) {
        (
            ((p.x - min.x) / cell) as i32,
            ((p.y - min.y) / cell) as i32,
            ((p.z - min.z) / cell) as i32,
        )
    };

    let mut cells: HashMap<(i32, i32, i32), u32> = HashMap::new();
    let mut sums: Vec<Vec3> = Vec::new();
    let mut counts: Vec<f32> = Vec::new();

    let mut corner_ids = Vec::with_capacity(corners.len());
    for &p in corners {
        let key = key_of(p);
        let id = *cells.entry(key).or_insert_with(|| {
            sums.push(Vec3::ZERO);
            counts.push(0.0);
            (sums.len() - 1) as u32
        });
        sums[id as usize] += p;
        counts[id as usize] += 1.0;
        corner_ids.push(id);
    }

    let positions: Vec<Vec3> = sums.iter().zip(&counts).map(|(s, c)| *s / *c).collect();

    let mut indices = Vec::with_capacity(corner_ids.len());
    for tri in corner_ids.as_chunks::<3>().0 {
        // Two corners in the same cell means the triangle collapsed to a line.
        if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
            indices.extend_from_slice(tri);
        }
    }

    (positions, indices)
}

fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for &p in points {
        min = min.min(p);
        max = max.max(p);
    }
    (min, max)
}

fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];

    for tri in indices.as_chunks::<3>().0 {
        let (a, b, c) = (
            positions[tri[0] as usize],
            positions[tri[1] as usize],
            positions[tri[2] as usize],
        );
        let face = (b - a).cross(c - a);
        for &i in tri {
            normals[i as usize] += face;
        }
    }

    for n in &mut normals {
        *n = n.normalize_or_zero();
        if *n == Vec3::ZERO {
            *n = Vec3::Y;
        }
    }

    normals
}

fn normalize(positions: &mut [Vec3], target_height: f32) {
    let (min, max) = bounds(positions);
    let size = max - min;
    let scale = if size.y > f32::EPSILON {
        target_height / size.y
    } else {
        1.0
    };

    let centre_xz = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
    for p in positions {
        *p = (*p - centre_xz) * scale;
    }
}

pub fn load_piece_mesh(
    path: impl AsRef<Path>,
    target_height: f32,
    resolution: u32,
) -> Result<Mesh, StlError> {
    let path = path.as_ref();
    let corners = read_binary_stl(path)?;

    let (mut positions, indices) = cluster_decimate(&corners, resolution);
    normalize(&mut positions, target_height);
    let normals = smooth_normals(&positions, &indices);

    info!(
        "loaded {}: {} tris -> {} tris ({} verts)",
        path.file_name().unwrap_or_default().to_string_lossy(),
        corners.len() / 3,
        indices.len() / 3,
        positions.len()
    );

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        positions
            .iter()
            .map(|p| [p.x, p.y, p.z])
            .collect::<Vec<_>>(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        normals.iter().map(|n| [n.x, n.y, n.z]).collect::<Vec<_>>(),
    )
    .with_inserted_indices(Indices::U32(indices));

    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_models_load_and_normalise() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/assets");
        let models = [
            "01-pawn.stl",
            "02-knight_v2.stl",
            "03-bishop.stl",
            "04-rook.stl",
            "05-queen.stl",
            "06-king.stl",
        ];

        for name in models {
            let target = 0.75;
            let mesh = load_piece_mesh(format!("{dir}/{name}"), target, DEFAULT_CLUSTER_RESOLUTION)
                .unwrap_or_else(|e| panic!("{name} failed to load: {e}"));

            let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
                bevy::mesh::VertexAttributeValues::Float32x3(v) => v.clone(),
                other => panic!("unexpected position format: {other:?}"),
            };
            let pts: Vec<Vec3> = positions.iter().map(|p| Vec3::from_array(*p)).collect();
            let (min, max) = bounds(&pts);
            let tris = mesh.indices().unwrap().len() / 3;
            let original = read_binary_stl(Path::new(&format!("{dir}/{name}")))
                .unwrap()
                .len()
                / 3;
            assert!(tris > 500, "{name}: decimated to only {tris} triangles");
            assert!(tris < 200_000, "{name}: {tris} triangles is too many");
            assert!(
                tris * 2 < original,
                "{name}: decimation barely reduced {original} triangles to {tris}"
            );

            assert!(
                (max.y - min.y - target).abs() < 1e-3,
                "{name}: wrong height"
            );
            assert!(min.y.abs() < 1e-3, "{name}: base not resting on y=0");
            assert!((min.x + max.x).abs() < 1e-3, "{name}: not centred on x");
            assert!((min.z + max.z).abs() < 1e-3, "{name}: not centred on z");

            // A chess piece should be taller than it is wide.
            let width = (max.x - min.x).max(max.z - min.z);
            assert!(width < target * 1.4, "{name}: suspiciously wide ({width})");

            println!("{name}: {tris} tris, {} verts, width {width:.3}", pts.len());
        }
    }
}
