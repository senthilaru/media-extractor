# Data Model: Resolution-Aware Quality Ranking

## Value Objects & Structs

### `MediaQualityRank`
Represents the evaluated quality dimensions of a candidate media document.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaQualityRank {
    /// Total pixels (width * height), or 0 if unextractable / non-visual
    pub total_pixels: u64,
    /// Physical file size in bytes
    pub file_size_bytes: u64,
    /// Quality tier based on media category (e.g. RawPhoto > Photo > Video > Sidecar)
    pub format_tier: u8,
    /// True if standard EXIF temporal capture date exists
    pub has_exif_date: bool,
    /// True if GPS geolocation coordinates are present
    pub has_gps: bool,
    /// Drive group priority (Master > Replica > Staging > Unknown)
    pub drive_priority: u32,
    /// Whether file is within a canonical master directory structure
    pub is_in_master: bool,
}
```

### Ordering Semantics (`Ord` / `PartialOrd`)
The ordering of `MediaQualityRank` implements the exact specification rules:

1. `total_pixels` descending (highest resolution first).
2. `file_size_bytes` descending (larger file / less compressed first).
3. `format_tier` descending (`RawPhoto` > `Photo` > `Video`).
4. `has_exif_date` & `has_gps` descending (metadata completeness).
5. `drive_priority` descending.
6. `is_in_master` descending.

---

## State Transitions & Status Updates

| Current State | Condition | New State | Reclaimable Bytes |
|---|---|---|---|
| Non-Master Candidate | Highest `MediaQualityRank` in cluster (no master exists) | `NeedsIngestToMaster` | 0 |
| Non-Master Candidate | Lower `MediaQualityRank` than cluster keeper | `MarkForRemoval` | `file_size_bytes` |
| Primary Master Candidate | Highest `MediaQualityRank` in cluster | `CanonicalMaster` | 0 |
| Primary Master Candidate | Lower `MediaQualityRank` than non-master candidate | `CanonicalMaster` (Flagged for Replacement / Manifest) | 0 (Manual deletion manifest) |
| Non-Master Candidate | Superior to existing Primary Master copy | `NeedsIngestToMaster` (Upgrade) | 0 |
| Replica Candidate | In replica master path with verified primary master copy | `ReplicaKeep` | 0 |
