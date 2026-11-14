# Research & Technical Decisions: Prune Empty Directories Cleanup

## Decision 1: Upward Ancestor Traversal vs Global Recursive Scan

### Decision
Collect unique parent directories from all deleted duplicate files, sort by directory path depth descending (deepest first), and recursively inspect each ancestor directory upward until reaching a non-empty directory or the designated prune root.

### Rationale
- Scanning an entire multi-terabyte external hard drive with millions of files to locate empty directories after deleting 20 duplicates would take minutes of random I/O head thrashing.
- Gathering unique parent paths of deleted files and walking upward touches only the affected branches.
- Depth-descending order ensures leaf directories are checked and removed first; when a leaf is removed, its parent may now be empty on the subsequent iteration.
- Traversal terminates as soon as a parent directory contains other files/directories or reaches the prune root ceiling.

### Alternatives Considered
- *Full recursive WalkDir post-scan*: Evaluates every directory in the tree. Dismissed because it causes severe I/O degradation on external mechanical HDDs (violating Principle I).
- *Single-level parent deletion*: Only deletes the immediate parent directory of the file. Dismissed because nested empty folder hierarchies (e.g. `Photos/2021/11-Nov-2021/event/`) would leave `Photos/2021/11-Nov-2021/` stranded.

---

## Decision 2: Handling Disposable OS Metadata Files (`.DS_Store`, `Thumbs.db`)

### Decision
A directory is considered empty if it contains zero entries, OR if all remaining entries are recognized disposable operating system indexing files (`.DS_Store`, `Thumbs.db`). In execution mode, any disposable OS metadata files in an otherwise empty directory are unlinked so the directory can be cleanly removed via `std::fs::remove_dir`.

### Rationale
- On macOS, opening any folder in Finder automatically generates `.DS_Store`. If a user organizes or views their unorganized photos in Finder, almost every directory has `.DS_Store`.
- If `.DS_Store` prevented directory removal, virtually zero directories would ever be cleaned up, defeating the entire purpose of the user's request.
- Deleting `.DS_Store` inside an otherwise empty folder is safe because `.DS_Store` only stores view preferences (icon positions, window size) for that folder.

### Alternatives Considered
- *Strict emptiness check only*: Fails if `.DS_Store` exists. Dismissed as non-viable on macOS where `.DS_Store` is pervasive.
- *Forceful recursive removal (`remove_dir_all`)*: Extremely dangerous if a directory contains legitimate user files. Dismissed because `remove_dir` guarantees emptiness safety.

---

## Decision 3: Boundary Guarding and Pruning Root Preservation

### Decision
1. The pruning target root (`prune_target`, e.g. `/Volumes/SP-1/SPSA/unorganized/Photos` or a drive mount root `/Volumes/SP-1`) is strictly protected as the stopping ceiling: `if candidate_dir == prune_target { break; }`.
2. Paths are canonicalized or normalized to prevent path aliasing bypasses.
3. Every candidate directory is validated against `ArchiveProtectionGuard::is_protected_path`: any canonical master archive path is strictly forbidden from deletion.

### Rationale
- The user targets `--drive /Volumes/SP-1/SPSA/unorganized/Photos`. Deleting `Photos` itself would delete the user's staging container and could cause errors for subsequent ingestion/scan workflows.
- Master archives (`/Volumes/SP-1/SPSA/media/master`, `/Volumes/SP-2/SPSA/media/master`) must be bulletproof against deletion under all circumstances.

### Alternatives Considered
- *Allowing target root deletion if completely empty*: Rejected per user expectation that the target folder represents the designated workspace.

---

## Decision 4: Dry-Run Simulation

### Decision
In dry-run mode, simulate empty directory cleanup by:
1. Collecting the candidate paths marked for removal.
2. For each affected directory, inspecting physical entries on disk while discounting entries present in the simulated deletion set or disposable OS metadata.
3. Propagating simulated emptiness upward to count all directories that would become empty without touching the filesystem.

### Rationale
Provides 100% accurate preview telemetry in `--dry-run` matching what `--execute` will do.
