//! The CI-enforced layering rule from docs/ARCHITECTURE.md.
//!
//! L0 depends on no workspace crate. L1 depends only on L0. L2 depends only on L0 and
//! L1 — never on another L2, never on L3. L3 may depend on anything below it.
//!
//! If two L2 crates need to share something, it moves to lapidary-core.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    L0,
    L1,
    L2,
    L3,
    /// Binaries and xtask. Outside the rule; may depend on anything.
    Bin,
}

/// The authoritative layer assignment. Adding a crate to the workspace without adding
/// it here is itself a failure — see `check`.
pub fn layer_of(crate_name: &str) -> Option<Layer> {
    Some(match crate_name {
        "lapidary-core" => Layer::L0,
        "lapidary-db" | "lapidary-storage" => Layer::L1,
        "lapidary-cad" | "lapidary-jobs" | "lapidary-index" | "lapidary-vcs" | "lapidary-build"
        | "lapidary-targets" => Layer::L2,
        "lapidary-api" | "lapidary-enterprise" => Layer::L3,
        "lapidary-server" | "lapidary" | "xtask" => Layer::Bin,
        _ => return None,
    })
}

/// Workspace crate name -> the workspace crates it depends on.
pub type Graph = BTreeMap<String, Vec<String>>;

#[derive(Debug, PartialEq, Eq)]
pub enum Violation {
    /// A dependency edge the layering rule forbids.
    ForbiddenEdge {
        from: String,
        from_layer: Layer,
        to: String,
        to_layer: Layer,
    },
    /// A workspace member with no entry in `layer_of`.
    UnknownCrate { name: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::ForbiddenEdge {
                from,
                from_layer,
                to,
                to_layer,
            } => write!(
                f,
                "{from} ({from_layer:?}) -> {to} ({to_layer:?}) is forbidden. \
                 L2 crates may depend only on L0 and L1. If these two need to share \
                 something, move it into lapidary-core."
            ),
            Violation::UnknownCrate { name } => write!(
                f,
                "{name} is a workspace member but has no layer. Add it to layer_of() in \
                 xtask/src/layers.rs, choosing its layer from docs/ARCHITECTURE.md."
            ),
        }
    }
}

/// True when a crate at `from` may depend on a crate at `to`.
fn edge_allowed(from: Layer, to: Layer) -> bool {
    match from {
        Layer::L0 => false,
        Layer::L1 => to == Layer::L0,
        Layer::L2 => to == Layer::L0 || to == Layer::L1,
        Layer::L3 => to != Layer::Bin,
        Layer::Bin => true,
    }
}

pub fn check(graph: &Graph) -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();

    for (name, deps) in graph {
        let Some(from_layer) = layer_of(name) else {
            violations.push(Violation::UnknownCrate { name: name.clone() });
            continue;
        };
        for dep in deps {
            let Some(to_layer) = layer_of(dep) else {
                violations.push(Violation::UnknownCrate { name: dep.clone() });
                continue;
            };
            if !edge_allowed(from_layer, to_layer) {
                violations.push(Violation::ForbiddenEdge {
                    from: name.clone(),
                    from_layer,
                    to: dep.clone(),
                    to_layer,
                });
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> Graph {
        edges
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
            .collect()
    }

    #[test]
    fn accepts_a_legal_graph() {
        let g = graph(&[
            ("lapidary-core", &[]),
            ("lapidary-db", &["lapidary-core"]),
            ("lapidary-index", &["lapidary-core", "lapidary-db"]),
            ("lapidary-api", &["lapidary-core", "lapidary-index"]),
        ]);
        assert!(check(&g).is_ok());
    }

    #[test]
    fn rejects_l2_depending_on_l2() {
        let g = graph(&[
            ("lapidary-core", &[]),
            ("lapidary-vcs", &["lapidary-index"]),
            ("lapidary-index", &["lapidary-core"]),
        ]);
        let violations = check(&g).expect_err("L2 -> L2 must be rejected");
        assert_eq!(
            violations,
            vec![Violation::ForbiddenEdge {
                from: "lapidary-vcs".to_owned(),
                from_layer: Layer::L2,
                to: "lapidary-index".to_owned(),
                to_layer: Layer::L2,
            }]
        );
    }

    #[test]
    fn rejects_l2_depending_on_l3() {
        let g = graph(&[("lapidary-jobs", &["lapidary-enterprise"])]);
        let violations = check(&g).expect_err("L2 -> L3 must be rejected");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn rejects_l0_depending_on_anything() {
        let g = graph(&[("lapidary-core", &["lapidary-db"])]);
        assert!(check(&g).is_err(), "L0 must depend on no workspace crate");
    }

    #[test]
    fn rejects_a_member_with_no_layer() {
        let g = graph(&[("lapidary-mystery", &[])]);
        let violations = check(&g).expect_err("unlayered members must be rejected");
        assert_eq!(
            violations,
            vec![Violation::UnknownCrate {
                name: "lapidary-mystery".to_owned()
            }]
        );
    }

    #[test]
    fn allows_bins_to_depend_on_everything() {
        let g = graph(&[("lapidary-server", &["lapidary-api", "lapidary-core"])]);
        assert!(check(&g).is_ok());
    }

    #[test]
    fn violation_message_names_the_edge_and_the_remedy() {
        let v = Violation::ForbiddenEdge {
            from: "lapidary-vcs".to_owned(),
            from_layer: Layer::L2,
            to: "lapidary-index".to_owned(),
            to_layer: Layer::L2,
        };
        let msg = v.to_string();
        assert!(msg.contains("lapidary-vcs"));
        assert!(msg.contains("lapidary-index"));
        assert!(
            msg.contains("lapidary-core"),
            "message must state the remedy"
        );
    }
}
