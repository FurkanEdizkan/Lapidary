//! The analytic surfaces and edges a CAD kernel reads from a B-rep, which measurement snaps to.

use serde::Serialize;
use ts_rs::TS;

/// Analytic B-rep entities — axes, radii, normals — that measurement snaps to.
///
/// Typed, never the `Vec<String>` it once was: measurement cannot snap to
/// `"CYLINDRICAL_SURFACE:22.000"` without parsing it back out of a string. A mesh has none,
/// and that emptiness is load-bearing — it is what stops tessellated numbers being presented
/// as exact. `OcctKernel` fills it from `occt-bridge`'s `entities.json`.
///
/// Each entity is in its prototype's own coordinates, once per prototype rather than once per
/// placed instance: the [`AssemblyTree`](crate::AssemblyTree) beside them holds the transforms
/// that place it. `face` and `edge` are 1-based indices into the prototype's faces and edges in
/// OCCT's map order.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entity {
    Plane {
        prototype: String,
        face: u32,
        origin: [f64; 3],
        normal: [f64; 3],
    },
    Cylinder {
        prototype: String,
        face: u32,
        radius: f64,
        origin: [f64; 3],
        axis: [f64; 3],
    },
    Cone {
        prototype: String,
        face: u32,
        ref_radius: f64,
        semi_angle_rad: f64,
        origin: [f64; 3],
        axis: [f64; 3],
    },
    Sphere {
        prototype: String,
        face: u32,
        radius: f64,
        center: [f64; 3],
    },
    Torus {
        prototype: String,
        face: u32,
        major_radius: f64,
        minor_radius: f64,
        origin: [f64; 3],
        axis: [f64; 3],
    },
    Circle {
        prototype: String,
        edge: u32,
        radius: f64,
        center: [f64; 3],
        normal: [f64; 3],
    },
}
