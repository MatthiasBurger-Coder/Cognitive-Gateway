# Tiny Swarm World Agent, Role and Skill Inventory

## Status and provenance

This is the CG-03.01 factual migration baseline. It records what exists in
Tiny Swarm World; it does not migrate or authorize any asset. Classification
and normalization decisions are recorded in the CG-03.02 decision register:
[`tiny-swarm-world-agent-skill-migration.md`](tiny-swarm-world-agent-skill-migration.md).

| Field | Value |
| --- | --- |
| Source repository | `MatthiasBurger-Coder/Tiny-Swarm-World` |
| Source commit | `27ce3960da98a9ba124fd3f9ff5e003b13e89c60` |
| Source branch at inspection | `feature/classic-public-beta-rc1-stabilization` |
| Inventory repository | `MatthiasBurger-Coder/Cognitive-Gateway` |
| Inventory scope | tracked files at the source commit |
| Inventory date | 2026-08-23 |
| Migration decision | See CG-03.02 decision register |

The source worktree had unrelated uncommitted changes outside the inventoried
candidate subtrees. Three untracked evidence files existed under
`.codex/evidence/`; no uncommitted change was found under
`.agents/roles/`, `.agents/skills/`, `.codex/agents/`, `.codex/subagents/` or
`.codex/skills/`. The commit above is therefore the reproducible source
boundary for the candidate inventory.

## Discovery rules and counts

The source repository distinguishes discoverable project skills from the
supporting Codex team configuration. A role is a responsibility document under
`.agents/roles/`; a role directory containing `SKILL.md` is still counted as a
role, not as an additional project skill.

| Asset class | Discovery rule | Count | Migration relevance |
| --- | --- | ---: | --- |
| Project roles | `.agents/roles/**` (`.md` or `SKILL.md`) | 19 | Candidate role/agent semantics |
| Project skills | `.agents/skills/**/SKILL.md` | 132 | Candidate skill semantics |
| Portable Codex skills | `.codex/skills/**/SKILL.md` | 6 | Supporting/runtime metadata; inspect for overlap |
| Durable subagent descriptions | `.codex/subagents/*.md` | 9 | Supporting role routing metadata |
| Callable Codex agents | `.codex/agents/*.toml` | 34 | Runtime execution metadata, not domain agents |
| Prompts | `.agents/prompts/*.md` | 6 | Workflow/process references only |

All 132 project skill entrypoints contain the required `name` and
`description` frontmatter. The candidate ID below is the frontmatter `name`
for skills and the stable filename stem for flat roles. The two role
directories use their frontmatter `name`.

## Architecture and scope findings

- No project file declares a Cognitive Gateway-style `AgentDefinition`. The
  source uses “role” for responsibility documents and “agent” for Codex team
  runtime configuration. This terminology difference is recorded, not
  silently normalized.
- Role files point to required skills. Those relationships are explicit in the
  role table below and are the only role-to-skill dependencies recorded here.
- Candidate files contain prose guidance, not executable capability bindings.
  Names such as Docker, Swarm, LXD/Incus, Portainer, Git, gRPC, Protobuf,
  Joern/CPG, Kubernetes, React, Java/Maven/Spring, Nexus, Jenkins, SonarQube,
  NGINX and VM/network tooling are recorded as semantic/tool signals. They do
  not grant permission and must remain abstract during later migration.
- `.agents/prompts/`, `.agents/orchestrator/` and workflow files describe
  process selection, lifecycle and execution. They are inspected for
  applicability and handoff, but are excluded from CG-03 migration and
  deferred conceptually to CG-05.
- `.codex/agents/` and `.codex/subagents/` configure callable/runtime roles.
  They are retained as provenance and routing evidence, but are not copied
  into the Cognitive Gateway domain catalog.
- The source explicitly retires Java/Maven/Spring reintroduction through the
  `senior-java-backend` role and `java-25-backend` skill. These are marked
  `deprecated/guard` below rather than treated as reusable candidates.

## Role inventory

Every row has a source path, candidate ID, responsibility, required skill
references and a migration placeholder. “Signals” are observations for
CG-03.02, not the final migration decisions. The completed decision matrix is
maintained in the CG-03.02 register linked above.

| Candidate ID | Source path | Responsibility | Required skill IDs | Signals | Migration |
| --- | --- | --- | --- | --- | --- |
| `senior-analysis-storage-architect` | `.agents/roles/senior-analysis-storage-architect.md` | Analysis-session, raw-ingestion, normalized-store, artifact, graph-projection, indexing, partitioning and trace-correlation storage planning. | `analysis-storage-architect`, `analytics-persistence-review`, `architecture-hexagonal`, `quality-testing-strategy` | analysis/storage/graph | `UNCLASSIFIED` |
| `senior-devops` | `.agents/roles/senior-devops.md` | Python tooling, Docker, Kubernetes, CI/CD, deployment, observability, logging and runtime infrastructure slices. | `devops-ci-cd`, `devops-docker`, `devops-kubernetes`, `observability-diagnostics`, `quality-gate` | platform/runtime; project-specific | `UNCLASSIFIED` |
| `senior-documentation-engineer` | `.agents/roles/senior-documentation-engineer.md` | Documentation consistency, workflows, ADR alignment, agent/skill audits, README updates and workflow handoff documentation. | `arc42-architecture-governance`, `documentation-sync`, `engineering-governance`, `requirement-engineering`, `workflow-slice`, `workflow-slice-execution` | governance/documentation | `UNCLASSIFIED` |
| `senior-execution-orchestrator` | `.agents/roles/senior-execution-orchestrator.md` | S3D execution orchestration, slice metadata, dependency ordering, locks and handoffs. | `release-branch-governance`, `s3d-execution-orchestrator`, `workflow-conflict-resolution` | workflow/process-only | `UNCLASSIFIED` |
| `senior-git-workspace-specialist` | `.agents/roles/senior-git-workspace-specialist.md` | Git checkout, repository references, workspace lifecycle/isolation, source-root preparation, cleanup and large-repository hardening. | `architecture-hexagonal`, `git-large-repository-specialist`, `performance-scalability-engineer`, `security-sandbox-specialist`, `workspace-lifecycle-specialist` | Git/filesystem/security | `UNCLASSIFIED` |
| `senior-grpc-proto-specialist` | `.agents/roles/senior-grpc-proto-specialist.md` | gRPC contracts, Protobuf evolution, DTO boundaries, streaming, validation, deadlines, retries and plugin communication. | `architecture-hexagonal`, `grpc-ingestion`, `grpc-streaming-specialist`, `ingestion-handoff-review`, `protobuf-contracts` | gRPC/Protobuf/API | `UNCLASSIFIED` |
| `senior-java-backend` | `.agents/roles/senior-java-backend.md` | Guard that stops reintroduction of Java, Maven, Gradle or Spring project structure. | none | explicitly retired; guard only | `UNCLASSIFIED` |
| `senior-joern-cpg-specialist` | `.agents/roles/senior-joern-cpg-specialist.md` | Joern/Code Property Graph planning, semantic artifacts, analysis boundaries and large-project CPG risks. | `architecture-hexagonal`, `code-property-graph-joern-specialist`, `joern-semantic-analysis`, `source-analysis-pipeline` | Joern/CPG; optional/out-of-scope | `UNCLASSIFIED` |
| `senior-performance-engineer` | `.agents/roles/senior-performance-engineer.md` | Performance budgets, scalability, repository metrics, quotas, timeouts, asynchronous execution and instrumentation. | `distributed-systems-architect`, `git-large-repository-specialist`, `performance-scalability-engineer`, `quality-gate-governance` | performance/runtime | `UNCLASSIFIED` |
| `senior-plugin-integration-developer` | `.agents/roles/senior-plugin-integration-developer.md` | Plugin handoff contracts, request construction, repository/build/branch/commit context and gRPC client integration. | `architecture-hexagonal`, `grpc-streaming-specialist`, `ingestion-handoff-review`, `protobuf-contracts` | plugin/API; runtime boundary | `UNCLASSIFIED` |
| `senior-python-automation-developer` | `.agents/roles/senior-python-automation-developer.md` | Python implementation slices for domain, application, ports, adapters, YAML, commands and infrastructure automation. | `architecture-hexagonal`, `python-automation`, `quality-architecture-validation`, `quality-gate`, `quality-testing-strategy`, `resilience-engineering` | Python/project-specific | `UNCLASSIFIED` |
| `senior-react-frontend` | `.agents/roles/senior-react-frontend.md` | React components, frontend state, API boundaries, accessibility and performance when a frontend module exists. | `architecture-hexagonal`, `frontend-hexagonal`, `frontend-react`, `frontend-ux-guidelines`, `quality-testing-strategy` | React; conditional | `UNCLASSIFIED` |
| `senior-security-sandbox-engineer` | `.agents/roles/senior-security-sandbox-engineer.md` | Untrusted repository handling, sandbox boundaries, safe Git, filesystem restrictions, quotas, malicious builds and secret leakage prevention. | `devops-docker`, `git-large-repository-specialist`, `security-sandbox-specialist`, `workspace-lifecycle-specialist` | security/filesystem | `UNCLASSIFIED` |
| `senior-swarm-orchestrator` | `.agents/roles/senior-swarm-orchestrator.md` | Multi-role coordination, slice planning, routing, branch coordination, conflict management, review sequencing and quality handoff. | `engineering-governance`, `git-branch-strategy`, `swarm-coordination`, `workflow-authoring`, `workflow-conflict-resolution`, `workflow-slice-execution` | workflow/process-only | `UNCLASSIFIED` |
| `senior-system-architect` | `.agents/roles/senior-system-architect.md` | Cross-module architecture, hexagonal boundaries, dependencies, event flows, scalability, security and architecture reviews. | `architecture-hexagonal`, `architecture-modular-monorepo`, `contract-governance-expert`, `grpc-ingestion`, `microservice-migration-safety-gate`, `microservice-runtime-readiness-expert`, `protobuf-contracts`, `service-decomposition-bounded-context` | architecture/API | `UNCLASSIFIED` |
| `senior-tester` | `.agents/roles/senior-tester.md` | Regression, unit/integration/architecture tests, coverage, mutation guidance and quality-gate validation. | `quality-architecture-validation`, `quality-gate`, `quality-gate-governance`, `quality-mutation-testing`, `quality-testing-strategy` | quality/testing | `UNCLASSIFIED` |
| `senior-ux-designer` | `.agents/roles/senior-ux-designer.md` | UX strategy, information architecture, accessibility, user flows, visualization clarity and design-system consistency. | `frontend-react`, `frontend-ux-guidelines` | React/UX; conditional | `UNCLASSIFIED` |
| `senior-requirement-engineer` | `.agents/roles/senior-requirement-engineer/SKILL.md` | Requirement integrity across EPIC, implementation, arc42, ADRs, workflows, skills and roles. | `arc42-architecture-governance`, `documentation-sync`, `engineering-governance`, `requirement-engineering` | governance; role directory | `UNCLASSIFIED` |
| `senior-workflow-architect` | `.agents/roles/senior-workflow-architect/SKILL.md` | Executable workflow creation, slice ordering, role ownership, planning risks and documentation/workflow regeneration. | `documentation-sync`, `engineering-governance`, `workflow-authoring`, `workflow-conflict-resolution`, `workflow-slice-execution` | workflow/process-only; role directory | `UNCLASSIFIED` |

## Project skill inventory

The `description` column is the source frontmatter purpose/responsibility. A
blank dependency means no dependency is declared in the skill's frontmatter or
body; role references are captured in the role table above. The migration
placeholder is retained as historical inventory evidence; CG-03.02 decisions
are recorded in the separate decision matrix.

| Candidate ID | Source path | Declared purpose / responsibility | Signals | Migration |
| --- | --- | --- | --- | --- |
| `acceptance-checks` | `.agents/skills/acceptance-checks/SKILL.md` | Define done criteria and verification evidence for Tiny Swarm World slices. | TSW/project-specific | `UNCLASSIFIED` |
| `adr-steward` | `.agents/skills/adr-steward/SKILL.md` | Decide when ADRs are required, preserve ADR history, maintain naming, and align workflow, arc42 and governance documentation. | governance | `UNCLASSIFIED` |
| `agent-handoff-protocol` | `.agents/skills/agent-handoff-protocol/SKILL.md` | Define and validate handoffs between workflow roles, callable subagents and reviewers with explicit ownership, inputs, outputs, status and blockers. | workflow/runtime | `UNCLASSIFIED` |
| `agent-swarm-coordination-specialist` | `.agents/skills/agent-swarm-coordination-specialist/SKILL.md` | Dependency graph planning, slice orchestration, multi-agent collaboration, conflict resolution, merge coordination, artifact handoff and review pipelines. | workflow/runtime | `UNCLASSIFIED` |
| `analysis-storage-architect` | `.agents/skills/analysis-storage-architect/SKILL.md` | Raw ingestion storage, normalized analysis stores, session storage, object storage, graph projection boundaries, indexing, partitioning and trace correlation. | storage/graph | `UNCLASSIFIED` |
| `analytics-persistence-review` | `.agents/skills/analytics-persistence-review/SKILL.md` | Review persistence, stored evidence, generated summaries, provenance and deterministic artifact behavior for the current project. | project-specific; analytics | `UNCLASSIFIED` |
| `analytics-slice-workflow` | `.agents/skills/analytics-slice-workflow/SKILL.md` | Create implementation slices for ingestion, source analysis, Joern artifacts, persistence, graph, replay, reports and LLM context preparation. | workflow; Joern/LLM | `UNCLASSIFIED` |
| `arc42-architecture-governance` | `.agents/skills/arc42-architecture-governance/SKILL.md` | Keep arc42 synchronized with EPIC changes, ADRs, runtime behavior, deployment views, service boundaries, resilience, constraints and verified implementation. | architecture/governance | `UNCLASSIFIED` |
| `architecture-archunit-hexagonal` | `.agents/skills/architecture-archunit-hexagonal/SKILL.md` | Add, review or fix ArchUnit rules for hexagonal architecture and forbidden dependencies. | Java/ArchUnit; conditional | `UNCLASSIFIED` |
| `architecture-hexagonal` | `.agents/skills/architecture-hexagonal/SKILL.md` | Preserve hexagonal boundaries, domain isolation, ports/adapters separation and explicit state semantics. | architecture | `UNCLASSIFIED` |
| `architecture-modular-monorepo` | `.agents/skills/architecture-modular-monorepo/SKILL.md` | Module boundaries, Gradle project dependency and monorepo responsibility changes. | Gradle; conditional | `UNCLASSIFIED` |
| `audit-evidence-manager` | `.agents/skills/audit-evidence-manager/SKILL.md` | Audit evidence structure, findings registers, remediation evidence and evidence status discipline. | governance/evidence | `UNCLASSIFIED` |
| `bdd-expert` | `.agents/skills/bdd-expert/SKILL.md` | Behavior scenarios and acceptance language for Tiny Swarm World workflows. | TSW; workflow | `UNCLASSIFIED` |
| `branch-ci-governance-expert` | `.agents/skills/branch-ci-governance-expert/SKILL.md` | Branch protection expectations, CI quality policy, pull-request evidence and merge governance review. | Git/CI governance | `UNCLASSIFIED` |
| `build-gradle` | `.agents/skills/build-gradle/SKILL.md` | Gradle build logic, tasks, dependency verification, toolchains and quality-gate alignment. | Gradle; conditional | `UNCLASSIFIED` |
| `code-property-graph-joern-specialist` | `.agents/skills/code-property-graph-joern-specialist/SKILL.md` | Joern and Code Property Graph planning, data/control flow, taint, graph traversal, semantic analysis and large-project CPG handling. | Joern/CPG; optional | `UNCLASSIFIED` |
| `console-status-ui-developer` | `.agents/skills/console-status-ui-developer/SKILL.md` | Tiny Swarm World terminal status output, progress and recovery UI. | TSW/UI | `UNCLASSIFIED` |
| `contract-first-api-steward` | `.agents/skills/contract-first-api-steward/SKILL.md` | REST, gRPC and Protobuf contract governance with compatibility, error model and no shared Java DTO enforcement. | API; Java/gRPC | `UNCLASSIFIED` |
| `contract-governance-expert` | `.agents/skills/contract-governance-expert/SKILL.md` | REST/OpenAPI, gRPC/Protobuf and event contract governance including versioning, compatibility, DTO boundaries, errors, idempotency, timeouts and shared implementation coupling. | API/runtime | `UNCLASSIFIED` |
| `data-ownership-persistence-steward` | `.agents/skills/data-ownership-persistence-steward/SKILL.md` | Service data ownership, persistence, cross-store governance, projections, event replication and blocking cross-service database coupling. | persistence/service boundaries | `UNCLASSIFIED` |
| `devops-ci-cd` | `.agents/skills/devops-ci-cd/SKILL.md` | CI/CD workflow changes, local CI equivalents and quality-command alignment. | CI/CD | `UNCLASSIFIED` |
| `devops-docker` | `.agents/skills/devops-docker/SKILL.md` | Docker workflow, Joern container and local infrastructure changes that remain optional unless documented. | Docker/Joern; conditional | `UNCLASSIFIED` |
| `devops-kubernetes` | `.agents/skills/devops-kubernetes/SKILL.md` | Kubernetes material only after verifying Kubernetes manifests or deployment tooling. | Kubernetes; conditional | `UNCLASSIFIED` |
| `distributed-systems-architect` | `.agents/skills/distributed-systems-architect/SKILL.md` | Distributed analysis-platform design, long-running jobs, worker lifecycle, backpressure, retry, consistency and failure recovery. | distributed runtime | `UNCLASSIFIED` |
| `docker-engine-installation` | `.agents/skills/docker-engine-installation/SKILL.md` | Docker engine installation guidance in Tiny Swarm World setup documentation. | Docker/TSW | `UNCLASSIFIED` |
| `docker-registry-bootstrap` | `.agents/skills/docker-registry-bootstrap/SKILL.md` | Docker registry bootstrap guidance without live registry mutations. | Docker/registry; conditional | `UNCLASSIFIED` |
| `docker-swarm-initialization` | `.agents/skills/docker-swarm-initialization/SKILL.md` | Docker Swarm initialization guidance without running Swarm mutations. | Docker Swarm/TSW | `UNCLASSIFIED` |
| `documentation-audience-architect` | `.agents/skills/documentation-audience-architect/SKILL.md` | Documentation audience separation, navigation and reader-specific structure in Tiny Swarm World. | TSW/documentation | `UNCLASSIFIED` |
| `documentation-generation` | `.agents/skills/documentation-generation/SKILL.md` | Generate and synchronize Tiny Swarm World documentation. | TSW/documentation | `UNCLASSIFIED` |
| `documentation-sync` | `.agents/skills/documentation-sync/SKILL.md` | Keep project documentation, examples, workflows, ADRs, architecture docs and process instructions consistent with implementation. | documentation/governance | `UNCLASSIFIED` |
| `engineering-governance` | `.agents/skills/engineering-governance/SKILL.md` | Synchronize EPIC, requirements, arc42, ADRs, workflows, quality, resilience, architecture, documentation, skills, roles and Codex coordination. | governance | `UNCLASSIFIED` |
| `execution-profile-router` | `.agents/skills/execution-profile-router/SKILL.md` | Classify workflow create/execute requests as FAST_PATH, NORMAL_PATH or FULL_PATH while preserving governance and quality authority. | workflow/process-only | `UNCLASSIFIED` |
| `flowchart-integrity-auditor` | `.agents/skills/flowchart-integrity-auditor/SKILL.md` | Audit Governance Flowchart V2 decision paths, STOP paths, terminal nodes, loops, publication flow and level consistency. | governance/process | `UNCLASSIFIED` |
| `frontend-developer` | `.agents/skills/frontend-developer/SKILL.md` | Tiny Swarm World console status UI work only, not browser or React frontend development. | TSW/UI; conditional | `UNCLASSIFIED` |
| `frontend-hexagonal` | `.agents/skills/frontend-hexagonal/SKILL.md` | Frontend boundaries separating UI components, frontend state, API adapters and domain evidence models. | frontend/API | `UNCLASSIFIED` |
| `frontend-react` | `.agents/skills/frontend-react/SKILL.md` | React frontend work only after verifying that the repository has a frontend module. | React; conditional | `UNCLASSIFIED` |
| `frontend-ux-guidelines` | `.agents/skills/frontend-ux-guidelines/SKILL.md` | UX, accessibility, evidence-review UI, replay visualization and operational analysis workflows. | UX/frontend | `UNCLASSIFIED` |
| `git-branch-strategy` | `.agents/skills/git-branch-strategy/SKILL.md` | Branch isolation, commit preparation boundaries, staged-file review and line-ending checks. | Git governance | `UNCLASSIFIED` |
| `git-clean` | `.agents/skills/git-clean/SKILL.md` | Clean and synchronize branches after verified PR merge, including fetch/prune, fast-forward, proven merged-branch deletion and blocker reporting. | Git lifecycle | `UNCLASSIFIED` |
| `git-commit-message-preparation` | `.agents/skills/git-commit-message-preparation/SKILL.md` | Draft, review and validate commit messages from actual status, diffs, scope, evidence and repository rules. | Git lifecycle | `UNCLASSIFIED` |
| `git-commit-preparation` | `.agents/skills/git-commit-preparation/SKILL.md` | Prepare, review, commit, push and create PRs while enforcing repository rules, evidence, quality gates and branch lifecycle. | GitHub/Git lifecycle | `UNCLASSIFIED` |
| `git-large-repository-specialist` | `.agents/skills/git-large-repository-specialist/SKILL.md` | Git checkout architecture, large-repository performance, shallow/partial clones, sparse checkout, mirrors and corruption recovery. | Git/filesystem | `UNCLASSIFIED` |
| `grpc-ingestion` | `.agents/skills/grpc-ingestion/SKILL.md` | gRPC ingestion adapters, service mapping, transport validation and correlation preservation. | gRPC/runtime | `UNCLASSIFIED` |
| `grpc-streaming-specialist` | `.agents/skills/grpc-streaming-specialist/SKILL.md` | gRPC/Protobuf contracts, unary/streaming RPCs, uploads, retries, compression, sizing, deadlines and cancellation. | gRPC/Protobuf | `UNCLASSIFIED` |
| `hexagonal-architecture-expert` | `.agents/skills/hexagonal-architecture-expert/SKILL.md` | Tiny Swarm World hexagonal boundary decisions and dependency-direction review. | TSW/architecture | `UNCLASSIFIED` |
| `idempotent-platform-automation` | `.agents/skills/idempotent-platform-automation/SKILL.md` | Idempotency, retry and safe re-run semantics in Tiny Swarm World automation. | TSW/platform | `UNCLASSIFIED` |
| `image-build-publish` | `.agents/skills/image-build-publish/SKILL.md` | Container image build and publish workflow guidance without running Docker commands. | Docker; conditional | `UNCLASSIFIED` |
| `image-verification` | `.agents/skills/image-verification/SKILL.md` | Non-mutating container image verification guidance. | Docker; conditional | `UNCLASSIFIED` |
| `image-versioning-tagging` | `.agents/skills/image-versioning-tagging/SKILL.md` | Container image version and tag governance in Tiny Swarm World. | Docker/TSW | `UNCLASSIFIED` |
| `ingestion-handoff-review` | `.agents/skills/ingestion-handoff-review/SKILL.md` | Review engine requests, gRPC ingestion, payload descriptors and cross-repository handoff contracts. | gRPC/repository | `UNCLASSIFIED` |
| `isms-light-security-governance-expert` | `.agents/skills/isms-light-security-governance-expert/SKILL.md` | ISMS-light scope, security risks, controls, incident response and secret-handling governance. | security/governance | `UNCLASSIFIED` |
| `issue-completion-auditor` | `.agents/skills/issue-completion-auditor/SKILL.md` | Audit whether an issue or workflow satisfies requirements, acceptance criteria, tests, evidence, documentation and completion discipline. | governance/evidence | `UNCLASSIFIED` |
| `java-25-backend` | `.agents/skills/java-25-backend/SKILL.md` | Stop unapproved Java/Maven/Spring Boot reintroduction; retired for Tiny Swarm World. | explicitly retired; guard only | `UNCLASSIFIED` |
| `jenkins-bootstrap` | `.agents/skills/jenkins-bootstrap/SKILL.md` | Jenkins bootstrap guidance without running Jenkins setup. | Jenkins; conditional | `UNCLASSIFIED` |
| `joern-semantic-analysis` | `.agents/skills/joern-semantic-analysis/SKILL.md` | Review and plan optional Joern Docker/CPG semantic enrichment without breaking the default quality gate. | Joern/CPG; optional | `UNCLASSIFIED` |
| `kubernetes-expert` | `.agents/skills/kubernetes-expert/SKILL.md` | Future Kubernetes readiness review without changing the Docker Swarm-first runtime. | Kubernetes; future/conditional | `UNCLASSIFIED` |
| `linux-host-preparation` | `.agents/skills/linux-host-preparation/SKILL.md` | Linux or WSL host prerequisite guidance for Tiny Swarm World. | Linux/WSL/TSW | `UNCLASSIFIED` |
| `live-evidence-validation-expert` | `.agents/skills/live-evidence-validation-expert/SKILL.md` | Live greenpath evidence contracts, redaction, smoke checklists and pass/failure classification. | evidence/runtime | `UNCLASSIFIED` |
| `llm-analysis-expert` | `.agents/skills/llm-analysis-expert/SKILL.md` | LLM-assisted analysis guidance while keeping Tiny Swarm World evidence explicit. | LLM/TSW; advisory | `UNCLASSIFIED` |
| `mapping-dsl-expert` | `.agents/skills/mapping-dsl-expert/SKILL.md` | Tiny Swarm World mapping DSL guidance when command or configuration mappings are modeled. | TSW/DSL | `UNCLASSIFIED` |
| `maven-repository-bootstrap` | `.agents/skills/maven-repository-bootstrap/SKILL.md` | Nexus Maven repository bootstrap guidance without reintroducing Java/Maven build authority. | Maven/Nexus; conditional/guarded | `UNCLASSIFIED` |
| `microservice-migration-safety-gate` | `.agents/skills/microservice-migration-safety-gate/SKILL.md` | Gate production microservice migrations for scope, service target, contract-first sequencing, data ownership, tests, rollback/strangler strategy, risk and evidence. | service/process | `UNCLASSIFIED` |
| `microservice-runtime-readiness-expert` | `.agents/skills/microservice-runtime-readiness-expert/SKILL.md` | Verify independent build, start, test, configuration, observability, health checks and container readiness before calling a candidate a microservice. | service/runtime | `UNCLASSIFIED` |
| `migration-workflow` | `.agents/skills/migration-workflow/SKILL.md` | Plan and execute repository, module or cross-repository migrations in small verifiable slices with architecture and evidence integrity. | migration/process | `UNCLASSIFIED` |
| `network-topology-design` | `.agents/skills/network-topology-design/SKILL.md` | Tiny Swarm World VM and Docker Swarm network topology planning. | VM/network/Swarm/TSW | `UNCLASSIFIED` |
| `nexus-bootstrap` | `.agents/skills/nexus-bootstrap/SKILL.md` | Nexus bootstrap guidance without executing Nexus setup scripts. | Nexus; conditional | `UNCLASSIFIED` |
| `observability-and-diagnostics` | `.agents/skills/observability-and-diagnostics/SKILL.md` | Logging, diagnostics and evidence reporting in Tiny Swarm World. | TSW/observability | `UNCLASSIFIED` |
| `observability-diagnostics` | `.agents/skills/observability-diagnostics/SKILL.md` | Logging, metrics, diagnostics, redaction and trace/correlation observability work. | observability; overlap candidate | `UNCLASSIFIED` |
| `observability-runtime-diagnostics` | `.agents/skills/observability-runtime-diagnostics/SKILL.md` | Governance of correlation IDs, trace context, structured logs, metrics and runtime diagnostics across services, workers and evidence flows. | observability/governance | `UNCLASSIFIED` |
| `owasp-asvs-local-infrastructure-expert` | `.agents/skills/owasp-asvs-local-infrastructure-expert/SKILL.md` | Map OWASP ASVS controls to Tiny Swarm World's local infrastructure and administrative surfaces. | security/TSW | `UNCLASSIFIED` |
| `performance-scalability-engineer` | `.agents/skills/performance-scalability-engineer/SKILL.md` | Memory profiling, large AST/repository handling, parallel analysis, CPU pressure, scan optimization, async/batch execution, streaming and instrumentation. | performance/runtime | `UNCLASSIFIED` |
| `platform-layout-governance` | `.agents/skills/platform-layout-governance/SKILL.md` | Repository layout and documentation path governance in Tiny Swarm World. | TSW/governance | `UNCLASSIFIED` |
| `platform-quality-gates` | `.agents/skills/platform-quality-gates/SKILL.md` | Select Tiny Swarm World verification gates without live infrastructure side effects. | TSW/quality | `UNCLASSIFIED` |
| `platform-reset-and-recovery` | `.agents/skills/platform-reset-and-recovery/SKILL.md` | Reset, cleanup and recovery guidance for Tiny Swarm World platform workflows. | TSW/platform | `UNCLASSIFIED` |
| `platform-verification` | `.agents/skills/platform-verification/SKILL.md` | Safe Tiny Swarm World platform verification planning and evidence reporting. | TSW/platform/evidence | `UNCLASSIFIED` |
| `portainer-bootstrap` | `.agents/skills/portainer-bootstrap/SKILL.md` | Portainer bootstrap guidance without live service mutation. | Portainer; conditional/TSW | `UNCLASSIFIED` |
| `process-performance-profiler` | `.agents/skills/process-performance-profiler/SKILL.md` | Record workflow process metrics without replacing required gates. | workflow/process | `UNCLASSIFIED` |
| `protobuf-contracts` | `.agents/skills/protobuf-contracts/SKILL.md` | Protobuf contract changes, ingestion messages, compatibility review and schema verification. | Protobuf/API | `UNCLASSIFIED` |
| `python-automation` | `.agents/skills/python-automation/SKILL.md` | Tiny Swarm World Python automation across domain, application, ports, adapters, YAML, commands and VM/network/deployment automation. | Python/TSW/platform | `UNCLASSIFIED` |
| `python-cli-automation` | `.agents/skills/python-cli-automation/SKILL.md` | Tiny Swarm World CLI automation, command orchestration and progress reporting. | Python/CLI/TSW | `UNCLASSIFIED` |
| `python-pip-packaging-expert` | `.agents/skills/python-pip-packaging-expert/SKILL.md` | pip, virtual environment and Python dependency guidance for Tiny Swarm World. | Python/TSW | `UNCLASSIFIED` |
| `python-senior-developer` | `.agents/skills/python-senior-developer/SKILL.md` | Senior Python automation design in Tiny Swarm World. | Python/TSW | `UNCLASSIFIED` |
| `python-test-automation` | `.agents/skills/python-test-automation/SKILL.md` | Python unittest fixtures, mocks and deterministic Tiny Swarm World test automation. | Python/testing/TSW | `UNCLASSIFIED` |
| `qms-light-governance-expert` | `.agents/skills/qms-light-governance-expert/SKILL.md` | QMS-light quality objectives, CAPA, change control, audit process and ISO 9001 readiness governance. | quality/governance | `UNCLASSIFIED` |
| `quality-architecture-validation` | `.agents/skills/quality-architecture-validation/SKILL.md` | Current-project architecture validation, package boundaries, import-linter review and module dependency verification. | Python/architecture/quality | `UNCLASSIFIED` |
| `quality-archunit-review` | `.agents/skills/quality-archunit-review/SKILL.md` | Review Python regression tests, import-linter contracts, architecture tests and quality-gate impact. | Python/quality | `UNCLASSIFIED` |
| `quality-gate` | `.agents/skills/quality-gate/SKILL.md` | Identify and execute the repository quality gate without weakening verification rules. | quality/governance | `UNCLASSIFIED` |
| `quality-gate-governance` | `.agents/skills/quality-gate-governance/SKILL.md` | Quality-gate selection, command reporting, dependency verification, coverage and optional external checks. | quality/governance | `UNCLASSIFIED` |
| `quality-gate-orchestrator` | `.agents/skills/quality-gate-orchestrator/SKILL.md` | Plan, execute, classify and report workflow-slice quality gates from QUALITY.md without weakening required checks. | workflow/quality | `UNCLASSIFIED` |
| `quality-impact-classifier` | `.agents/skills/quality-impact-classifier/SKILL.md` | Classify changed files and workflow slices so required quality checks are selected without weakening gates. | workflow/quality | `UNCLASSIFIED` |
| `quality-mutation-testing` | `.agents/skills/quality-mutation-testing/SKILL.md` | Mutation-testing guidance after verifying documented mutation tooling. | quality; conditional | `UNCLASSIFIED` |
| `quality-testing-strategy` | `.agents/skills/quality-testing-strategy/SKILL.md` | Test planning, regression-first workflow, deterministic fixtures and evidence-integrity coverage. | quality/testing | `UNCLASSIFIED` |
| `registry-infrastructure` | `.agents/skills/registry-infrastructure/SKILL.md` | Local registry and artifact repository infrastructure guidance. | registry; conditional | `UNCLASSIFIED` |
| `release-baseline-governance-expert` | `.agents/skills/release-baseline-governance-expert/SKILL.md` | Release baselines, changelog policy, release evidence and readiness governance without publishing releases. | release/governance | `UNCLASSIFIED` |
| `release-branch-governance` | `.agents/skills/release-branch-governance/SKILL.md` | Branch, commit, push, rollback and release-readiness governance tied to workflow slices and quality gates. | Git/release governance | `UNCLASSIFIED` |
| `replay-graph-llm-review` | `.agents/skills/replay-graph-llm-review/SKILL.md` | Review replay, graph projection, reporting and LLM evidence packages without treating generated output as verified evidence. | graph/LLM/evidence | `UNCLASSIFIED` |
| `replay-runtime-correlation-specialist` | `.agents/skills/replay-runtime-correlation-specialist/SKILL.md` | Runtime replay, trace stitching, correlation models, temporal sequencing, causality graphs and stacktrace enrichment. | runtime/graph | `UNCLASSIFIED` |
| `requirement-engineering` | `.agents/skills/requirement-engineering/SKILL.md` | EPIC lifecycle, requirement drift, classification, architecture impact, traceability, constraints, assumptions and implementation comparison. | governance/requirements | `UNCLASSIFIED` |
| `resilience-engineering` | `.agents/skills/resilience-engineering/SKILL.md` | Timeouts, retries, backoff, breakers, bulkheads, idempotency, dead letters, health, readiness, cleanup, correlation, redaction and fail-fast/recoverable decisions. | runtime/resilience | `UNCLASSIFIED` |
| `reverse-proxy-routing` | `.agents/skills/reverse-proxy-routing/SKILL.md` | NGINX or reverse-proxy routing guidance in Tiny Swarm World. | NGINX/TSW; conditional | `UNCLASSIFIED` |
| `s3d-execution-orchestrator` | `.agents/skills/s3d-execution-orchestrator/SKILL.md` | Build workflow-execute dependency graphs, validate slice metadata, topologically group execution, check locks and return execution/blocker decisions. | workflow/process-only | `UNCLASSIFIED` |
| `sca-migration-expert` | `.agents/skills/sca-migration-expert/SKILL.md` | Source-code-analysis migration guidance within Tiny Swarm World governance boundaries. | SCA/TSW | `UNCLASSIFIED` |
| `secrets-and-config-management` | `.agents/skills/secrets-and-config-management/SKILL.md` | Secret handling and configuration governance in Tiny Swarm World. | security/config/TSW | `UNCLASSIFIED` |
| `security-sandbox-specialist` | `.agents/skills/security-sandbox-specialist/SKILL.md` | Untrusted repositories, sandboxing, container isolation, filesystem restrictions, malicious builds, secret leakage, quotas and safe Git. | security/filesystem/Docker | `UNCLASSIFIED` |
| `security-threat-modeling` | `.agents/skills/security-threat-modeling/SKILL.md` | Threat modeling APIs, gRPC, authentication, authorization, secrets, logging, containers, supply chain, repository processing and runtime traces. | security/API/runtime | `UNCLASSIFIED` |
| `service-decomposition-bounded-context` | `.agents/skills/service-decomposition-bounded-context/SKILL.md` | Evaluate bounded-context service ownership, data responsibility, contracts, independent runtime and technical-module distinction. | architecture/service | `UNCLASSIFIED` |
| `setup-bootstrap-expert` | `.agents/skills/setup-bootstrap-expert/SKILL.md` | Developer environment bootstrap guidance without running platform service bootstrap. | setup/TSW | `UNCLASSIFIED` |
| `skill-registry-conflict-auditor` | `.agents/skills/skill-registry-conflict-auditor/SKILL.md` | Inventory skills, detect responsibility overlap, classify governance conflicts and prevent hidden rule drift. | governance/registry | `UNCLASSIFIED` |
| `sonarqube-bootstrap` | `.agents/skills/sonarqube-bootstrap/SKILL.md` | SonarQube bootstrap guidance without adding external static analysis by default. | SonarQube; conditional | `UNCLASSIFIED` |
| `source-analysis-pipeline` | `.agents/skills/source-analysis-pipeline/SKILL.md` | Review source ingestion, static facts, semantic artifacts and unresolved-reference handling. | source analysis | `UNCLASSIFIED` |
| `spring-core` | `.agents/skills/spring-core/SKILL.md` | Spring wiring only when a verified project module already uses Spring; keep it out of domain/application packages. | Spring; conditional/legacy | `UNCLASSIFIED` |
| `strangler-command-adapter-pattern` | `.agents/skills/strangler-command-adapter-pattern/SKILL.md` | Safely replace legacy command adapters in Tiny Swarm World while preserving observable automation behavior. | TSW/migration | `UNCLASSIFIED` |
| `supply-chain-security-expert` | `.agents/skills/supply-chain-security-expert/SKILL.md` | Dependency security, SBOM, container image scan policy and optional supply-chain gates. | security/Docker | `UNCLASSIFIED` |
| `swagger-ui-bootstrap` | `.agents/skills/swagger-ui-bootstrap/SKILL.md` | Swagger UI service bootstrap and routing guidance in Tiny Swarm World. | Swagger/API; conditional | `UNCLASSIFIED` |
| `swarm-coordination` | `.agents/skills/swarm-coordination/SKILL.md` | Coordinate multiple project subagents or roles for bounded parallel work when explicitly requested. | workflow; overlap candidate | `UNCLASSIFIED` |
| `swarm-node-management` | `.agents/skills/swarm-node-management/SKILL.md` | Docker Swarm node lifecycle, labels and role management guidance. | Docker Swarm/TSW | `UNCLASSIFIED` |
| `swarm-orchestration` | `.agents/skills/swarm-orchestration/SKILL.md` | Convert complex tasks into Codex-local multi-agent workflows with reviewers, an orchestrator and an implementation worker. | Codex/workflow; overlap candidate | `UNCLASSIFIED` |
| `swarm-stack-deployment` | `.agents/skills/swarm-stack-deployment/SKILL.md` | Docker Swarm stack deployment guidance without live deployments. | Docker Swarm/TSW | `UNCLASSIFIED` |
| `swarm-volume-network-governance` | `.agents/skills/swarm-volume-network-governance/SKILL.md` | Keep Docker Swarm volumes, overlays, ports and service networks explicit and safe to review. | Docker Swarm/network/TSW | `UNCLASSIFIED` |
| `tdd-expert` | `.agents/skills/tdd-expert/SKILL.md` | Test-first Python automation changes and regression design for Tiny Swarm World. | Python/testing/TSW | `UNCLASSIFIED` |
| `terminal-status-dashboard` | `.agents/skills/terminal-status-dashboard/SKILL.md` | Terminal dashboards and dense operational status views for repeated platform inspection. | TSW/UI | `UNCLASSIFIED` |
| `testing-junit6` | `.agents/skills/testing-junit6/SKILL.md` | Deterministic JUnit 6 unit, integration and architecture tests. | Java/JUnit; conditional | `UNCLASSIFIED` |
| `three-amigos-requirement-gatekeeper` | `.agents/skills/three-amigos-requirement-gatekeeper/SKILL.md` | Validate incoming requirements, architecture fit, quality/testability, dependency cycles, slices, skills and workflow readiness. | governance/process | `UNCLASSIFIED` |
| `tiny-swarm-world-system-architecture` | `.agents/skills/tiny-swarm-world-system-architecture/SKILL.md` | Preserve Tiny Swarm World's Linux/WSL Python identity for LXD/Incus-backed Docker Swarm environments. | TSW/LXD/Incus/Swarm | `UNCLASSIFIED` |
| `traceability-engineer` | `.agents/skills/traceability-engineer/SKILL.md` | Requirement-to-architecture-to-test-to-evidence traceability and gap reporting. | governance/evidence | `UNCLASSIFIED` |
| `workflow-authoring` | `.agents/skills/workflow-authoring/SKILL.md` | Create/regenerate project workflows with slices, dependencies, ownership, architecture constraints, quality gates, stop conditions and documentation lifecycle. | workflow/process-only | `UNCLASSIFIED` |
| `workflow-conflict-resolution` | `.agents/skills/workflow-conflict-resolution/SKILL.md` | Resolve overlap with local changes, user edits, generated files or parallel ownership without losing work. | workflow/Git | `UNCLASSIFIED` |
| `workflow-executor` | `.agents/skills/workflow-executor/SKILL.md` | Execute project workflows with specialist reviews, slice sequencing, quality gates, diff review and commit restrictions. | workflow/process-only | `UNCLASSIFIED` |
| `workflow-orchestration` | `.agents/skills/workflow-orchestration/SKILL.md` | Coordinate workflow slices through dependencies, owners, locks, handoffs and verification gates. | workflow/process-only | `UNCLASSIFIED` |
| `workflow-slice` | `.agents/skills/workflow-slice/SKILL.md` | Create slice-based implementation plans from task, repository rules and workflow documentation. | workflow/process-only | `UNCLASSIFIED` |
| `workflow-slice-execution` | `.agents/skills/workflow-slice-execution/SKILL.md` | Execute small traceable increments with read-only verification, minimal implementation, tests, quality gates and summaries. | workflow/process-only | `UNCLASSIFIED` |
| `workspace-lifecycle-specialist` | `.agents/skills/workspace-lifecycle-specialist/SKILL.md` | Workspace checkout, isolation, cleanup, caching, locking, disk pressure, concurrency, lease expiry and commit pinning. | Git/filesystem | `UNCLASSIFIED` |

## Explicit skill-to-skill references

The following references were found in candidate bodies. They are kept
separate from role requirements because they describe supporting guidance, not
domain dependency declarations.

| Source skill | Referenced skill or runtime asset |
| --- | --- |
| `analysis-storage-architect` | `resilience-engineering` |
| `code-property-graph-joern-specialist` | `resilience-engineering` |
| `devops-ci-cd` | `resilience-engineering` |
| `devops-docker` | `resilience-engineering` |
| `distributed-systems-architect` | `resilience-engineering` |
| `frontend-react` | `resilience-engineering` |
| `git-clean` | `git-commit-preparation` |
| `git-commit-message-preparation` | `git-commit-preparation` |
| `git-commit-preparation` | `git-clean`, `git-commit-message-preparation`, `git-commit-preparation`, `git_commit_operator`, `git_commit_reviewer` |
| `git-large-repository-specialist` | `resilience-engineering` |
| `grpc-ingestion` | `resilience-engineering` |
| `grpc-streaming-specialist` | `resilience-engineering` |
| `joern-semantic-analysis` | `resilience-engineering` |
| `observability-diagnostics` | `resilience-engineering` |
| `performance-scalability-engineer` | `resilience-engineering` |
| `quality-gate-governance` | `resilience-engineering` |
| `source-analysis-pipeline` | `resilience-engineering` |
| `spring-core` | `resilience-engineering` |
| `workflow-executor` | `process-performance-profiler`, `workflow-executor`, `.codex/skills/workflow-executor` |
| `workspace-lifecycle-specialist` | `resilience-engineering` |

## Capability and technology signal register

No candidate declares an executable capability ID, permission, tool binding or
workflow authority. The following are named in responsibilities or guidance
and must be treated as abstract signals during migration:

| Signal family | Observed terms | Boundary hint |
| --- | --- | --- |
| Platform | Docker, Docker Swarm, LXD, Incus, Portainer, VM, network, Linux, WSL | TSW profile unless portability is proven |
| Source and delivery | Git, GitHub, filesystem, YAML, CLI, branch, commit | Adapter/process concern; never domain permission |
| Contracts | REST, OpenAPI, gRPC, Protobuf, plugin, DTO | Port/adapter semantics; no provider coupling |
| Analysis | Joern, CPG, LLM, graph, replay, trace | Optional/advisory evidence; not authority |
| Service/runtime | Kubernetes, Jenkins, Nexus/Maven, SonarQube, NGINX, runtime | Conditional infrastructure guidance |
| UI/language | React, Java, Spring, Gradle, JUnit | Conditional or retired material; not current TSW core |

## Explicit ambiguity, overlap and retirement register

These are source observations only. CG-03.02 must make the migration decision
and record merge targets or deprecation rationale.

| Source candidates | Finding | Required follow-up |
| --- | --- | --- |
| `observability-and-diagnostics`, `observability-diagnostics`, `observability-runtime-diagnostics` | Overlapping observability/diagnostics responsibilities with different scope wording. | Compare ownership and merge/deprecate only with an explicit target. |
| `swarm-coordination`, `swarm-orchestration`, `agent-swarm-coordination-specialist` | Coordination/orchestration overlap; one is explicitly Codex workflow packaging. | Separate reusable coordination semantics from process/runtime packaging. |
| `architecture-hexagonal`, `.codex/skills/hexagonal-architecture-expert` | Similar architecture guidance exists in project and portable layers. | Treat `.codex` as support; identify one canonical reusable semantic contract. |
| `.agents/roles/senior-<family>`, `.codex/agents/senior_<family>.toml`, `.codex/subagents/senior-<family>.md` | Same family appears as role, callable agent and durable subagent description. | Do not duplicate into the domain catalog; preserve provenance and runtime mapping. |
| `senior-java-backend`, `java-25-backend`, `maven-repository-bootstrap`, `spring-core`, Java/Gradle skills | Java/Maven/Spring material is guarded, conditional or retired under TSW root governance. | Mark retired/guarded; never import by existence alone. |
| Joern/CPG and forensic-analysis-related role/skill families | Present as optional analysis support but outside the current TSW product identity. | Require explicit scope approval before treating as migration candidates. |
| `frontend-react`, React role and Codex frontend agents | Conditional on a verified frontend module; current TSW routing defaults to terminal UI. | Keep conditional and separate browser/frontend packaging from generic semantics. |
| `docker-*`, `swarm-*`, `portainer-*`, `network-topology-design`, `tiny-swarm-world-system-architecture` | Concrete platform assumptions: Docker Swarm, LXD/Incus, Portainer, Linux/WSL, VM/network. | Candidate for project profile, not generic catalog, unless semantics are proven portable. |

No explicit duplicate ID, explicit deprecation marker beyond the retired Java
guards, or machine-readable capability binding was found in the candidate
roots at the source commit. Absence is recorded as an evidence result, not as
proof that future source revisions will remain unchanged.

## Applicability and activation evidence

- `.agents/orchestrator/routing-rules.md` routes Python, architecture,
  documentation, testing, DevOps, security and workflow concerns to the role
  families listed above.
- `.agents/activation/resolver.py` only conditionally activates `browser` with
  `browser-module-present` evidence and `frontend-react` with
  `verified-frontend-module` evidence; external skills require explicit
  approval. It does not select workflows or grant capabilities.
- `.agents/AGENTS.md` requires discoverable project skills to be directories
  containing `SKILL.md` with `name` and `description` frontmatter. This is the
  rule used for the 132 count.
- `.codex/AGENTS.md` states that `.codex/agents/` is project-scoped callable
  configuration and that `.codex/skills/` is reusable team material. That is
  why those assets are included as supporting evidence, not as domain
  candidates.

## Supporting `.codex` inventory

These assets are included so later migration work can distinguish semantic
content from runtime packaging. Their migration decision is
`EXCLUDED_FROM_DOMAIN_CATALOG` unless CG-03.02 explicitly identifies reusable
semantics.

### Portable Codex skills

| ID | Source path | Purpose signal |
| --- | --- | --- |
| `archunit-expert` | `.codex/skills/archunit-expert/SKILL.md` | ArchUnit architecture tests and forbidden dependencies; Java/conditional. |
| `hexagonal-architecture-expert` | `.codex/skills/hexagonal-architecture-expert/SKILL.md` | Hexagonal boundary and dependency-direction review. |
| `junit6-expert` | `.codex/skills/junit6-expert/SKILL.md` | JUnit 6 tests and deterministic fixtures; Java/conditional. |
| `microservice-architecture-expert` | `.codex/skills/microservice-architecture-expert/SKILL.md` | Service split, deployment autonomy and no-shared-implementation review. |
| `protobuf-grpc-expert` | `.codex/skills/protobuf-grpc-expert/SKILL.md` | Protobuf/gRPC contracts, streaming, validation and compatibility. |
| `workflow-executor` | `.codex/skills/workflow-executor/SKILL.md` | Portable workflow execution protocol; process-only. |

### Durable subagent descriptions

| ID | Source path | Purpose signal |
| --- | --- | --- |
| `agent-workflow-orchestrator` | `.codex/subagents/agent-workflow-orchestrator.md` | Coordinates workflow discovery, routing, slices and handoffs. |
| `documentation-engineer` | `.codex/subagents/documentation-engineer.md` | Documentation consistency and audit artifact ownership. |
| `senior-devops-engineer` | `.codex/subagents/senior-devops-engineer.md` | DevOps/runtime review routing. |
| `senior-java-backend-developer` | `.codex/subagents/senior-java-backend-developer.md` | Java example routing; retired/conditional. |
| `senior-python-automation-developer` | `.codex/subagents/senior-python-automation-developer.md` | Python implementation routing. |
| `senior-react-frontend-developer` | `.codex/subagents/senior-react-frontend-developer.md` | React routing; conditional. |
| `senior-system-architect` | `.codex/subagents/senior-system-architect.md` | Architecture review routing. |
| `senior-tester` | `.codex/subagents/senior-tester.md` | Test and quality review routing. |
| `senior-ux-designer` | `.codex/subagents/senior-ux-designer.md` | UX/accessibility review routing. |

### Callable agent IDs

The 34 TOML files below are runtime metadata. The `name` is the callable agent
ID; descriptions are intentionally summarized to show why each is not a
domain-agent candidate.

| ID | Source path | Runtime responsibility |
| --- | --- | --- |
| `analytics_persistence_reviewer` | `.codex/agents/analytics_persistence_reviewer.toml` | Reviews analytics persistence, evidence, provenance and deterministic artifacts. |
| `architecture_forensic_analytics_architect` | `.codex/agents/architecture_forensic_analytics_architect.toml` | Reviews TSW architecture, evidence, replay, graph, LLM and module responsibility. |
| `architecture_reviewer` | `.codex/agents/architecture_reviewer.toml` | Reviews architecture boundaries, dependencies, ports/adapters and migration risks. |
| `documentation_reviewer` | `.codex/agents/documentation_reviewer.toml` | Reviews README, workflows, examples, governance and migration documentation. |
| `git_commit_operator` | `.codex/agents/git_commit_operator.toml` | Executes commit/push/PR lifecycle after review. |
| `git_commit_reviewer` | `.codex/agents/git_commit_reviewer.toml` | Reviews commit readiness, evidence, message and push eligibility. |
| `implementation_worker` | `.codex/agents/implementation_worker.toml` | Implements one approved slice with targeted changes. |
| `ingestion_handoff_reviewer` | `.codex/agents/ingestion_handoff_reviewer.toml` | Reviews ingestion and cross-repository handoff contracts. |
| `joern_semantics_reviewer` | `.codex/agents/joern_semantics_reviewer.toml` | Reviews Joern/CPG semantic enrichment. |
| `quality_archunit_reviewer` | `.codex/agents/quality_archunit_reviewer.toml` | Reviews tests, architecture checks and quality impact. |
| `quality_reviewer` | `.codex/agents/quality_reviewer.toml` | Reviews tests, coverage, build and dependency verification. |
| `replay_graph_llm_reviewer` | `.codex/agents/replay_graph_llm_reviewer.toml` | Reviews replay, graph, reporting and LLM evidence risks. |
| `repository_explorer` | `.codex/agents/repository_explorer.toml` | Explores structure, build setup and process docs read-only. |
| `security_reviewer` | `.codex/agents/security_reviewer.toml` | Reviews security, dependency, secret and test-isolation risks. |
| `senior_analysis_storage_architect` | `.codex/agents/senior_analysis_storage_architect.toml` | Callable storage/analysis role. |
| `senior_devops` | `.codex/agents/senior_devops.toml` | Callable DevOps, Docker/Swarm, CI and runtime role. |
| `senior_documentation_engineer` | `.codex/agents/senior_documentation_engineer.toml` | Callable documentation, workflow and audit role. |
| `senior_git_workspace_specialist` | `.codex/agents/senior_git_workspace_specialist.toml` | Callable Git/workspace lifecycle role. |
| `senior_grpc_proto_specialist` | `.codex/agents/senior_grpc_proto_specialist.toml` | Callable gRPC/Protobuf role. |
| `senior_java_backend` | `.codex/agents/senior_java_backend.toml` | Callable Java example role; conditional/retired. |
| `senior_joern_cpg_specialist` | `.codex/agents/senior_joern_cpg_specialist.toml` | Callable Joern/CPG role. |
| `senior_performance_engineer` | `.codex/agents/senior_performance_engineer.toml` | Callable performance/scalability role. |
| `senior_plugin_integration_developer` | `.codex/agents/senior_plugin_integration_developer.toml` | Callable plugin/gRPC integration role. |
| `senior_python_automation_developer` | `.codex/agents/senior_python_automation_developer.toml` | Callable Python automation role. |
| `senior_react_frontend` | `.codex/agents/senior_react_frontend.toml` | Callable React role; conditional. |
| `senior_requirement_engineer` | `.codex/agents/senior_requirement_engineer.toml` | Callable requirements/traceability role. |
| `senior_security_sandbox_engineer` | `.codex/agents/senior_security_sandbox_engineer.toml` | Callable security/sandbox role. |
| `senior_swarm_orchestrator` | `.codex/agents/senior_swarm_orchestrator.toml` | Callable slice planning and coordination role. |
| `senior_system_architect` | `.codex/agents/senior_system_architect.toml` | Callable architecture role. |
| `senior_tester` | `.codex/agents/senior_tester.toml` | Callable Python test/quality role. |
| `senior_ux_designer` | `.codex/agents/senior_ux_designer.toml` | Callable UX role. |
| `senior_workflow_architect` | `.codex/agents/senior_workflow_architect.toml` | Callable workflow planning role. |
| `source_analysis_reviewer` | `.codex/agents/source_analysis_reviewer.toml` | Callable source-analysis reviewer. |
| `swarm_orchestrator` | `.codex/agents/swarm_orchestrator.toml` | Coordinates read-only findings into slice plans. |

## Handoff to CG-03.02

CG-03.02 must consume this inventory without adding candidates from memory or
from runtime retrieval. For every row in the role and project-skill tables it
must assign exactly one classification, define a canonical ID, decide whether
role terminology maps to an AgentDefinition, document generic versus
`tiny-swarm-world` ownership, and record duplicate/deprecation decisions.

The following constraints are already established by the inventory and are
not migration decisions:

1. Workflow/process semantics go to CG-05.
2. Concrete Docker Swarm, LXD/Incus, Portainer, Linux/WSL and TSW repository
   conventions are project-specific signals.
3. Prompt/provider/runtime packaging is not a domain contract.
4. Named tools/capabilities are references only; no permission is inferred.
5. Retired Java/Maven/Spring guards are not reusable product assets.

## Verification evidence

The inventory was checked against the source Git tree using:

```text
git -C D:/Projects/Tiny-Swarm-World rev-parse HEAD
git -C D:/Projects/Tiny-Swarm-World ls-files '.agents/roles/**'
git -C D:/Projects/Tiny-Swarm-World ls-files '.agents/skills/**/SKILL.md'
git -C D:/Projects/Tiny-Swarm-World ls-files '.codex/agents/*.toml'
git -C D:/Projects/Tiny-Swarm-World ls-files '.codex/subagents/*.md'
git -C D:/Projects/Tiny-Swarm-World ls-files '.codex/skills/**/SKILL.md'
```

The source repository's own governance evidence independently reports 132
discoverable project skills and 6 portable Codex skills in
`.tiny-swarm/evidence/workflow-skill-agent-governance-20260720/`. No parser or
runtime behavior was introduced in Cognitive Gateway, so no new automated
inventory/parser test is required for this documentation-only slice.
