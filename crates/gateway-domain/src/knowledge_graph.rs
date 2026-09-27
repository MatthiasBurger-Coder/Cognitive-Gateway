//! Versioned, derived graph records. Source material remains the authority for
//! its own facts; relationships are advisory retrieval data.
use crate::{
    ContentDigest, ContextScopeId, NonEmptyText, Provenance, QualityMetadata, RetrievedFragment,
    SourceId, Uncertainty, ValidationError,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    UnsupportedVersion,
    InvalidProjection,
    DuplicateIdentity,
    DanglingReference,
    ScopeMismatch,
    StaleProjection,
    InvalidBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphVersion(u16);
impl GraphVersion {
    pub const V1: Self = Self(1);
    pub fn new(value: u16) -> Result<Self, GraphError> {
        (value == 1)
            .then_some(Self(value))
            .ok_or(GraphError::UnsupportedVersion)
    }
    pub const fn number(self) -> u16 {
        self.0
    }
}

macro_rules! graph_id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonEmptyText);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                Ok(Self(NonEmptyText::new(value)?))
            }
            pub fn as_str(&self) -> &str { self.0.as_str() }
        }
    )+};
}
graph_id!(GraphProjectionId, GraphNodeId, GraphEdgeId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphRelation {
    DependsOn,
    References,
    Supports,
    Contradicts,
    RelatedTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RelationshipBasis {
    Observed,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    pub id: GraphNodeId,
    pub version: GraphVersion,
    pub fragment: RetrievedFragment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub id: GraphEdgeId,
    pub version: GraphVersion,
    pub from: GraphNodeId,
    pub to: GraphNodeId,
    pub relation: GraphRelation,
    pub basis: RelationshipBasis,
    /// Scope of the source that made this relationship claim.
    pub scope: ContextScopeId,
    pub provenance: Provenance,
    pub snapshot: ContentDigest,
    pub quality: QualityMetadata,
}

/// Snapshot keys include scope: an equal source ID in another project is a
/// different source. Callers obtain current snapshots from the source boundary.
pub type GraphSourceManifest = BTreeMap<(ContextScopeId, SourceId), ContentDigest>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphProjection {
    id: GraphProjectionId,
    version: GraphVersion,
    sources: GraphSourceManifest,
    nodes: BTreeMap<GraphNodeId, GraphNode>,
    edges: BTreeMap<GraphEdgeId, GraphEdge>,
}

impl GraphProjection {
    pub fn new(
        id: GraphProjectionId,
        version: GraphVersion,
        sources: GraphSourceManifest,
        nodes: impl IntoIterator<Item = GraphNode>,
        edges: impl IntoIterator<Item = GraphEdge>,
    ) -> Result<Self, GraphError> {
        if version != GraphVersion::V1 || sources.is_empty() {
            return Err(GraphError::InvalidProjection);
        }
        let mut node_map = BTreeMap::new();
        let mut fragment_ids = BTreeSet::new();
        for node in nodes {
            if node.version != version {
                return Err(GraphError::UnsupportedVersion);
            }
            let key = (
                node.fragment.scope.clone(),
                node.fragment.provenance.source_id().clone(),
            );
            if sources.get(&key) != Some(&node.fragment.snapshot) {
                return Err(GraphError::StaleProjection);
            }
            if !fragment_ids.insert(node.fragment.id.clone())
                || node_map.insert(node.id.clone(), node).is_some()
            {
                return Err(GraphError::DuplicateIdentity);
            }
        }
        let mut edge_map = BTreeMap::new();
        for edge in edges {
            if edge.version != version {
                return Err(GraphError::UnsupportedVersion);
            }
            let Some(from) = node_map.get(&edge.from) else {
                return Err(GraphError::DanglingReference);
            };
            let Some(to) = node_map.get(&edge.to) else {
                return Err(GraphError::DanglingReference);
            };
            if edge.scope != from.fragment.scope && edge.scope != to.fragment.scope {
                return Err(GraphError::ScopeMismatch);
            }
            let key = (edge.scope.clone(), edge.provenance.source_id().clone());
            if sources.get(&key) != Some(&edge.snapshot) {
                return Err(GraphError::StaleProjection);
            }
            if edge.basis == RelationshipBasis::Inferred
                && edge.quality.uncertainty() == Uncertainty::None
            {
                return Err(GraphError::InvalidProjection);
            }
            if edge_map.insert(edge.id.clone(), edge).is_some() {
                return Err(GraphError::DuplicateIdentity);
            }
        }
        Ok(Self {
            id,
            version,
            sources,
            nodes: node_map,
            edges: edge_map,
        })
    }

    pub fn ensure_current(&self, current: &GraphSourceManifest) -> Result<(), GraphError> {
        if &self.sources == current {
            Ok(())
        } else {
            Err(GraphError::StaleProjection)
        }
    }
    pub fn id(&self) -> &GraphProjectionId {
        &self.id
    }
    pub const fn version(&self) -> GraphVersion {
        self.version
    }
    pub fn sources(&self) -> &GraphSourceManifest {
        &self.sources
    }
    pub fn nodes(&self) -> &BTreeMap<GraphNodeId, GraphNode> {
        &self.nodes
    }
    pub fn edges(&self) -> &BTreeMap<GraphEdgeId, GraphEdge> {
        &self.edges
    }
}
