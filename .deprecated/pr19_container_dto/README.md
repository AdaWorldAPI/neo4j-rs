# Deprecated: PR #19 — ContainerDto + LadybugBackend (DTO paradigm)

**Date**: 2026-02-17
**PR**: #19 (feat: implement LadybugBackend — full StorageBackend over 8192-bit containers)

## Why deprecated

1. **ContainerDto duplicates ladybug_contract::Container.** 333 lines reimplementing
   xor(), hamming(), similarity(), random() — all of which already exist in ladybug-contract.

2. **Translation layer is CISC.** 9 layers between Cypher query and BindSpace operation.
   The RISC approach: Cypher parser → direct BindSpace calls. No intermediate executor,
   no StorageBackend trait dispatch, no NodeId ↔ Addr BiMap.

3. **PropertyMap side-HashMap is the old paradigm.** Properties should live in the DN tree
   as Container values at path positions, not in a parallel HashMap.

## What to salvage

- The `LadybugBackend` struct design in mod.rs has the right CALL procedures surface
  (ladybug.search, ladybug.bind, ladybug.similarity, ladybug.truth, ladybug.revise)
- The verb resolution via Surface 0x07 is correct
- procedures.rs has clean NARS and causal reasoning interfaces

## The RISC replacement

neo4j-rs becomes a Cypher parser that emits BindSpace operations:
- MATCH → space.traverse(dn, verb, depth)
- WHERE → Hamming filter / NARS truth filter
- RETURN → &Container references (zero copy)
- CREATE → space.write() (&mut borrow)

## Files

- `ladybug_module/` — the complete src/storage/ladybug/ directory
- `fingerprint.rs` — the standalone ContainerDto copy
