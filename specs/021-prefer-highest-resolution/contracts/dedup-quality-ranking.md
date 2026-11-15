# Interface Contract: Deduplication Quality Ranking

## Scope
Defines the functional contract for candidate evaluation in deduplication clusters and content groups.

### Function Signature
```rust
impl DedupEngine {
    /// Computes the quality rank of a document for deduplication keeper selection.
    pub fn compute_quality_rank(
        doc: &MediaMetadataDocument,
        drive_priority: u32,
        is_in_master: bool,
    ) -> MediaQualityRank;

    /// Evaluates a cluster or content group of duplicate documents, selecting the highest-resolution
    /// and highest-fidelity asset as the keeper, and designating lower-resolution/smaller copies
    /// for removal.
    pub fn evaluate_content_group(
        docs: &[MediaMetadataDocument],
        matcher: &CanonicalPathMatcher,
    ) -> Vec<(Uuid, DeduplicationMetadata)>;
}
```

### Invariants & Guarantees
1. **Resolution Dominance**: In any group where candidates differ in total pixels (`width * height`), the candidate with the greatest total pixels MUST be ranked higher than all lower-pixel candidates.
2. **Size Dominance on Resolution Ties**: When candidates have matching pixel dimensions, the candidate with the larger byte size MUST be ranked higher.
3. **Zero Data Loss**: Pruning eligibility (`MarkForRemoval`) is strictly limited to files that have an identified keeper asset.
4. **Master Archive Safety**: Files located within canonical primary master or replica paths MUST NOT be marked for automatic deletion; if an inferior master copy is superseded, it is emitted to a manual deletion manifest.
