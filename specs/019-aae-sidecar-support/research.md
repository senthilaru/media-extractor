# Research: Apple AAE Sidecar File Support

## Decision 1: Sidecar File Detection & Companion Association Strategy

### Decision
Implement companion sidecar detection based on filesystem stem matching and Apple iOS naming conventions, coupled with `MediaCategory::Sidecar` classification.

### Rationale
Apple iOS generates `.AAE` files when users apply edits (filters, crops, adjustments, slow-motion ranges, markups) in the iOS Photos app. These files are XML property lists (`plist`) sharing the base stem of the primary image or video:
- Standard: `IMG_1234.AAE` pairs with `IMG_1234.HEIC`, `IMG_1234.JPG`, or `IMG_1234.MOV`.
- iOS Edited export variants: `IMG_E1234.AAE` pairs with `IMG_E1234.JPG` or `IMG_1234.JPG`.
- Case-insensitivity: File systems on macOS and external drives may use `.AAE`, `.aae`, or `.Aae`. Case-insensitive extension and stem matching is required.

### Alternatives Considered
- *Full Plist Parsing*: Parsing the binary/XML plist structure to extract internal UUIDs. Rejected as overly complex and fragile; filesystem stem pairing is the universal standard used by macOS Photos, Lightroom, and exiftool.
- *Strict Case Matching*: Only looking for uppercase `.AAE`. Rejected because Android/iOS/Windows transfer tools often normalize extensions to lowercase `.aae`.

---

## Decision 2: Ingestion & Placement Lifecycle

### Decision
During `MasterIngestService` transfer execution, the ingestion pipeline proactively inspects the source directory for any companion `.AAE` sidecar matching the media item being transferred. When found:
1. The `.AAE` sidecar is stream-copied directly into the target master folder right next to the media asset (e.g., `media/master/2022/26-Aug-2022/Photos/IMG_1234.AAE`).
2. If the media asset undergoes collision renaming (e.g. `IMG_1234_1.JPG`), the sidecar is assigned the exact same renamed stem (`IMG_1234_1.AAE`).
3. Stream transfer uses verified hashing with rollback on failure.

### Rationale
Non-destructive photo editing applications require the sidecar to reside in the exact same folder with the exact same base name as the media file. Ingesting them together atomically ensures consistency.

### Alternatives Considered
- *Separate Ingestion Pipeline for .AAE*: Treating `.AAE` as an independent asset. Rejected because `.AAE` files lack EXIF dates and have no inherent category (`Photos` vs `Videos`), which would risk misrouting them to fallback dates or separate folders.

---

## Decision 3: Parity Replication Strategy

### Decision
In `ReplicaSyncService`, when replicating a canonical master file (`.JPG`, `.HEIC`, `.MOV`, etc.) to a replica drive, the service checks whether a companion `.AAE` sidecar exists on the primary master filesystem at the source relative path. If present:
1. The companion `.AAE` file is transferred to the replica drive at the identical destination relative path.
2. Verified stream copy ensures cryptographic integrity.
3. If the `.AAE` file already exists on the replica drive with matching size and hash, redundant copying is skipped.

### Rationale
Keeps backup replicas 100% byte-for-byte and structure-for-structure synchronized with the primary master.

---

## Decision 4: Safe Synchronized Pruning & Orphan Cleanup

### Decision
In `DrivePruningService`:
1. When a source media asset is validated and marked for deletion, any companion `.AAE` sidecar in the source directory is checked.
2. If the master destination contains the verified `.AAE` file, the source `.AAE` file is pruned alongside the source media asset.
3. A sweep of unorganized source directories checks for any remaining `.AAE` files whose companion media file exists in the master catalog. If verified in master, the orphan `.AAE` is also safely pruned.
4. This ensures empty subdirectories contain 0 remaining files and can be cleaned up cleanly.

### Rationale
Leaving `.AAE` files in source directories leaves orphan files that prevent empty ancestor directory pruning (`017-prune-empty-directories`).
