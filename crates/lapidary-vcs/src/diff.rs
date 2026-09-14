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
}

/// `to` against `from`: a delta for every figure both revisions recorded, and none for the rest.
pub fn diff(from: &RevisionFigures, to: &RevisionFigures) -> RevisionDiff {
    RevisionDiff {
        volume_mm3: figure(from.volume_mm3, to.volume_mm3),
        surface_area_mm2: figure(from.surface_area_mm2, to.surface_area_mm2),
        bbox_mm: from.bbox_mm.zip(to.bbox_mm).map(|(from, to)| {
            let approximate = from.is_approximate() || to.is_approximate();
            let (from, to) = (from.value(), to.value());
            [0, 1, 2].map(|axis| delta(from[axis], to[axis], approximate))
        }),
        triangle_count: from
            .triangle_count
            .zip(to.triangle_count)
            .map(|(from, to)| delta(f64::from(from), f64::from(to), false)),
    }
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
        }
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
}
