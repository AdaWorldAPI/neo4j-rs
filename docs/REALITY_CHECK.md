# Reality Check — neo4j-rs

> Brutal, honest audit of every bug, gap, lie, and unsound assumption in the
> codebase. Plus the exact fix for each one. Written 2026-02-13.
>
> If this document makes you uncomfortable, good. That's the point.

---

## Verdict

**neo4j-rs cannot execute a single Cypher query.**

Not `MATCH (n) RETURN n`. Not `CREATE (n)`. Not anything. The parser,
planner, and execution engine are all stubs that return errors. The storage
trait is well-designed. The model types are clean. The documentation is
beautiful. But zero queries run end-to-end.

The codebase is a well-organized skeleton with 15 passing tests that
exercise raw StorageBackend methods — never through Cypher.

---

## Table of Contents

1. [Showstoppers: Nothing Runs](#1-showstoppers-nothing-runs)
2. [Soundness Bugs: Things That Are Wrong](#2-soundness-bugs-things-that-are-wrong)
3. [Model Gaps: Types That Lie](#3-model-gaps-types-that-lie)
4. [Parser Gaps: Words We Can't Read](#4-parser-gaps-words-we-cant-read)
5. [Planner Gaps: Plans We Can't Make](#5-planner-gaps-plans-we-cant-make)
6. [Execution Gaps: Code That Doesn't Exist](#6-execution-gaps-code-that-doesnt-exist)
7. [Storage Gaps: Questions We Can't Answer](#7-storage-gaps-questions-we-cant-answer)
8. [Documentation Lies](#8-documentation-lies)
9. [The Fix: Exact Steps to Holy Grail](#9-the-fix-exact-steps-to-holy-grail)
10. [Scoreboard](#10-scoreboard)

---

## 1. Showstoppers: Nothing Runs

These five issues mean the entire pipeline is broken. Until ALL five are
fixed, neo4j-rs is a library that compiles but does nothing.

### S-1. Parser Returns Error (CRITICAL)

**File:** `src/cypher/parser.rs:3-7`

```rust
pub fn parse_statement(_tokens: &[Token]) -> Result<Statement> {
    Err(Error::SyntaxError {
        position: 0,
        message: "Parser not yet implemented".into(),
    })
}
```

**Impact:** `Graph::execute("MATCH (n) RETURN n")` fails immediately.
Every Cypher string hits this wall.

**Fix:** Implement recursive descent parser. ~800-1200 lines. Start with:
1. Statement dispatch (MATCH → parse_query, CREATE → parse_create, etc.)
2. Pattern parsing: `(n:Label {key: val})-[r:TYPE]->(m)`
3. Expression parser with operator precedence
4. WHERE, RETURN, ORDER BY, LIMIT, SKIP clauses

### S-2. Planner Returns Error (CRITICAL)

**File:** `src/planner/mod.rs:38-41`

```rust
pub fn plan(_ast: &Statement, _params: &PropertyMap) -> Result<LogicalPlan> {
    Err(Error::PlanError("Planner not yet implemented".into()))
}
```

**Impact:** Even if the parser worked, planning fails. No AST ever becomes
a LogicalPlan.

**Fix:** Implement AST-to-LogicalPlan conversion. ~400-600 lines:
- `Query` with `MATCH (n:L)` → `NodeScan { label, alias }`
- Relationships → `Expand`
- `WHERE expr` → `Filter { predicate }`
- `RETURN items` → `Project { items }`
- `CREATE` → `CreateNode` / `CreateRel`
- Variable binding context to catch `RETURN undefined_var`

### S-3. Execution Engine Returns Error (CRITICAL)

**File:** `src/execution/mod.rs:87-94`

```rust
pub async fn execute<B: StorageBackend>(
    _backend: &B,
    _tx: &B::Tx,
    _plan: LogicalPlan,
) -> Result<QueryResult> {
    Err(Error::ExecutionError("Execution engine not yet implemented".into()))
}
```

**Impact:** Even if parser and planner worked, execution fails. No
LogicalPlan ever produces results.

**Fix:** Implement Volcano-style pull executor. ~600-1000 lines:
- Pattern match on LogicalPlan variant
- `NodeScan` → `backend.nodes_by_label()`
- `Filter` → evaluate predicate per row
- `Project` → extract requested columns
- `CreateNode` → `backend.create_node()`
- Expression evaluator: `fn eval_expr(expr, row, params) -> Value`

### S-4. execute() Signature Has Type Mismatch (CRITICAL)

**File:** `src/execution/mod.rs:87-88` vs `src/lib.rs:114-115,130-131`

```rust
// execute() takes IMMUTABLE tx reference:
pub async fn execute<B: StorageBackend>(
    _backend: &B,
    _tx: &B::Tx,        // <-- immutable
    _plan: LogicalPlan,
) -> Result<QueryResult>

// But StorageBackend write methods need MUTABLE:
async fn create_node(&self, tx: &mut Self::Tx, ...) -> Result<NodeId>;
```

And in `lib.rs`:
```rust
// Graph::execute passes &tx (immutable) — OK for reads
let result = execution::execute(&self.backend, &tx, optimized).await?;

// Graph::mutate passes &mut tx — but execute() signature doesn't accept it!
let result = execution::execute(&self.backend, &mut tx, optimized).await?;
```

**Impact:** The moment someone implements execute() and tries to call
`backend.create_node()`, it won't compile. `&B::Tx` cannot become
`&mut B::Tx`.

**Fix:** Change execute() signature to take `&mut B::Tx`. Read-only
operations can accept `&mut` without issue (Rust allows borrowing `&mut`
as `&` implicitly at call sites). Update `Graph::execute()` to pass
`&mut tx` as well.

### S-5. ExplicitTx Has No Drop — Leaked Transactions (HIGH)

**File:** `src/lib.rs:157-180`

```rust
pub struct ExplicitTx<'g, B: StorageBackend> {
    graph: &'g Graph<B>,
    tx: B::Tx,
}
// No Drop impl. When this struct goes out of scope, the transaction
// just vanishes. No rollback. No cleanup. No error.
```

**Impact:**
```rust
{
    let tx = graph.begin(TxMode::ReadWrite).await?;
    tx.execute("CREATE (n:Person)").await?;
    // tx drops here — node was created but transaction never committed
    // MemoryBackend: doesn't care (no real tx)
    // BoltBackend: connection leaked, Neo4j session stays open
    // LadybugBackend: Lance write handle leaked
}
```

**Fix:** Add a `committed` flag and log/warn on drop without commit. We
can't call async rollback from Drop, but we can:
1. Track `committed: bool` flag
2. On drop, if !committed, log a warning via `tracing::warn!`
3. Document that explicit rollback is required
4. For Bolt, the connection pool will eventually reclaim the connection

---

## 2. Soundness Bugs: Things That Are Wrong

These are bugs in code that exists and "works" — but produces incorrect
results or has undefined behavior under concurrency.

### B-1. MemoryBackend Transactions Are Fake (HIGH)

**File:** `src/storage/memory.rs:92-93`

```rust
async fn commit_tx(&self, _tx: MemoryTx) -> Result<()> { Ok(()) }
async fn rollback_tx(&self, _tx: MemoryTx) -> Result<()> { Ok(()) }
```

Both are no-ops. Writes are applied immediately on each StorageBackend
call. "Rollback" does nothing — the data is already mutated.

**Impact:** Any test that creates data then rolls back will see stale
data. The "test oracle" is lying about ACID semantics.

**Fix (phased):**
- **Phase 1 (now):** Document in MemoryTx that rollback is a no-op.
  Add `#[doc = "WARNING: rollback is a no-op"]` to rollback_tx.
- **Phase 2 (later):** Implement write-ahead log: MemoryTx records
  all mutations. `commit_tx` applies them. `rollback_tx` discards them.
  This requires MemoryTx to hold a `Vec<Mutation>` and `commit_tx` to
  replay them against the inner state.

### B-2. No MVCC — Concurrent Access Is Unsound (HIGH)

**File:** `src/storage/memory.rs` (entire file)

The backend uses `parking_lot::RwLock` per collection (nodes, rels,
adjacency, label_index). Each method acquires/releases locks independently.
A multi-step operation like `delete_node` does:

1. Acquire `adjacency.read()` → check for rels → release
2. Acquire `nodes.write()` → remove node → release
3. Acquire `adjacency.write()` → remove entry → release
4. Acquire `label_index.write()` → clean up → release

Between steps 1 and 2, another thread can add a relationship. The node
gets deleted despite now having relationships.

**Impact:** Data corruption under concurrent writes. The MemoryBackend
is only safe for single-threaded use.

**Fix (phased):**
- **Phase 1 (now):** Document `MemoryBackend` as single-writer.
- **Phase 2 (later):** Use a single `RwLock<MemoryState>` instead of
  per-collection locks. Acquire write lock once per mutation, read lock
  once per query. Coarser but correct.
- **Phase 3 (future):** MVCC with snapshot isolation. Each transaction
  sees a consistent snapshot. Writes buffer until commit.

### B-3. delete_node Leaves Stale Adjacency Entries (MEDIUM)

**File:** `src/storage/memory.rs:130-156`

When node B is deleted, `adjacency.remove(&B)` removes B's own entry.
But if node A has A→B in its adjacency list, A's entry still contains
the RelId pointing to B. The relationship itself is NOT in the
`relationships` map anymore (it was checked and blocked), but the
adjacency list has a stale pointer.

**Impact:** Memory leak. `get_relationships(A)` will try to look up the
stale RelId and get None — not a crash, but unnecessary work.

**Fix:** In `delete_node`, after removing the node, iterate its former
adjacency list (already captured before removal) and clean up peer
adjacency entries. Actually, `delete_node` already checks `rels.is_empty()`
and returns error if not — so this only matters for `detach_delete_node`.
The default impl handles it correctly by deleting rels first.

### B-4. property_index Is Dead Code (MEDIUM)

**File:** `src/storage/memory.rs:39`

```rust
property_index: RwLock<HashMap<(String, String), HashMap<String, Vec<NodeId>>>>,
```

Allocated in `new()`. Never read. Never written. `nodes_by_property()`
does a brute-force scan instead. `set_node_property()` doesn't update it.

**Impact:** Wasted memory. False impression that property indexes work.
Compiler warning about unused field.

**Fix:** Remove it entirely. Add it back when implementing real indexes.

### B-5. create_index() Is a Silent Lie (MEDIUM)

**File:** `src/storage/memory.rs:400-403`

```rust
async fn create_index(&self, _label: &str, _property: &str, _index_type: IndexType) -> Result<()> {
    // Memory backend always has implicit indexes
    Ok(())
}
```

The comment says "always has implicit indexes" — this is false. There
are no indexes. `nodes_by_property()` does a full scan every time.
`create_index()` silently succeeds and changes nothing.

**Impact:** A planner that trusts index existence will generate IndexLookup
plans that are no faster than full scans. Performance lies.

**Fix:** Either:
1. Maintain a `HashSet<(String, String)>` of "created" indexes and
   use it to choose between scan and lookup (even if both do the same
   thing in memory, at least the bookkeeping is honest).
2. Or change the comment to: `// No-op: memory backend always full-scans`.

### B-6. detach_delete_node Default Impl Has Race Window (LOW)

**File:** `src/storage/mod.rs` (trait default)

```rust
async fn detach_delete_node(&self, tx: &mut Self::Tx, id: NodeId) -> Result<bool> {
    let rels = self.get_relationships(tx, id, Direction::Both, None).await?;
    for rel in &rels {
        self.delete_relationship(tx, rel.id).await?;
    }
    self.delete_node(tx, id).await
}
```

Between `get_relationships()` and `delete_relationship()`, another
transaction could add a new relationship. The new relationship won't be
deleted, and `delete_node` will fail with constraint violation.

**Impact:** Low for MemoryBackend (no real concurrency). High for
backends with real concurrency. Each backend should override with an
atomic implementation.

**Fix:** Document that backends with real concurrency MUST override
this default. MemoryBackend's override would use a single write lock.

---

## 3. Model Gaps: Types That Lie

### M-1. No element_id on Node/Relationship (HIGH)

**Files:** `src/model/node.rs`, `src/model/relationship.rs`

Neo4j 5.x returns two IDs for every entity:
- `id` (integer, internal, may be reused after deletion)
- `elementId` (string, stable, globally unique like `"4:abc:123"`)

Our Node has only `NodeId(u64)`. The Bolt protocol returns element_id as
the primary identifier. When we implement BoltBackend, we'll get string
IDs from Neo4j with no field to put them in.

**Fix:** Add `element_id: Option<String>` to both Node and Relationship.
`None` for MemoryBackend (generates its own). `Some(...)` for Bolt.

### M-2. Value Enum Has 18 Variants, Not 17 (LOW)

**File:** `src/model/value.rs`

DEVELOPMENT.md and CLAUDE.md both say "17 variants." Actual count: 18.
(Date, Time, DateTime, LocalDateTime, Duration = 5 temporal; Point2D,
Point3D = 2 spatial; Null, Bool, Int, Float, String, Bytes, List, Map,
Node, Relationship, Path = 11. Total: 18.)

**Fix:** Update docs to say 18.

### M-3. ResultRow Uses HashMap — Column Order Lost (HIGH)

**File:** `src/execution/mod.rs:22-24`

```rust
pub struct ResultRow {
    pub values: HashMap<String, Value>,
}
```

Neo4j guarantees `RETURN b, a` returns columns in order (b, a). HashMap
has no ordering. A user iterating `row.values` gets arbitrary order.

**Fix:** Change to `Vec<(String, Value)>` or use `IndexMap` from the
`indexmap` crate. Or use `Vec<Value>` and reference columns from
QueryResult.columns by index.

### M-4. FromValue Only Covers 3 of 18 Types (MEDIUM)

**File:** `src/execution/mod.rs:49-84`

Implemented: `Node`, `String`, `i64`.

Missing: `bool`, `f64`, `Vec<Value>`, `Relationship`, `Path`,
`HashMap<String, Value>`, `Vec<u8>`, all temporal types, all spatial types.

Users can't do `row.get::<bool>("active")` or `row.get::<f64>("score")`.

**Fix:** Add impl blocks for all remaining types. Each is 5-10 lines:
```rust
impl FromValue for bool {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::Bool(b) => Ok(*b),
            _ => Err(Error::TypeError { expected: "Bool".into(), got: val.type_name().into() }),
        }
    }
}
```

### M-5. Path Lacks Standard Operations (LOW)

**File:** `src/model/path.rs`

Missing: iterator methods, reverse, contains_node, node_at(index),
relationship_at(index).

**Fix:** Add convenience methods. Low priority — users can access
`.nodes` and `.relationships` directly.

### M-6. Missing From Impls for Value (LOW)

**File:** `src/model/value.rs`

No `From<Vec<u8>>` for `Bytes`, no `From<NaiveDate>` for `Date`, no
`From<HashMap<String, Value>>` for `Map`.

**Fix:** Add ~10 more From impls. Each is 3 lines.

---

## 4. Parser Gaps: Words We Can't Read

### P-1. Parser Is 100% Unimplemented (CRITICAL)

See S-1 above. The parser is a stub that returns an error.

### P-2. Missing CALL/YIELD Keywords (HIGH)

**File:** `src/cypher/lexer.rs`

The lexer has no `CALL` or `YIELD` tokens. The AST has no
`CallClause` or `YieldClause`. Without these, `CALL ladybug.similar()`
cannot be parsed — the entire procedure extension surface is blocked.

**Fix:** Add `Call` and `Yield` to `TokenKind`. Add keyword entries
in the `tokenize()` keyword match. Add `CallClause` to the AST.

### P-3. Missing UNION / FOREACH / UNWIND in AST (MEDIUM)

**File:** `src/cypher/ast.rs`

- `UNION` / `UNION ALL` — no Statement variant for combining queries
- `FOREACH` — no clause for imperative loops
- `UNWIND` — token exists but no AST clause

**Fix:** Add `Union { all: bool, left: Box<Statement>, right: Box<Statement> }`
to Statement. Add `UnwindClause { expr: Expr, alias: String }` to Query.
Add `ForeachClause { variable: String, list: Expr, body: Vec<Statement> }`.

### P-4. No Block Comments (LOW)

**File:** `src/cypher/lexer.rs`

Only `//` line comments are supported. `/* */` block comments are not.
openCypher supports both.

**Fix:** Add block comment handling in the tokenizer's whitespace/comment
skip loop. ~15 lines.

### P-5. Parameter Span Off By One (LOW)

**File:** `src/cypher/lexer.rs` (parameter tokenization)

`end: start + name.len()` should be `end: start + name.len() + 1` to
account for the `$` prefix character.

**Fix:** One-line fix.

---

## 5. Planner Gaps: Plans We Can't Make

### PL-1. Planner Is 100% Unimplemented (CRITICAL)

See S-2 above. The plan() function is a stub.

### PL-2. Missing ~20 LogicalPlan Operators (HIGH)

**File:** `src/planner/mod.rs:12-37`

Current operators: NodeScan, IndexLookup, Expand, Filter, Project,
CreateNode, CreateRel, Limit, Sort, CartesianProduct, CallProcedure,
Argument. (12 total)

Missing operators needed for basic Cypher:

| Operator | Blocks | Example Query |
|----------|--------|---------------|
| **Aggregate** | `count()`, `sum()`, `avg()` | `RETURN count(n)` |
| **Distinct** | `DISTINCT` keyword | `RETURN DISTINCT n.label` |
| **Skip** | `SKIP 10` | Pagination |
| **Optional** | `OPTIONAL MATCH` | Left-outer-join |
| **Unwind** | `UNWIND list AS x` | List expansion |
| **With** | `WITH n WHERE ...` | Pipeline stages |
| **MergeNode** | `MERGE (n:L {k:v})` | Upsert |
| **DeleteNode** | `DELETE n` | Node deletion |
| **DeleteRel** | `DELETE r` | Relationship deletion |
| **DetachDelete** | `DETACH DELETE n` | Cascade deletion |
| **SetProperty** | `SET n.x = 1` | Property update |
| **SetLabel** | `SET n:Label` | Label add |
| **RemoveProperty** | `REMOVE n.x` | Property removal |
| **RemoveLabel** | `REMOVE n:Label` | Label removal |
| **Union** | `UNION` / `UNION ALL` | Set operations |
| **ProduceResults** | Final output | Column ordering + materialization |
| **AllNodesScan** | `MATCH (n)` with no label | Full scan fallback |

**Fix:** Add all variants to the LogicalPlan enum. Each is one line of
enum definition. The execution engine implements the actual logic.

### PL-3. No Variable Binding Context (MEDIUM)

The planner has no way to track which variables are bound at each stage.
`MATCH (n) RETURN m` should be a semantic error (m undefined), but the
planner can't detect it without a binding context.

**Fix:** Add `BindingContext { bindings: HashMap<String, BindingType> }`
that the planner maintains while walking the AST.

### PL-4. No Index Selection Logic (MEDIUM)

The optimize() function is a pass-through. Even if indexes existed, the
planner would never use them.

**Fix:** In optimize(), check if Filter predicates match known indexes.
If `WHERE n.email = $e` and an index exists on (Person, email), rewrite
`NodeScan + Filter` to `IndexLookup`.

---

## 6. Execution Gaps: Code That Doesn't Exist

### E-1. Execution Engine Is 100% Unimplemented (CRITICAL)

See S-3 above.

### E-2. No Expression Evaluator (CRITICAL)

The execution engine needs `fn eval_expr(expr, row, params) -> Value` to
evaluate WHERE clauses, RETURN expressions, function calls, etc. This
function does not exist anywhere in the codebase.

**Required capabilities:**
- Literal evaluation: `42` → `Value::Int(42)`
- Variable lookup: `n` → row["n"]
- Property access: `n.name` → row["n"].properties["name"]
- Parameter substitution: `$param` → params["param"]
- Binary operations: `a + b`, `a > b`, `a AND b`
- Function calls: `count()`, `labels()`, `id()`, `type()`
- NULL propagation: `NULL + 1 = NULL`, `NULL = NULL → NULL`
- Type coercion: `1 + 1.0 = 2.0` (int promotes to float)

**Fix:** Implement `eval_expr()` in `src/execution/mod.rs`. ~200-400
lines. This is the core of the execution engine.

### E-3. No Aggregation Framework (HIGH)

Even with eval_expr, there's no way to compute `count(n)` or `sum(n.age)`.
Aggregation requires:
1. Grouping rows by group-by keys
2. Accumulating aggregation state per group
3. Producing one output row per group

**Fix:** Add `AggregateAccumulator` enum (Count, Sum, Avg, Min, Max,
Collect) and `aggregate_rows()` function. ~150 lines.

---

## 7. Storage Gaps: Questions We Can't Answer

### ST-1. No Range Scan (HIGH)

**Query:** `MATCH (n:Person) WHERE n.age > 30 RETURN n`

StorageBackend has `nodes_by_property(label, key, value)` — exact match
only. No `nodes_by_property_range(label, key, min, max)`.

The execution engine would have to: fetch ALL Person nodes, then filter
in memory. For 1M nodes, this is catastrophic.

**Fix:** Add `nodes_by_property_range()` to the trait (already in the
DEVELOPMENT.md gap tracker as SHOULD HAVE). MemoryBackend implements
with brute-force filter. Future backends can use B-tree indexes.

### ST-2. No Aggregation Push-Down (MEDIUM)

`RETURN count(n)` must fetch all nodes into the execution engine, then
count them. StorageBackend has `node_count()`, but the planner has no
way to recognize that `count(*)` maps to `node_count()`.

**Fix:** Long-term: planner recognizes simple count queries and rewrites
them to use `node_count()` directly. Short-term: aggregation in the
execution engine is sufficient.

### ST-3. Batch Type Inconsistency (LOW)

`create_node()` takes `labels: &[&str]`.
`create_nodes_batch()` takes `Vec<(Vec<String>, PropertyMap)>`.

The batch version requires owned Strings while the single version takes
references. This forces unnecessary allocations in the default impl.

**Fix:** Accept both, or standardize on one form. The default impl
already converts with `labels.iter().map(|s| s.as_str()).collect()`.
This is fine — batch callers typically have owned data anyway.

---

## 8. Documentation Lies

| # | Claim | Reality | File |
|---|-------|---------|------|
| D-1 | "17-variant Value enum" | 18 variants | CLAUDE.md, DEVELOPMENT.md |
| D-2 | "Memory backend always has implicit indexes" | No indexes. Full scan every time. | memory.rs:401 comment |
| D-3 | ARCHITECTURE.md documents `connect()` and `execute_raw()` in trait | `connect()` is per-backend (by design). `execute_raw()` now added. | ARCHITECTURE.md |
| D-4 | CLAUDE.md says `parking_lot` for sync | Wasn't in Cargo.toml until this session | Cargo.toml (now fixed) |
| D-5 | "StorageBackend trait: Complete" in status table | Complete for CRUD, but missing range scans, aggregation push-down | DEVELOPMENT.md |
| D-6 | "Complete, tested" for MemoryBackend | No transaction tests. No concurrent access tests. No rollback tests. | DEVELOPMENT.md |
| D-7 | "ACID transactions" in contract summary | MemoryBackend has no A (atomicity), no I (isolation), no D (durability). Only C. | DEVELOPMENT.md 14.13 |

**Fix:** Update all documentation. Be honest about what works and what
doesn't. Mark MemoryBackend as "single-writer, no rollback, no isolation."

---

## 9. The Fix: Exact Steps to Holy Grail

This is the roadmap that takes neo4j-rs from "compiles but does nothing"
to "runs real Cypher queries against multiple backends."

### Step 0: Fix the Type Mismatch (30 min)

Change `execute()` in `src/execution/mod.rs` to take `&mut B::Tx`.
Update `Graph::execute()` in `src/lib.rs` to pass `&mut tx`.
This unblocks ALL future work.

```rust
// execution/mod.rs
pub async fn execute<B: StorageBackend>(
    backend: &B,
    tx: &mut B::Tx,    // was &B::Tx
    plan: LogicalPlan,
) -> Result<QueryResult>
```

### Step 1: Parser — Make Cypher Readable (3-5 days)

**Goal:** `parse("MATCH (n:Person) RETURN n")` produces an AST.

Priority order:
1. Statement dispatch (MATCH/CREATE/DELETE/SET routing)
2. Node pattern: `(n:Label {key: val})`
3. Relationship pattern: `-[r:TYPE]->`
4. Expression parser with precedence (AND/OR > compare > arithmetic)
5. WHERE clause
6. RETURN clause with aliases
7. CREATE pattern
8. ORDER BY, LIMIT, SKIP
9. SET/DELETE/REMOVE
10. CALL...YIELD (add CALL/YIELD tokens first)

**Test:** Parse the 8 Cypher forms from DEVELOPMENT.md Section 15.

### Step 2: Planner — Make ASTs Executable (2-3 days)

**Goal:** `plan(ast)` produces a LogicalPlan tree.

Priority order:
1. Add missing operators: Aggregate, Distinct, AllNodesScan, DeleteNode,
   SetProperty, ProduceResults
2. plan_query(): MATCH → NodeScan/Expand, WHERE → Filter, RETURN → Project
3. plan_create(): patterns → CreateNode/CreateRel
4. plan_delete(): DETACH DELETE → DetachDelete operator
5. plan_set(): SET items → SetProperty operator
6. Variable binding context (catch undefined variables)
7. CALL → CallProcedure

**Test:** Plan the same 8 Cypher forms.

### Step 3: Expression Evaluator (2-3 days)

**Goal:** `eval_expr(expr, row, params)` returns a Value.

This is the heart of execution. Needed before ANY operator can run.

Priority order:
1. Literals, variables, parameters
2. Property access: `n.name`
3. Binary ops: `+`, `-`, `*`, `/`, `=`, `<>`, `<`, `>`, `AND`, `OR`
4. Unary ops: `NOT`, `-`
5. NULL propagation (three-valued logic)
6. Function calls: `id()`, `labels()`, `type()`, `count()`
7. IS NULL / IS NOT NULL
8. String ops: STARTS WITH, ENDS WITH, CONTAINS
9. IN operator
10. CASE/WHEN/THEN/ELSE

**Test:** Expression evaluation for each operator against known values.

### Step 4: Execution Engine — Run the Plans (3-5 days)

**Goal:** `execute(backend, tx, plan)` returns QueryResult rows.

Priority order:
1. NodeScan → `backend.nodes_by_label()`
2. AllNodesScan → `backend.all_nodes()`
3. Filter → `eval_expr(predicate)` per row
4. Project → extract columns
5. CreateNode → `backend.create_node()`
6. CreateRel → `backend.create_relationship()`
7. Expand → `backend.get_relationships()` + `backend.get_node()`
8. Limit → take first N rows
9. Sort → sort rows by key expression
10. Aggregate → group + accumulate
11. CallProcedure → `backend.call_procedure()`

**Test:** End-to-end: `MATCH (n:Person) RETURN n.name` against MemoryBackend.

### Step 5: Fix the Model (1 day)

1. Add `element_id: Option<String>` to Node and Relationship
2. Change ResultRow to preserve column order (Vec or IndexMap)
3. Add remaining FromValue implementations
4. Remove dead property_index from MemoryBackend
5. Fix documentation lies (18 variants, no ACID, etc.)

### Step 6: End-to-End Integration Tests (1-2 days)

Create `tests/` directory with:
```
tests/
├── e2e_basic.rs        — MATCH/CREATE/RETURN against MemoryBackend
├── e2e_traversal.rs    — Multi-hop paths, variable-length
├── e2e_write.rs        — CREATE, SET, DELETE, DETACH DELETE
├── e2e_aggregation.rs  — COUNT, SUM, DISTINCT
├── e2e_procedures.rs   — CALL with mock procedure
└── e2e_edge_cases.rs   — NULL handling, empty results, type coercion
```

### Step 7: MemoryBackend Transaction Honesty (1 day)

1. Add write-ahead log to MemoryTx
2. `commit_tx()` replays the log
3. `rollback_tx()` discards the log
4. Add `committed` flag + warning in ExplicitTx drop

### Step 8: Bolt Protocol Client (1-2 weeks)

Borrow from neo4rs:
1. PackStream serde (binary format)
2. Bolt handshake + authentication
3. Message catalog (HELLO, BEGIN, RUN, PULL, COMMIT)
4. Connection pool (deadpool)
5. impl StorageBackend for BoltBackend

### Step 9: ladybug-rs Integration (1-2 weeks)

Build `impl StorageBackend for LadybugBackend` in ladybug-rs:
1. Node CRUD → Lance + BindSpace
2. Relationship CRUD → Lance adjacency
3. expand() → Lance adjacency (EXACT mode)
4. call_procedure() → route to cognitive modules
5. vector_query() → CAKES/HDR cascade

### Step 10: openCypher TCK (ongoing)

Run the Technology Compatibility Kit from the reference Neo4j repo.
Fix every failing test. This is the definitive proof of correctness.

---

## 10. Scoreboard

### What's Actually Done

| Component | Lines | Tests | Grade |
|-----------|-------|-------|-------|
| Value enum (18 variants) | 233 | 3 | **A-** |
| Node / Relationship / Path DTOs | 150 | 0 | **B+** |
| PropertyMap | 19 | 0 | **A** (it's a type alias, it's fine) |
| Lexer (56 token types) | 400 | 4 | **A-** |
| AST (6 statement types, 16 expr types) | 350 | 0 | **A** |
| StorageBackend trait (31 methods) | 460 | 0 | **A** |
| MemoryBackend | 612 | 8 | **B+** |
| Transaction types | 21 | 0 | **C** (no real semantics) |
| Error types | 30 | 0 | **A** |
| Graph<B> public API | 100 | 0 | **B** (type mismatch in execute) |
| LogicalPlan enum | 37 | 0 | **B-** (missing operators) |

### What's Not Done

| Component | Required Lines (est.) | Blocks |
|-----------|-----------------------|--------|
| **Parser** | 800-1200 | Everything |
| **Planner** | 400-600 | Execution |
| **Expression evaluator** | 200-400 | Execution |
| **Execution engine** | 600-1000 | End-to-end |
| **Aggregation framework** | 150 | COUNT/SUM/AVG |
| **Integration tests** | 500+ | Confidence |
| **Bolt PackStream** | 500-800 | Bolt backend |
| **Bolt protocol** | 400-600 | External Neo4j |
| **LadybugBackend** | 800-1200 | ladybug-rs integration |

### Estimated Total Work to "Holy Grail"

```
Done:           ~2,400 lines, 15 tests
Remaining:      ~4,000-6,500 lines, ~100+ tests
Completion:     ~35-45% by line count
                ~10% by functionality (nothing runs)
```

### The Uncomfortable Truth

neo4j-rs has excellent architecture, excellent types, excellent
documentation, and zero functionality. It's a blueprint without a
building. The StorageBackend trait is well-designed. The model types
are clean. The module boundaries are correct. But none of it is
connected — the parser can't parse, the planner can't plan, and the
executor can't execute.

The good news: the hard architectural decisions are made and they're
correct. The remaining work is implementation — filling in the stubs
with real code. The types are right. The boundaries are right. The
trait is right. Now someone needs to write the actual algorithms.

Steps 0-4 (type fix + parser + planner + expression evaluator +
execution engine) represent ~2,000-3,400 lines of new code and would
take neo4j-rs from "nothing runs" to "basic Cypher queries work
end-to-end." That's the critical path. Everything else is incremental.

---

*"The first step toward fixing a problem is admitting it exists."*
