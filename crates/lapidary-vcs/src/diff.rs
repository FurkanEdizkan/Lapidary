//! Geometric diff (`docs/DATA.md` §6.1): what changed between two revisions, figure by
//! figure, from the figures ingest already recorded. Arithmetic over rows — no geometry is
//! read, so the diff never reaches the kernel or a source file.

use lapidary_core::{Approximate, Delta, RevisionDiff};

/// One revision's recorded figures, each with its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RevisionFigures {
    pub volume_mm3: Option<Approximate<f64>>,
    pub surface_area_mm2: Option<Approximate<f64>>,
    pub bbox_mm: Option<Approximate<[f64; 3]>>,
    /// The tessellation's triangles, which a count states exactly for the mesh it counts.
    pub triangle_count: Option<u32>,
    /// The B-rep's faces and edges, counted exactly. `None` on a mesh.
    pub face_count: Option<u32>,
    pub edge_count: Option<u32>,
    /// Worked out when read from the volume and a typed density, never recorded. Always approximate.
    pub mass_g: Option<Approximate<f64>>,
    /// The centre of the volume, recorded at ingest. `None` for a revision recorded before centres were.
    pub centre_mm: Option<Approximate<[f64; 3]>>,
}

/// `to` against `from`: a delta for every figure both revisions recorded, and none for the rest.
pub fn diff(from: &RevisionFigures, to: &RevisionFigures) -> RevisionDiff {
    RevisionDiff {
        volume_mm3: figure(from.volume_mm3, to.volume_mm3),
        surface_area_mm2: figure(from.surface_area_mm2, to.surface_area_mm2),
        bbox_mm: axes(from.bbox_mm, to.bbox_mm),
        triangle_count: count(from.triangle_count, to.triangle_count),
        face_count: count(from.face_count, to.face_count),
        edge_count: count(from.edge_count, to.edge_count),
        mass_g: figure(from.mass_g, to.mass_g),
        // No percent: a coordinate's share of where it started says only where the origin is, and a
        // centre on its axis at 1e-16 mm would read as moving by thousands of percent.
        centre_mm: axes(from.centre_mm, to.centre_mm).map(|axes| {
            axes.map(|axis| Delta {
                percent: None,
                ..axis
            })
        }),
    }
}

/// A figure with three axes, each axis's change approximate if either end of the figure is.
fn axes(
    from: Option<Approximate<[f64; 3]>>,
    to: Option<Approximate<[f64; 3]>>,
) -> Option<[Delta; 3]> {
    from.zip(to).map(|(from, to)| {
        let approximate = from.is_approximate() || to.is_approximate();
        let (from, to) = (from.value(), to.value());
        [0, 1, 2].map(|axis| delta(from[axis], to[axis], approximate))
    })
}

/// A count's change, exact: a count states exactly what it counts.
fn count(from: Option<u32>, to: Option<u32>) -> Option<Delta> {
    from.zip(to)
        .map(|(from, to)| delta(f64::from(from), f64::from(to), false))
}

/// One measured figure's change, approximate if either end of it is.
fn figure(from: Option<Approximate<f64>>, to: Option<Approximate<f64>>) -> Option<Delta> {
    let (from, to) = (from?, to?);
    Some(delta(
        *from.value(),
        *to.value(),
        from.is_approximate() || to.is_approximate(),
    ))
}

fn delta(from: f64, to: f64, approximate: bool) -> Delta {
    let change = to - from;
    Delta {
        from,
        to,
        change,
        percent: (from != 0.0).then(|| change / from * 100.0),
        approximate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flange's figures, as a mesh kernel records them.
    fn flange(volume: f64, x: f64) -> RevisionFigures {
        RevisionFigures {
            volume_mm3: Some(Approximate::tessellated(volume)),
            surface_area_mm2: Some(Approximate::tessellated(41_210.5)),
            bbox_mm: Some(Approximate::tessellated([x, 150.0, 18.0])),
            triangle_count: Some(36_868),
            face_count: None,
            edge_count: None,
            mass_g: None,
            centre_mm: None,
        }
    }

    /// A centre of mass moves per axis, exactly between two B-reps and approximately where either is a
    /// mesh. A revision recorded before centres were has none to move from.
    #[test]
    fn a_centre_of_mass_moves_per_axis_and_is_exact_only_between_two_b_reps() {
        let at = |centre: [f64; 3], exact: bool| RevisionFigures {
            centre_mm: Some(if exact {
                Approximate::analytic(centre)
            } else {
                Approximate::tessellated(centre)
            }),
            ..RevisionFigures::default()
        };
        let moved = diff(&at([0.0, 0.0, 15.0], true), &at([0.0, 0.0, 20.0], true))
            .centre_mm
            .expect("both have a centre");
        assert_eq!(moved.map(|axis| axis.change), [0.0, 0.0, 5.0]);
        assert!(
            moved.iter().all(|axis| axis.percent.is_none()),
            "a position has no percent: {moved:?}"
        );
        assert!(moved.iter().all(|axis| !axis.approximate));
        let meshed = diff(&at([0.0, 0.0, 15.0], true), &at([0.0, 0.0, 20.0], false))
            .centre_mm
            .expect("both have a centre");
        assert!(meshed.iter().all(|axis| axis.approximate));
        assert!(
            diff(&RevisionFigures::default(), &at([0.0, 0.0, 20.0], true))
                .centre_mm
                .is_none()
        );
    }

    /// The flange widened 10% along X: its volume and its box grow by about a tenth, and every
    /// measured figure still says it came from a mesh.
    #[test]
    fn a_widened_part_reports_its_growth_and_keeps_the_mesh_mark() {
        let changed = diff(&flange(214_780.0, 150.0), &flange(236_258.0, 165.0));

        let volume = changed
            .volume_mm3
            .expect("both revisions measured a volume");
        assert_eq!(volume.change, 21_478.0);
        assert!((volume.percent.expect("a non-zero base") - 10.0).abs() < 1e-9);
        assert!(volume.approximate);

        let [x, y, z] = changed.bbox_mm.expect("both revisions have a box");
        assert_eq!((x.change, y.change, z.change), (15.0, 0.0, 0.0));
        assert!(x.approximate && y.approximate && z.approximate);

        let triangles = changed.triangle_count.expect("both counted");
        assert_eq!(triangles.change, 0.0);
        assert!(
            !triangles.approximate,
            "a count is exact for the mesh it counts"
        );
    }

    #[test]
    fn a_difference_is_approximate_when_either_figure_is() {
        let exact = RevisionFigures {
            volume_mm3: Some(Approximate::analytic(35_840.0)),
            ..RevisionFigures::default()
        };
        let meshed = RevisionFigures {
            volume_mm3: Some(Approximate::tessellated(39_424.0)),
            ..RevisionFigures::default()
        };
        let volume = |from, to| diff(from, to).volume_mm3.expect("both measured");
        assert!(volume(&exact, &meshed).approximate);
        assert!(volume(&meshed, &exact).approximate);
        assert!(!volume(&exact, &exact).approximate);
    }

    /// An open mesh records no volume. A change against it would be a number about nothing,
    /// so there is none — and certainly not a zero.
    #[test]
    fn a_figure_either_revision_did_not_record_has_no_delta() {
        let open = RevisionFigures {
            volume_mm3: None,
            ..flange(0.0, 150.0)
        };
        assert_eq!(diff(&flange(214_780.0, 150.0), &open).volume_mm3, None);
        assert_eq!(diff(&open, &flange(214_780.0, 150.0)).volume_mm3, None);
        assert!(
            diff(&open, &flange(214_780.0, 150.0)).bbox_mm.is_some(),
            "the figures both did record still compare"
        );
    }

    #[test]
    fn a_change_from_zero_has_no_percentage() {
        let empty = RevisionFigures {
            triangle_count: Some(0),
            ..RevisionFigures::default()
        };
        let meshed = RevisionFigures {
            triangle_count: Some(12),
            ..RevisionFigures::default()
        };
        let triangles = diff(&empty, &meshed).triangle_count.expect("both counted");
        assert_eq!(triangles.change, 12.0);
        assert_eq!(triangles.percent, None);
    }

    /// A CAD revision's faces and edges compare exactly, even beside a mesh-derived volume; a mesh on
    /// either side has no counts to compare.
    #[test]
    fn faces_and_edges_compare_exactly_and_only_between_two_b_reps() {
        let bored = |faces, edges| RevisionFigures {
            face_count: Some(faces),
            edge_count: Some(edges),
            ..flange(214_780.0, 150.0)
        };
        let changed = diff(&bored(38, 96), &bored(42, 108));
        let faces = changed.face_count.expect("both counted faces");
        assert_eq!((faces.from, faces.to, faces.change), (38.0, 42.0, 4.0));
        assert!(!faces.approximate, "a count is exact");
        assert_eq!(changed.edge_count.map(|edges| edges.change), Some(12.0));

        let mesh = flange(214_780.0, 150.0);
        assert_eq!(diff(&mesh, &bored(42, 108)).face_count, None);
        assert_eq!(diff(&bored(42, 108), &mesh).edge_count, None);
    }
}
