use gateway_domain::knowledge_graph::*;
use gateway_domain::{
    Confidence, ContentDigest, ContextScopeId, FreshnessStatus, NonEmptyText, Provenance,
    ProvenanceId, QualityMetadata, ReferenceId, RetrievedFragment, SensitivityClass, SourceId,
    SourceKind, TrustClass, Uncertainty,
};
use std::collections::{BTreeMap, BTreeSet};

fn scope(value: &str) -> ContextScopeId {
    ContextScopeId::new(value).unwrap()
}
fn source() -> SourceId {
    SourceId::new("source").unwrap()
}
fn snapshot(value: char) -> ContentDigest {
    ContentDigest::new(value.to_string().repeat(64)).unwrap()
}
fn provenance(value: &str) -> Provenance {
    Provenance::new(
        ProvenanceId::new(value).unwrap(),
        SourceKind::Repository,
        source(),
        value,
    )
    .unwrap()
}
fn quality(uncertainty: Uncertainty) -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::RetrievedContent,
        SensitivityClass::Normal,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        uncertainty,
    )
}
fn node(id: &str, project: &str) -> GraphNode {
    GraphNode {
        id: GraphNodeId::new(id).unwrap(),
        version: GraphVersion::V1,
        fragment: RetrievedFragment {
            id: ReferenceId::new(id).unwrap(),
            scope: scope(project),
            content: NonEmptyText::new(format!("content {id}")).unwrap(),
            provenance: provenance(id),
            snapshot: snapshot('a'),
            quality: quality(Uncertainty::None),
            evidence: BTreeSet::new(),
        },
    }
}
fn edge(id: &str, from: &str, to: &str, project: &str) -> GraphEdge {
    GraphEdge {
        id: GraphEdgeId::new(id).unwrap(),
        version: GraphVersion::V1,
        from: GraphNodeId::new(from).unwrap(),
        to: GraphNodeId::new(to).unwrap(),
        relation: GraphRelation::DependsOn,
        basis: RelationshipBasis::Observed,
        scope: scope(project),
        provenance: provenance(id),
        snapshot: snapshot('a'),
        quality: quality(Uncertainty::None),
    }
}
fn manifest(projects: &[&str]) -> GraphSourceManifest {
    projects
        .iter()
        .map(|project| ((scope(project), source()), snapshot('a')))
        .collect()
}
fn build(
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    sources: GraphSourceManifest,
) -> Result<GraphProjection, GraphError> {
    GraphProjection::new(
        GraphProjectionId::new("graph").unwrap(),
        GraphVersion::V1,
        sources,
        nodes,
        edges,
    )
}

#[test]
fn rejects_bad_versions_dangling_and_duplicate_identities() {
    assert_eq!(GraphVersion::new(2), Err(GraphError::UnsupportedVersion));
    assert_eq!(GraphVersion::new(0), Err(GraphError::UnsupportedVersion));
    assert_eq!(GraphVersion::new(1).unwrap().number(), 1);
    assert!(GraphNodeId::new(" ").is_err());
    assert_eq!(
        build(
            vec![node("a", "one")],
            vec![edge("ab", "a", "b", "one")],
            manifest(&["one"])
        ),
        Err(GraphError::DanglingReference)
    );
    assert_eq!(
        build(
            vec![node("a", "one"), node("a", "one")],
            vec![],
            manifest(&["one"])
        ),
        Err(GraphError::DuplicateIdentity)
    );
    let mut duplicate_fragment = node("b", "one");
    duplicate_fragment.fragment.id = ReferenceId::new("a").unwrap();
    assert_eq!(
        build(
            vec![node("a", "one"), duplicate_fragment],
            vec![],
            manifest(&["one"])
        ),
        Err(GraphError::DuplicateIdentity)
    );
    assert_eq!(
        build(
            vec![node("a", "one"), node("b", "one")],
            vec![edge("ab", "a", "b", "one"), edge("ab", "a", "b", "one")],
            manifest(&["one"])
        ),
        Err(GraphError::DuplicateIdentity)
    );
    assert_eq!(
        build(vec![], vec![], BTreeMap::new()),
        Err(GraphError::InvalidProjection)
    );
}

#[test]
fn validates_scope_snapshot_and_inference() {
    let nodes = vec![node("a", "one"), node("b", "two")];
    assert_eq!(
        build(
            nodes.clone(),
            vec![edge("ab", "a", "b", "third")],
            manifest(&["one", "two", "third"])
        ),
        Err(GraphError::ScopeMismatch)
    );
    let mut inferred = edge("ab", "a", "b", "one");
    inferred.basis = RelationshipBasis::Inferred;
    assert_eq!(
        build(
            nodes.clone(),
            vec![inferred.clone()],
            manifest(&["one", "two"])
        ),
        Err(GraphError::InvalidProjection)
    );
    inferred.quality = quality(Uncertainty::Probabilistic);
    let graph = build(nodes.clone(), vec![inferred], manifest(&["one", "two"])).unwrap();
    assert_eq!(graph.version(), GraphVersion::V1);
    assert_eq!(graph.id().as_str(), "graph");
    assert_eq!(graph.nodes().len(), 2);
    assert_eq!(graph.edges().len(), 1);
    assert_eq!(graph.sources(), &manifest(&["one", "two"]));
    assert!(graph.ensure_current(&manifest(&["one", "two"])).is_ok());
    let mut stale = manifest(&["one", "two"]);
    stale.insert((scope("one"), source()), snapshot('b'));
    assert_eq!(
        graph.ensure_current(&stale),
        Err(GraphError::StaleProjection)
    );
    assert_eq!(
        build(nodes.clone(), vec![], stale),
        Err(GraphError::StaleProjection)
    );
    let mut stale_edge = edge("ab", "a", "b", "one");
    stale_edge.snapshot = snapshot('b');
    assert_eq!(
        build(nodes, vec![stale_edge], manifest(&["one", "two"])),
        Err(GraphError::StaleProjection)
    );
}

#[test]
fn rebuild_is_independent_of_input_order() {
    let first = build(
        vec![node("a", "one"), node("b", "one")],
        vec![edge("ab", "a", "b", "one")],
        manifest(&["one"]),
    )
    .unwrap();
    let second = build(
        vec![node("b", "one"), node("a", "one")],
        vec![edge("ab", "a", "b", "one")],
        manifest(&["one"]),
    )
    .unwrap();
    assert_eq!(first, second);
}
