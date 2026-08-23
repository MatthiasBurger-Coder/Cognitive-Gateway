# Tiny Swarm World Agent and Skill Migration Decisions (CG-03.02)

## Decision status and source boundary

This document is the deterministic migration map for the CG-03.01 inventory.
It consumes only the 19 role rows and 132 project-skill rows in
[`tiny-swarm-world-agent-skill-inventory.md`](tiny-swarm-world-agent-skill-inventory.md).
The source commit, source paths and source responsibilities remain authoritative
there; this document records the normalized interpretation and disposition.

No prompt, provider, runtime package, named tool or infrastructure reference is
copied into the Cognitive Gateway domain catalog. The decisions below describe
catalog/profile ownership, not authorization. A capability reference remains an
abstract `CapabilityId` and requires an independent policy decision.

## Classification vocabulary

Every candidate has exactly one classification from the issue contract. The
classification answers *what kind of asset this is*; the disposition explains
whether it is materialized now.

| Classification | Meaning | Default owner / disposition |
| --- | --- | --- |
| `generic-core-catalog` | Provider-independent reusable responsibility or skill needed by the gateway domain. | Cognitive Gateway catalog |
| `generic-development` | Reusable engineering, adapter or workspace responsibility without TSW infrastructure assumptions. | Cognitive Gateway catalog |
| `generic-quality` | Reusable testing, verification, evidence or supply-chain responsibility. | Cognitive Gateway catalog |
| `generic-architecture` | Reusable architecture, contract, boundary or service-design responsibility. | Cognitive Gateway catalog |
| `generic-documentation` | Reusable documentation maintenance or information-structure responsibility. | Cognitive Gateway catalog |
| `generic-governance` | Reusable governance semantics. Workflow/process execution remains deferred to CG-05. | Catalog only when declarative; otherwise CG-05 |
| `project-specific:tiny-swarm-world` | Depends on TSW product, repository, platform or current-module conventions. | `tiny-swarm-world` project profile |
| `duplicate/merge-candidate` | Semantics overlap another candidate and must have one named canonical target. | Target’s owner; source becomes an alias or is removed |
| `deprecated` | Retired, prohibited or obsolete material that must not be imported. | No catalog entry; retain only as a guard/provenance |

`catalog` below means a future `AgentDefinition` or `SkillDefinition` entry,
not an implementation in this slice. `profile` means an entry may live only in
the TSW project profile. `deferred(CG-05)` is intentionally not a domain entry.

## Normalization rules

1. Canonical IDs use lowercase ASCII kebab case and the existing strict gateway
   identifier grammar. A source ID is retained as an alias only when it is
   already canonical or when the matrix explicitly names its merge target.
2. A flat `senior-<name>` role is terminology, not a second runtime agent. If
   its responsibility is an actionable responsibility contract, its canonical
   `AgentId` is `<name>` and the source role ID is provenance. Process-only,
   retired and scope-gated roles do not become `AgentDefinition` values.
3. The canonical responsibility is the source responsibility with provider
   packaging, prompt language and concrete TSW infrastructure removed. A
   responsibility that cannot be expressed that way remains project-specific.
4. Docker Swarm, LXD/Incus, Portainer, Linux/WSL, VM/network topology and TSW
   repository conventions are profile-owned. React, Java, Gradle, Jenkins,
   Nexus, SonarQube, NGINX, Kubernetes and Joern remain conditional or profile
   material unless a future verified module proves portability.
5. Workflow, routing, handoff, branch, commit and multi-agent execution
   semantics are governance/process material and are deferred to CG-05 even
   where their policy language is reusable.
6. Duplicate candidates have one target below. No duplicate source is silently
   imported under a second canonical ID.

## Role decision matrix

`AgentDefinition` is `yes` only when the normalized role is a declarative
responsibility contract with reusable or profile-owned skills. `no` means the
role remains process metadata, a guard, or a scope-gated reference.

| Source candidate | Canonical ID | Classification | Owner / disposition | AgentDefinition | Canonical responsibility |
| --- | --- | --- | --- | --- | --- |
| `senior-analysis-storage-architect` | `analysis-storage-architect` | `generic-architecture` | catalog | yes | Analysis and evidence storage boundaries |
| `senior-devops` | `devops` | `project-specific:tiny-swarm-world` | TSW profile | yes | TSW platform automation and runtime operations |
| `senior-documentation-engineer` | `documentation-engineer` | `generic-documentation` | deferred(CG-05); no catalog role | no | Documentation consistency and publication governance |
| `senior-execution-orchestrator` | `execution-orchestrator` | `generic-governance` | deferred(CG-05) | no | Slice ordering, locks and handoffs |
| `senior-git-workspace-specialist` | `git-workspace-specialist` | `generic-development` | catalog | yes | Safe repository and workspace lifecycle |
| `senior-grpc-proto-specialist` | `grpc-proto-specialist` | `generic-architecture` | catalog | yes | Contract and transport-boundary design |
| `senior-java-backend` | `java-backend-guard` | `deprecated` | retired guard only | no | Prevent retired Java/Maven/Spring reintroduction |
| `senior-joern-cpg-specialist` | `joern-cpg-specialist` | `project-specific:tiny-swarm-world` | scope approval required; not cataloged now | no | Optional TSW source-analysis enrichment |
| `senior-performance-engineer` | `performance-engineer` | `generic-development` | catalog | yes | Performance budgets and runtime scalability |
| `senior-plugin-integration-developer` | `plugin-integration-developer` | `generic-architecture` | catalog | yes | Provider-independent plugin handoff contracts |
| `senior-python-automation-developer` | `python-automation-developer` | `project-specific:tiny-swarm-world` | TSW profile | yes | TSW Python automation boundaries |
| `senior-react-frontend` | `react-frontend` | `project-specific:tiny-swarm-world` | conditional TSW profile | yes | Frontend boundary work when a module is verified |
| `senior-security-sandbox-engineer` | `security-sandbox-engineer` | `generic-core-catalog` | catalog | yes | Safe handling of untrusted workspaces |
| `senior-swarm-orchestrator` | `swarm-orchestrator` | `generic-governance` | deferred(CG-05) | no | Multi-role coordination and review sequencing |
| `senior-system-architect` | `system-architect` | `generic-architecture` | catalog | yes | Cross-module boundaries and architecture decisions |
| `senior-tester` | `tester` | `generic-quality` | catalog | yes | Regression and quality-gate verification |
| `senior-ux-designer` | `ux-designer` | `project-specific:tiny-swarm-world` | conditional TSW profile | yes | Operational UI and evidence-review usability |
| `senior-requirement-engineer` | `requirement-engineer` | `generic-governance` | deferred(CG-05) | no | Requirement integrity and traceability |
| `senior-workflow-architect` | `workflow-architect` | `generic-governance` | deferred(CG-05) | no | Executable workflow design and ownership |

The role-to-skill references in CG-03.01 continue to point to source skill
aliases. When a catalog is materialized, they are resolved through the skill
matrix below; no role may introduce an unlisted skill or capability.

## Project-skill decision matrix

The source description in CG-03.01 is the evidence for each normalized purpose.
The compact purpose below removes accidental packaging and retains only the
semantic responsibility needed by the target catalog or profile.

| Source candidate | Canonical ID | Classification | Owner / disposition | Normalized responsibility |
| --- | --- | --- | --- | --- |
| `acceptance-checks` | `acceptance-checks` | `project-specific:tiny-swarm-world` | TSW profile | TSW slice acceptance evidence |
| `adr-steward` | `adr-steward` | `generic-governance` | catalog | ADR lifecycle and decision alignment |
| `agent-handoff-protocol` | `agent-handoff-protocol` | `generic-governance` | deferred(CG-05) | Role handoff metadata and ownership |
| `agent-swarm-coordination-specialist` | `swarm-coordination` | `duplicate/merge-candidate` | merge into `swarm-coordination`; deferred(CG-05) | Multi-role coordination semantics |
| `analysis-storage-architect` | `analysis-storage-architect` | `generic-core-catalog` | catalog | Analysis evidence storage and projections |
| `analytics-persistence-review` | `analytics-persistence-review` | `project-specific:tiny-swarm-world` | TSW profile | TSW analytics persistence review |
| `analytics-slice-workflow` | `analytics-slice-workflow` | `generic-governance` | deferred(CG-05) | Analysis workflow slicing |
| `arc42-architecture-governance` | `arc42-architecture-governance` | `generic-architecture` | catalog | Architecture documentation alignment |
| `architecture-archunit-hexagonal` | `architecture-archunit-hexagonal` | `project-specific:tiny-swarm-world` | conditional TSW profile | Java/ArchUnit boundary checks |
| `architecture-hexagonal` | `architecture-hexagonal` | `generic-architecture` | catalog | Hexagonal boundaries and dependency direction |
| `architecture-modular-monorepo` | `architecture-modular-monorepo` | `project-specific:tiny-swarm-world` | conditional TSW profile | Gradle/module boundary review |
| `audit-evidence-manager` | `audit-evidence-manager` | `generic-quality` | catalog | Findings and remediation evidence |
| `bdd-expert` | `bdd-expert` | `project-specific:tiny-swarm-world` | TSW profile; deferred(CG-05) | TSW behavior scenarios |
| `branch-ci-governance-expert` | `branch-ci-governance-expert` | `generic-governance` | deferred(CG-05) | Branch and CI governance |
| `build-gradle` | `build-gradle` | `project-specific:tiny-swarm-world` | conditional TSW profile | Gradle build verification |
| `code-property-graph-joern-specialist` | `code-property-graph-joern-specialist` | `project-specific:tiny-swarm-world` | scope approval required | Optional Joern/CPG analysis |
| `console-status-ui-developer` | `console-status-ui-developer` | `project-specific:tiny-swarm-world` | TSW profile | Terminal status UI |
| `contract-first-api-steward` | `contract-governance-expert` | `duplicate/merge-candidate` | merge into `contract-governance-expert` | Contract-first API governance |
| `contract-governance-expert` | `contract-governance-expert` | `generic-architecture` | catalog | API, event and transport contracts |
| `data-ownership-persistence-steward` | `data-ownership-persistence-steward` | `generic-architecture` | catalog | Data ownership and persistence boundaries |
| `devops-ci-cd` | `devops-ci-cd` | `generic-development` | catalog | CI/CD automation boundaries |
| `devops-docker` | `devops-docker` | `project-specific:tiny-swarm-world` | conditional TSW profile | Docker and optional analysis containers |
| `devops-kubernetes` | `devops-kubernetes` | `project-specific:tiny-swarm-world` | conditional TSW profile | Kubernetes readiness guidance |
| `distributed-systems-architect` | `distributed-systems-architect` | `generic-core-catalog` | catalog | Long-running distributed execution |
| `docker-engine-installation` | `docker-engine-installation` | `project-specific:tiny-swarm-world` | TSW profile | Docker host prerequisites |
| `docker-registry-bootstrap` | `docker-registry-bootstrap` | `project-specific:tiny-swarm-world` | conditional TSW profile | Registry bootstrap guidance |
| `docker-swarm-initialization` | `docker-swarm-initialization` | `project-specific:tiny-swarm-world` | TSW profile | Swarm initialization guidance |
| `documentation-audience-architect` | `documentation-audience-architect` | `project-specific:tiny-swarm-world` | TSW profile | TSW documentation audience structure |
| `documentation-generation` | `documentation-generation` | `project-specific:tiny-swarm-world` | TSW profile | TSW documentation generation |
| `documentation-sync` | `documentation-sync` | `generic-documentation` | catalog | Documentation and example consistency |
| `engineering-governance` | `engineering-governance` | `generic-governance` | deferred(CG-05) | Cross-artifact engineering governance |
| `execution-profile-router` | `execution-profile-router` | `generic-governance` | deferred(CG-05) | Workflow execution profile routing |
| `flowchart-integrity-auditor` | `flowchart-integrity-auditor` | `generic-governance` | deferred(CG-05) | Governance-flow validation |
| `frontend-developer` | `frontend-developer` | `project-specific:tiny-swarm-world` | conditional TSW profile | TSW terminal UI implementation |
| `frontend-hexagonal` | `frontend-hexagonal` | `generic-architecture` | catalog | Frontend/API boundary separation |
| `frontend-react` | `frontend-react` | `project-specific:tiny-swarm-world` | conditional TSW profile | React work after module verification |
| `frontend-ux-guidelines` | `frontend-ux-guidelines` | `generic-architecture` | catalog | Accessible evidence-review UX |
| `git-branch-strategy` | `git-branch-strategy` | `generic-governance` | deferred(CG-05) | Branch isolation process |
| `git-clean` | `git-clean` | `generic-governance` | deferred(CG-05) | Post-merge workspace lifecycle |
| `git-commit-message-preparation` | `git-commit-message-preparation` | `generic-governance` | deferred(CG-05) | Commit message evidence |
| `git-commit-preparation` | `git-commit-preparation` | `generic-governance` | deferred(CG-05) | Commit/push/PR lifecycle |
| `git-large-repository-specialist` | `git-large-repository-specialist` | `generic-development` | catalog | Large-repository workspace handling |
| `grpc-ingestion` | `grpc-ingestion` | `generic-development` | catalog | Ingestion adapter mapping and validation |
| `grpc-streaming-specialist` | `grpc-streaming-specialist` | `generic-development` | catalog | Streaming transport behavior |
| `hexagonal-architecture-expert` | `architecture-hexagonal` | `duplicate/merge-candidate` | merge into `architecture-hexagonal` | TSW packaging of hexagonal guidance |
| `idempotent-platform-automation` | `idempotent-platform-automation` | `project-specific:tiny-swarm-world` | TSW profile | Safe repeatable platform automation |
| `image-build-publish` | `image-build-publish` | `project-specific:tiny-swarm-world` | conditional TSW profile | Container image build guidance |
| `image-verification` | `image-verification` | `project-specific:tiny-swarm-world` | conditional TSW profile | Non-mutating image verification |
| `image-versioning-tagging` | `image-versioning-tagging` | `project-specific:tiny-swarm-world` | TSW profile | Container image version governance |
| `ingestion-handoff-review` | `ingestion-handoff-review` | `generic-development` | catalog | Ingestion handoff contract review |
| `isms-light-security-governance-expert` | `isms-light-security-governance-expert` | `generic-governance` | catalog | Security management governance |
| `issue-completion-auditor` | `issue-completion-auditor` | `generic-governance` | deferred(CG-05) | Completion and evidence audit process |
| `java-25-backend` | `java-25-backend` | `deprecated` | retired guard only | Prevent retired Java backend reintroduction |
| `jenkins-bootstrap` | `jenkins-bootstrap` | `project-specific:tiny-swarm-world` | conditional TSW profile | Jenkins bootstrap guidance |
| `joern-semantic-analysis` | `joern-semantic-analysis` | `project-specific:tiny-swarm-world` | scope approval required | Optional Joern semantic enrichment |
| `kubernetes-expert` | `kubernetes-expert` | `project-specific:tiny-swarm-world` | conditional TSW profile | Future Kubernetes readiness |
| `linux-host-preparation` | `linux-host-preparation` | `project-specific:tiny-swarm-world` | TSW profile | Linux/WSL host preparation |
| `live-evidence-validation-expert` | `live-evidence-validation-expert` | `generic-quality` | catalog | Live evidence validation and redaction |
| `llm-analysis-expert` | `llm-analysis-expert` | `project-specific:tiny-swarm-world` | advisory TSW profile | LLM-assisted analysis with explicit evidence |
| `mapping-dsl-expert` | `mapping-dsl-expert` | `project-specific:tiny-swarm-world` | TSW profile | TSW mapping DSL guidance |
| `maven-repository-bootstrap` | `maven-repository-bootstrap` | `deprecated` | guarded provenance only | Retired Maven/Nexus build path |
| `microservice-migration-safety-gate` | `microservice-migration-safety-gate` | `generic-governance` | deferred(CG-05) | Migration safety gate process |
| `microservice-runtime-readiness-expert` | `microservice-runtime-readiness-expert` | `generic-architecture` | catalog | Independent service readiness |
| `migration-workflow` | `migration-workflow` | `generic-governance` | deferred(CG-05) | Migration slice process |
| `network-topology-design` | `network-topology-design` | `project-specific:tiny-swarm-world` | TSW profile | TSW VM and Swarm topology |
| `nexus-bootstrap` | `nexus-bootstrap` | `project-specific:tiny-swarm-world` | conditional TSW profile | Nexus bootstrap guidance |
| `observability-and-diagnostics` | `observability-diagnostics` | `duplicate/merge-candidate` | merge into `observability-diagnostics` | TSW wording of diagnostics |
| `observability-diagnostics` | `observability-diagnostics` | `generic-core-catalog` | canonical target; catalog | Logging, metrics and trace correlation |
| `observability-runtime-diagnostics` | `observability-runtime-diagnostics` | `generic-core-catalog` | catalog; distinct governance scope | Runtime-wide diagnostic governance |
| `owasp-asvs-local-infrastructure-expert` | `owasp-asvs-local-infrastructure-expert` | `project-specific:tiny-swarm-world` | TSW profile | Local-infrastructure security controls |
| `performance-scalability-engineer` | `performance-scalability-engineer` | `generic-development` | catalog | Performance and resource scaling |
| `platform-layout-governance` | `platform-layout-governance` | `project-specific:tiny-swarm-world` | TSW profile | TSW repository layout |
| `platform-quality-gates` | `platform-quality-gates` | `project-specific:tiny-swarm-world` | TSW profile | TSW verification gate selection |
| `platform-reset-and-recovery` | `platform-reset-and-recovery` | `project-specific:tiny-swarm-world` | TSW profile | Safe platform recovery |
| `platform-verification` | `platform-verification` | `project-specific:tiny-swarm-world` | TSW profile | Platform verification evidence |
| `portainer-bootstrap` | `portainer-bootstrap` | `project-specific:tiny-swarm-world` | conditional TSW profile | Portainer bootstrap guidance |
| `process-performance-profiler` | `process-performance-profiler` | `generic-governance` | deferred(CG-05) | Workflow process metrics |
| `protobuf-contracts` | `protobuf-contracts` | `generic-development` | catalog | Protobuf contract evolution |
| `python-automation` | `python-automation` | `project-specific:tiny-swarm-world` | canonical TSW profile target | TSW Python automation |
| `python-cli-automation` | `python-cli-automation` | `project-specific:tiny-swarm-world` | TSW profile | TSW CLI orchestration |
| `python-pip-packaging-expert` | `python-pip-packaging-expert` | `project-specific:tiny-swarm-world` | TSW profile | TSW Python packaging |
| `python-senior-developer` | `python-automation` | `duplicate/merge-candidate` | merge into `python-automation` | Seniority wrapper around Python automation |
| `python-test-automation` | `python-test-automation` | `project-specific:tiny-swarm-world` | TSW profile | TSW Python test automation |
| `qms-light-governance-expert` | `qms-light-governance-expert` | `generic-governance` | catalog | Quality-management governance |
| `quality-architecture-validation` | `quality-architecture-validation` | `generic-quality` | catalog | Architecture and dependency validation |
| `quality-archunit-review` | `quality-archunit-review` | `project-specific:tiny-swarm-world` | conditional TSW profile | TSW Python/architecture review |
| `quality-gate` | `quality-gate` | `generic-quality` | catalog | Required quality-gate execution |
| `quality-gate-governance` | `quality-gate-governance` | `generic-governance` | catalog | Quality policy and evidence selection |
| `quality-gate-orchestrator` | `quality-gate-orchestrator` | `generic-governance` | deferred(CG-05) | Workflow quality-gate orchestration |
| `quality-impact-classifier` | `quality-impact-classifier` | `generic-governance` | deferred(CG-05) | Changed-scope gate selection |
| `quality-mutation-testing` | `quality-mutation-testing` | `generic-quality` | conditional catalog | Mutation testing when tooling exists |
| `quality-testing-strategy` | `quality-testing-strategy` | `generic-quality` | catalog | Deterministic regression strategy |
| `registry-infrastructure` | `registry-infrastructure` | `project-specific:tiny-swarm-world` | conditional TSW profile | Artifact/registry infrastructure |
| `release-baseline-governance-expert` | `release-baseline-governance-expert` | `generic-governance` | deferred(CG-05) | Release evidence governance |
| `release-branch-governance` | `release-branch-governance` | `generic-governance` | deferred(CG-05) | Release branch lifecycle |
| `replay-graph-llm-review` | `replay-graph-llm-review` | `project-specific:tiny-swarm-world` | scope approval required | TSW replay/graph/LLM evidence review |
| `replay-runtime-correlation-specialist` | `replay-runtime-correlation-specialist` | `generic-core-catalog` | catalog | Replay and trace correlation semantics |
| `requirement-engineering` | `requirement-engineering` | `generic-governance` | deferred(CG-05) | Requirement lifecycle and traceability |
| `resilience-engineering` | `resilience-engineering` | `generic-core-catalog` | catalog | Retry, timeout and failure semantics |
| `reverse-proxy-routing` | `reverse-proxy-routing` | `project-specific:tiny-swarm-world` | conditional TSW profile | NGINX/reverse-proxy routing |
| `s3d-execution-orchestrator` | `s3d-execution-orchestrator` | `generic-governance` | deferred(CG-05) | Dependency-aware slice execution |
| `sca-migration-expert` | `sca-migration-expert` | `project-specific:tiny-swarm-world` | scope approval required | TSW source-analysis migration guidance |
| `secrets-and-config-management` | `secrets-and-config-management` | `project-specific:tiny-swarm-world` | TSW profile | TSW secret/configuration governance |
| `security-sandbox-specialist` | `security-sandbox-specialist` | `generic-core-catalog` | catalog | Untrusted repository isolation |
| `security-threat-modeling` | `security-threat-modeling` | `generic-core-catalog` | catalog | Threat modeling across boundaries |
| `service-decomposition-bounded-context` | `service-decomposition-bounded-context` | `generic-architecture` | catalog | Service ownership and boundaries |
| `setup-bootstrap-expert` | `setup-bootstrap-expert` | `project-specific:tiny-swarm-world` | TSW profile | Developer environment setup |
| `skill-registry-conflict-auditor` | `skill-registry-conflict-auditor` | `generic-governance` | deferred(CG-05) | Registry overlap and rule-drift audit |
| `sonarqube-bootstrap` | `sonarqube-bootstrap` | `project-specific:tiny-swarm-world` | conditional TSW profile | SonarQube bootstrap guidance |
| `source-analysis-pipeline` | `source-analysis-pipeline` | `generic-core-catalog` | catalog | Source facts and unresolved references |
| `spring-core` | `spring-core` | `deprecated` | legacy guard/provenance only | Retired Spring wiring path |
| `strangler-command-adapter-pattern` | `strangler-command-adapter-pattern` | `project-specific:tiny-swarm-world` | TSW profile; deferred(CG-05) | TSW legacy adapter replacement |
| `supply-chain-security-expert` | `supply-chain-security-expert` | `generic-quality` | catalog | Dependency and artifact supply-chain checks |
| `swagger-ui-bootstrap` | `swagger-ui-bootstrap` | `project-specific:tiny-swarm-world` | conditional TSW profile | Swagger/API UI bootstrap |
| `swarm-coordination` | `swarm-coordination` | `generic-governance` | canonical target; deferred(CG-05) | Bounded multi-role coordination |
| `swarm-node-management` | `swarm-node-management` | `project-specific:tiny-swarm-world` | TSW profile | Docker Swarm node lifecycle |
| `swarm-orchestration` | `swarm-coordination` | `duplicate/merge-candidate` | merge into `swarm-coordination`; deferred(CG-05) | Codex packaging of coordination |
| `swarm-stack-deployment` | `swarm-stack-deployment` | `project-specific:tiny-swarm-world` | TSW profile | Docker Swarm stack deployment |
| `swarm-volume-network-governance` | `swarm-volume-network-governance` | `project-specific:tiny-swarm-world` | TSW profile | Swarm volume and network safety |
| `tdd-expert` | `tdd-expert` | `project-specific:tiny-swarm-world` | TSW profile | TSW test-first automation |
| `terminal-status-dashboard` | `terminal-status-dashboard` | `project-specific:tiny-swarm-world` | TSW profile | Terminal operational status views |
| `testing-junit6` | `testing-junit6` | `project-specific:tiny-swarm-world` | conditional TSW profile | Java/JUnit testing if verified |
| `three-amigos-requirement-gatekeeper` | `three-amigos-requirement-gatekeeper` | `generic-governance` | deferred(CG-05) | Requirement readiness gate |
| `tiny-swarm-world-system-architecture` | `tiny-swarm-world-system-architecture` | `project-specific:tiny-swarm-world` | TSW profile | TSW Linux/WSL and Swarm architecture |
| `traceability-engineer` | `traceability-engineer` | `generic-governance` | catalog | Requirement-to-evidence traceability |
| `workflow-authoring` | `workflow-authoring` | `generic-governance` | deferred(CG-05) | Workflow definition authoring |
| `workflow-conflict-resolution` | `workflow-conflict-resolution` | `generic-governance` | deferred(CG-05) | Workflow and workspace conflict handling |
| `workflow-executor` | `workflow-executor` | `generic-governance` | deferred(CG-05) | Workflow execution process |
| `workflow-orchestration` | `workflow-orchestration` | `generic-governance` | deferred(CG-05) | Workflow slice coordination |
| `workflow-slice` | `workflow-slice` | `generic-governance` | deferred(CG-05) | Slice planning |
| `workflow-slice-execution` | `workflow-slice-execution` | `generic-governance` | deferred(CG-05) | Traceable slice execution |
| `workspace-lifecycle-specialist` | `workspace-lifecycle-specialist` | `generic-development` | catalog | Workspace checkout and lifecycle |

## Duplicate, merge and deprecation decisions

| Source candidate(s) | Canonical target | Decision |
| --- | --- | --- |
| `agent-swarm-coordination-specialist`, `swarm-orchestration` | `swarm-coordination` | Merge coordination semantics; keep Codex/runtime execution details out of the domain. Defer the process contract to CG-05. |
| `contract-first-api-steward` | `contract-governance-expert` | Merge the compatible contract-governance content; preserve the no-shared-implementation constraint as a rule, not a second skill. |
| `hexagonal-architecture-expert` | `architecture-hexagonal` | Merge TSW-specific packaging into the generic boundary contract; retain TSW examples in the profile/docs layer. |
| `observability-and-diagnostics` | `observability-diagnostics` | Merge the narrower TSW wording into the generic diagnostics skill. `observability-runtime-diagnostics` remains distinct because it governs cross-runtime correlation. |
| `python-senior-developer` | `python-automation` | Remove seniority as an identity and keep the TSW automation responsibility under one canonical skill. |
| `senior-<family>` role, matching `.codex/agents` and `.codex/subagents` entries | normalized role ID in the role matrix | These are provenance/runtime-routing views of one responsibility family, not separate domain agents. |
| `senior-java-backend`, `java-25-backend` | none | Deprecated/guarded. They explicitly prevent a retired Java/Maven/Spring direction and must not be imported. |
| `maven-repository-bootstrap`, `spring-core` | none | Deprecated/legacy under the current TSW root direction; preserve only as explicit guards or historical provenance. |

## Generic/project boundary and capability safety

Generic catalog content may mention concepts such as Git, gRPC, Protobuf,
storage, observability, resilience, security or service boundaries, but it may
not assume Docker Swarm, LXD/Incus, Portainer, Linux/WSL, TSW paths, a specific
repository layout, a provider prompt or a runtime package. Such assumptions
belong in the `tiny-swarm-world` profile or an adapter.

Project-profile content may retain those concrete conventions, but profile
ownership does not authorize execution. Tool names, Docker commands, Git
operations, filesystem actions and external services remain abstract
capability references. A future policy must classify and allow/deny each
capability independently before compilation into an execution context.

The six portable `.codex/skills` entries, nine `.codex/subagents` entries, 34
`.codex/agents` entries and six prompt entries in CG-03.01 are supporting
runtime/process evidence, not additional CG-03 domain candidates. They remain
excluded from the catalog and are not copied into either matrix. Their
semantic overlap is covered by the explicit merge rule above; their packaging
is deferred to runtime/workflow work.

## Verification record

The decision matrix is intentionally documentation-only. No runtime migration,
catalog mutation or capability binding is introduced, so no Rust behavior or
coverage target changes in this slice. The accompanying checker validates that
all 19 role rows and all 132 project-skill rows from CG-03.01 occur exactly once
in this matrix, that each has one allowed classification and that every
canonical ID is valid under the gateway identifier grammar.

Run the check from the repository root with:

```powershell
pwsh -NoProfile -File scripts/check-migration-matrix.ps1
```

## CG-03.08 materialization record

The reusable Skill candidates selected by this matrix are materialized under
[`../catalog/skills/`](../catalog/skills/). The catalog contains exactly the
37 canonical Skill IDs classified as catalog-owned: generic core, development,
quality, architecture, documentation and declarative governance skills.

Each document uses the CG-03.03 v1 Skill contract. Its normalized
responsibility is stored in `description`, retrieval hints in
`knowledge_queries`, and both `dependency_ids` and
`required_capability_ids` are present as explicit ordered arrays. Empty arrays
mean that this migration does not assert a dependency or abstract capability
requirement; they are not an implicit execution grant.

The three merged canonical skills preserve every source path in `origin.source`:
`architecture-hexagonal`, `contract-governance-expert` and
`observability-diagnostics`. Project-specific, process-only, scope-gated and
deprecated candidates remain outside the generic catalog for CG-03.09 or the
later workflow/governance slices.
