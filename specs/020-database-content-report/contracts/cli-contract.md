# CLI Interface Contract: `report` Subcommand

**Feature**: `020-database-content-report`  
**Date**: 2026-11-14

## Command Invocation

```bash
cargo run -- report [OPTIONS] [SUBCOMMAND]
```

### Aliases
- `cargo run -- status`
- `cargo run -- audit`

---

## Global Options

| Flag | Short | Default | Description |
|---|---|---|---|
| `--format` | `-f` | `text` | Output format: `text` (human-readable table) or `json` (machine-readable) |
| `--inspect` | `-i` | None | Direct inspection category shortcut (`problems`, `missing-exif`, `duplicates`, `unreplicated`, `marked-for-delete`, `unmastered`) |
| `--limit` | `-n` | `3` | Maximum number of sample filepaths to display when inspecting (default: 3) |
| `--config` | `-c` | `drives.yaml` | Path to drive configuration YAML (for drive IDs and mounts) |

---

## Subcommands

### `report inspect <CATEGORY>`
Inspects top sample items for a specific issue or status.

```bash
cargo run -- report inspect <CATEGORY> [--limit <N>]
```

**Valid Categories**:
- `problems`: Assets with extraction failures or error diagnostics.
- `missing-exif`: Assets without camera/video capture timestamp (fallback to file modification time).
- `duplicates`: Assets clustered as duplicate copies.
- `unreplicated`: Canonical master assets pending replica backup.
- `marked-for-delete`: Assets flagged for safe pruning.
- `unmastered`: Assets needing ingestion into canonical master.

---

## Output Examples

### 1. Default Terminal Summary (`text`)

```text
================================================================================
                      DATABASE CONTENT & HEALTH REPORT                          
================================================================================
Generated At: 2026-11-14 14:35:00 UTC
Total Cataloged Files: 1,542

Format Breakdown:
  .jpg        850  (55.1%)
  .heic       412  (26.7%)
  .mp4        210  (13.6%)
  .mov         60  ( 3.9%)
  .png         10  ( 0.6%)

Catalog Lifecycle & Health Status:
  ✓ Copied / Ingested to Master : 1,200
  🛡 Replicated Backups Preserved : 1,050
     - replica_1                :   800  (   400 yet to replicate)
     - replica_2                :   250  (   950 yet to replicate)
  ⧗ Master Assets Pending Backup:   400
  ⧉ Duplicate Assets (To Prune) :   192
  ✕ Marked for Deletion / Prune :   192
  ? Lacking EXIF Metadata       :    45
  ! Problematic / Error Files   :     3
  → Unmastered / Scanned Assets :   120

--------------------------------------------------------------------------------
SUGGESTED NEXT ACTION: Synchronize Backup Replicas
  Reason : 150 canonical masters are awaiting replication to backup drives.
  Command: cargo run -- sync-replicas
================================================================================
```

### 2. Sample Inspection Output (`report inspect problems`)

```text
================================================================================
                 SAMPLE INSPECTION: Problematic / Error Files                   
================================================================================
Found 3 items (showing top 3):

[1] /Volumes/Drive1/Photos/corrupt_header.jpg (42,105 bytes)
    Error: Failed to parse JPEG header: unexpected EOF

[2] /Volumes/Drive1/Photos/unsupported_codec.mov (1,048,576 bytes)
    Error: Video stream decoding timed out

[3] /Volumes/Drive1/Photos/bad_exif.png (245,110 bytes)
    Error: EXIF IFD pointer points beyond file boundary

--------------------------------------------------------------------------------
Suggested Next Action: Review files manually or re-extract after fixing permissions
================================================================================
```
