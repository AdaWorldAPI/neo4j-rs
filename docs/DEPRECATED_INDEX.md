# Deprecated Documents Index

> These documents describe the **pre-SPOQ architecture** where neo4j-rs had its own
> StorageBackend trait (36 methods), execution engine (1,171 LOC), planner (436 LOC),
> and model types (554 LOC).
>
> The SPOQ Integration Plan v2 supersedes this architecture:
> neo4j-rs becomes a ~2,100 LOC Cypher compiler that emits direct BindSpace operations.
>
> **Current architecture**: See `SPOQ_AUDIT.md`

## Superseded Documents

| Document | Reason Superseded |
|----------|-------------------|
| `DEVELOPMENT.md` | 46 refs to StorageBackend — describes deleted architecture |
| `ROADMAP_REVIEW.md` | 69 refs to StorageBackend — 87KB of old review |
| `STRATEGY_INTEGRATION_PLAN.md` | Old strategy: neo4j-rs as database layer |
| `REALITY_CHECK.md` | Audit of pre-SPOQ code |
| `INTEGRATION_ROADMAP.md` | Old integration roadmap |
| `INTEGRATION_PLAN_SCHEMA_CHANGES.md` | Schema changes for old architecture |

## Still Valid Documents

| Document | Why Still Valid |
|----------|----------------|
| `CAM_CYPHER_REFERENCE.md` | Cypher→CAM translation — core of new architecture |
| `FEATURE_MATRIX.md` | Feature overview (update scope) |
| `THEORETICAL_FOUNDATIONS.md` | Scientific references — timeless |
| `COMPATIBILITY_REPORT.md` | crewai-rust compatibility (still relevant) |
| `CHESS_HARVEST_PLAN.md` | Orthogonal to architecture |
| `GUI_PLAN.md` | Orthogonal |
| `INSPIRATION.md` | Orthogonal |
| `SPOQ_AUDIT.md` | Current — the audit of the new plan |

## PR #21 Status

PR #21 (SPO holographic traces, Belichtungsmesser cascade) should be **closed without merge**.
Both `permute()`/`unpermute()` and `belichtungsmesser()` already exist in ladybug-rs.
The SPO trace math in PR #21 is correct but operates on ContainerDto (deprecated).
See SPOQ Audit §2 D6/D7 for details.
