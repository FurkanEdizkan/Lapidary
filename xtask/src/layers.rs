//! The CI-enforced layering rule from docs/ARCHITECTURE.md.
//!
//! L0 depends on no workspace crate. L1 depends only on L0. L2 depends only on L0 and
//! L1 — never on another L2, never on L3. L3 may depend on L0, L1, L2, and other L3
//! crates, but never on Enterprise and never on a binary.
//!
//! If two L2 crates need to share something, it moves to lapidary-core.
//!
//! `Enterprise` is a wrapper tier above L3, not a fifth peer of L0-L3: it holds only
//! `lapidary-enterprise`, which wraps auth, RBAC and audit around `lapidary-api`. So
//! `lapidary-enterprise → lapidary-api` must be allowed — that's L3→L3 via the
//! `lapidary-api` edge, permitted below. The reverse edge, `lapidary-api →
//! lapidary-enterprise`, would make the free application depend on the enterprise crate,
//! breaking the project rule that the application is free and complete with no gated
//! features. That edge is now forbidden structurally, by `edge_allowed` giving L3 no
//! path to `Enterprise` — not by code review.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    L0,
    L1,
    L2,
    L3,
    /// Wraps L3, not a peer of it. Holds `lapidary-enterprise` alone: licence
    /// verification, auth, RBAC, audit. Exists for one product reason — see the
    /// module doc — so it's named for that reason, not slotted in as `L4`.
    Enterprise,
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
        "lapidary-api" => Layer::L3,
        "lapidary-enterprise" => Layer::Enterprise,
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
            } if *from_layer == Layer::L3 && *to_layer == Layer::Enterprise => write!(
                f,
                "{from} ({from_layer:?}) -> {to} ({to_layer:?}) is forbidden. The \
                 application is free and complete — no gated features. {from} may not \
                 depend on {to}; that edge would make the free application depend on the \
                 enterprise crate. lapidary-enterprise may depend on lapidary-api, never \
                 the reverse."
            ),
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
        Layer::L3 => to == Layer::L0 || to == Layer::L1 || to == Layer::L2 || to == Layer::L3,
        Layer::Enterprise => to != Layer::Bin,
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
        let g = graph(&[("lapidary-jobs", &["lapidary-api"])]);
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

    #[test]
    fn allows_l3_to_depend_on_another_l3_so_enterprise_can_wrap_the_api() {
        // lapidary-enterprise wraps auth/RBAC/audit around lapidary-api, so
        // lapidary-enterprise → lapidary-api must be permitted.
        // The reverse edge, lapidary-api → lapidary-enterprise, is now forbidden
        // structurally — see rejects_api_depending_on_enterprise below.
        let g = graph(&[
            ("lapidary-api", &[]),
            ("lapidary-enterprise", &["lapidary-api"]),
        ]);
        assert!(check(&g).is_ok());
    }

    #[test]
    fn rejects_api_depending_on_enterprise() {
        let g = graph(&[("lapidary-api", &["lapidary-enterprise"])]);
        let violations =
            check(&g).expect_err("lapidary-api -> lapidary-enterprise must be rejected");
        assert_eq!(
            violations,
            vec![Violation::ForbiddenEdge {
                from: "lapidary-api".to_owned(),
                from_layer: Layer::L3,
                to: "lapidary-enterprise".to_owned(),
                to_layer: Layer::Enterprise,
            }]
        );
    }

    #[test]
    fn violation_message_for_api_to_enterprise_names_the_edge_and_the_remedy() {
        let v = Violation::ForbiddenEdge {
            from: "lapidary-api".to_owned(),
            from_layer: Layer::L3,
            to: "lapidary-enterprise".to_owned(),
            to_layer: Layer::Enterprise,
        };
        let msg = v.to_string();
        assert!(msg.contains("lapidary-api"));
        assert!(msg.contains("lapidary-enterprise"));
        assert!(
            msg.contains("free and complete"),
            "message must state the remedy: the app is free and complete"
        );
    }

    #[test]
    fn rejects_enterprise_depending_on_a_binary() {
        let g = graph(&[("lapidary-enterprise", &["lapidary-server"])]);
        let violations = check(&g).expect_err("Enterprise -> Bin must be rejected");
        assert_eq!(violations.len(), 1);
        match &violations[0] {
            Violation::ForbiddenEdge {
                from_layer: Layer::Enterprise,
                to_layer: Layer::Bin,
                ..
            } => {}
            other => panic!("expected Enterprise->Bin violation, got {other:?}"),
        }
    }

    #[test]
    fn rejects_l3_depending_on_bin() {
        let g = graph(&[("lapidary-api", &["lapidary-server"])]);
        let violations = check(&g).expect_err("L3 -> Bin must be rejected");
        assert_eq!(violations.len(), 1);
        match &violations[0] {
            Violation::ForbiddenEdge {
                from_layer: Layer::L3,
                to_layer: Layer::Bin,
                ..
            } => {}
            other => panic!("expected L3->Bin violation, got {other:?}"),
        }
    }

    #[test]
    fn rejects_l1_depending_on_l1() {
        let g = graph(&[("lapidary-db", &[]), ("lapidary-storage", &["lapidary-db"])]);
        let violations = check(&g).expect_err("L1 -> L1 must be rejected");
        assert_eq!(violations.len(), 1);
        match &violations[0] {
            Violation::ForbiddenEdge {
                from_layer: Layer::L1,
                to_layer: Layer::L1,
                ..
            } => {}
            other => panic!("expected L1->L1 violation, got {other:?}"),
        }
    }

    #[test]
    fn rejects_l1_depending_on_l2() {
        let g = graph(&[("lapidary-db", &["lapidary-index"])]);
        let violations = check(&g).expect_err("L1 -> L2 must be rejected");
        assert_eq!(violations.len(), 1);
        match &violations[0] {
            Violation::ForbiddenEdge {
                from_layer: Layer::L1,
                to_layer: Layer::L2,
                ..
            } => {}
            other => panic!("expected L1->L2 violation, got {other:?}"),
        }
    }

    #[test]
    fn rejects_l1_depending_on_l3() {
        let g = graph(&[("lapidary-storage", &["lapidary-api"])]);
        let violations = check(&g).expect_err("L1 -> L3 must be rejected");
        assert_eq!(violations.len(), 1);
        match &violations[0] {
            Violation::ForbiddenEdge {
                from_layer: Layer::L1,
                to_layer: Layer::L3,
                ..
            } => {}
            other => panic!("expected L1->L3 violation, got {other:?}"),
        }
    }

    #[test]
    fn produces_multiple_violations_at_once() {
        // Verify the accumulator collects all violations, not just the first.
        // BTreeMap iteration order is deterministic (alphabetical by crate name).
        let g = graph(&[
            ("lapidary-core", &["lapidary-db"]),   // L0 -> L1 forbidden
            ("lapidary-vcs", &["lapidary-index"]), // L2 -> L2 forbidden
            ("lapidary-jobs", &["lapidary-api"]),  // L2 -> L3 forbidden
        ]);
        let violations = check(&g).expect_err("multiple violations must be collected");
        assert_eq!(
            violations,
            vec![
                Violation::ForbiddenEdge {
                    from: "lapidary-core".to_owned(),
                    from_layer: Layer::L0,
                    to: "lapidary-db".to_owned(),
                    to_layer: Layer::L1,
                },
                Violation::ForbiddenEdge {
                    from: "lapidary-jobs".to_owned(),
                    from_layer: Layer::L2,
                    to: "lapidary-api".to_owned(),
                    to_layer: Layer::L3,
                },
                Violation::ForbiddenEdge {
                    from: "lapidary-vcs".to_owned(),
                    from_layer: Layer::L2,
                    to: "lapidary-index".to_owned(),
                    to_layer: Layer::L2,
                },
            ]
        );
    }
}
