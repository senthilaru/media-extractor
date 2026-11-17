# Feature Specification: Prune Empty Directories Cleanup

**Feature Branch**: `017-prune-empty-directories`

**Created**: 2026-11-13

**Status**: Draft

**Input**: User description: "when i run 'cargo run -- dedup prune --drive /Volumes/SP-1/SPSA/unorganized/Photos --execute' it deletes the duplicate files, after deleting it keep the empty directory. If the directory is empty, we can delete that too."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Automatic Empty Directory Removal on Prune Execution (Priority: P1)

As a media archivist cleaning up duplicate media from an unorganized photo folder or staging drive, when I run duplicate pruning with execution enabled, I want any subdirectories that become empty after their files are deleted to be automatically removed, so that I am not left with hundreds of nested empty folders cluttering my storage.

**Why this priority**: Leaving empty directory trees behind after deleting duplicate files leaves visual clutter and forces users to manually find and delete stranded empty folders. Automating empty folder cleanup as part of pruning completes the cleanup lifecycle.

**Independent Test**: Create a folder structure with nested subdirectories containing duplicate files marked for removal. Execute prune. Verify that all duplicate files are deleted, all empty subdirectories are removed from disk, and non-empty sibling directories remain intact.

**Acceptance Scenarios**:

1. **Given** a directory `Photos/2021/11-Nov-2021` containing only duplicate files marked for removal, **When** prune execution runs, **Then** all duplicate files are deleted, and the now-empty directories `11-Nov-2021` and `2021` are deleted.
2. **Given** a directory containing two files, where one is marked for removal and one is kept, **When** prune execution runs, **Then** the duplicate file is deleted and the containing directory remains intact because it still contains a file.
3. **Given** a folder containing only duplicate files and a hidden system metadata file (such as `.DS_Store`), **When** prune execution runs, **Then** the duplicate files and the orphaned system metadata file are removed, allowing the containing folder to be cleanly deleted.

---

### User Story 2 - Boundary Preservation of Target and Protected Roots (Priority: P2)

As a safety-conscious user specifying a prune target root directory (e.g., `--drive /Volumes/SP-1/SPSA/unorganized/Photos`), I want the specified target directory itself and all protected master archives to NEVER be deleted under any circumstances, even if all files and subdirectories beneath the target root are deleted.

**Why this priority**: The root directory specified by the user represents an intentional mount point or target workspace container. Deleting the target root folder itself or crossing into protected master archives could cause system disruption or data loss.

**Independent Test**: Target a directory that contains only duplicate files. Execute prune. Verify that all subdirectories beneath the target are deleted, but the target root directory itself remains present on disk.

**Acceptance Scenarios**:

1. **Given** a target directory `/Volumes/SP-1/SPSA/unorganized/Photos` where all subfolders become empty after pruning, **When** prune execution completes, **Then** all child subdirectories inside `Photos` are removed, but `/Volumes/SP-1/SPSA/unorganized/Photos` itself is preserved.
2. **Given** any directory residing within a protected canonical master archive, **When** prune evaluation or directory cleanup executes, **Then** the system strictly forbids directory removal within the protected archive root.

---

### User Story 3 - Dry-Run Preview and Telemetry Reporting (Priority: P3)

As an operator running pruning in preview (dry-run) mode, I want to see the count of empty directories that will be eligible for cleanup, and in execution mode, I want to see the exact count of empty directories removed in the final audit summary.

**Why this priority**: Transparency ensures operators know exactly what changes will occur before modifying the filesystem, and provides clear audit telemetry after execution.

**Independent Test**: Run prune in preview mode against a directory with duplicate files. Verify the output displays the anticipated empty directories to be removed without modifying disk. Run with execution enabled and verify the summary reports the actual count of directories deleted.

**Acceptance Scenarios**:

1. **Given** a dry-run prune operation where 5 subdirectories will become empty upon file deletion, **When** the preview report is displayed, **Then** it reports that 5 empty directories are eligible for removal, with 0 directories deleted on disk.
2. **Given** an executed prune operation that successfully removes 5 empty subdirectories, **When** the execution summary is displayed, **Then** it clearly lists `Empty Directories Removed: 5`.

---

### User Story 4 - Upward Ancestor Directory Pruning to Drive Mount Ceiling in Organize (Priority: P1)

As an operator running the unified `organize` pipeline on an event folder (e.g. `/Volumes/Seagate Backup Plus Drive/2026/18Jul2026`), after all media assets have been ingested and replicated, I want the now-vacated event folder `18Jul2026` and any now-empty parent folders (such as `2026`) to be cleanly pruned up to the external drive mount ceiling (`/Volumes/Seagate Backup Plus Drive`), so that no empty ghost directories are left behind.

**Acceptance Scenarios**:

1. **Given** an organize run targeting `/Volumes/ExternalDrive/2026/18Jul2026` where all files are moved to master, **When** stage 5 pruning executes without `--keep-source-dir`, **Then** `18Jul2026` and `2026` are removed, while `/Volumes/ExternalDrive` is strictly preserved.
2. **Given** an organize run with `--keep-source-dir` passed, **When** stage 5 pruning executes, **Then** child files and subdirectories are pruned if empty, but `/Volumes/ExternalDrive/2026/18Jul2026` is preserved.

---

### User Story 5 - Zero-Byte Dummy File Disposal & Ghost Tree Pruning (Priority: P1)

As a user with corrupted or zero-byte placeholder files remaining in source directories (e.g. `DSC_0256.JPG` or `PXL_...mp4` of 0 bytes), I want pruning to recognize 0-byte files as disposable artifacts, unlinking them and pruning their empty parent folder trees and database records, so that zero-byte files do not permanently block empty directory removal.

**Acceptance Scenarios**:

1. **Given** a directory containing only zero-byte dummy files and/or `.DS_Store` files, **When** prune executes, **Then** the zero-byte files and OS metadata files are unlinked, the directory is deleted, and any matching zero-byte documents in MongoDB are purged.
2. **Given** a user running `cargo run -- dedup prune --drive <DRIVE> --clean-empty-dirs`, **When** executed, **Then** all existing orphaned empty directories and 0-byte dummy files across the specified drive are swept and removed.

---

### Edge Cases

- **Directory with non-media files**: If a directory contains non-media files (e.g. text notes, documents) with non-zero size that were not part of the duplicate candidate list, the directory is not empty and MUST NOT be deleted.
- **Zero-byte dummy files**: If a directory contains only 0-byte dummy files and OS metadata, it is treated as disposable; the 0-byte files are unlinked, allowing the directory hierarchy to be cleaned.
- **Hidden macOS `.DS_Store` or Windows `Thumbs.db` files**: If a directory has no media files left but contains only an orphaned `.DS_Store` or `Thumbs.db` file generated by the operating system file browser, the cleaner must handle this file and delete the directory rather than leaving an empty shell behind.
- **Deeply nested empty hierarchies**: If a deep chain of directories (e.g. `A/B/C/D/`) becomes empty after deleting files in `D`, the cleanup must unwind bottom-up (D, then C, then B, then A) stopping strictly at the prune root or drive boundary ceiling.
- **Permission errors**: If an empty directory cannot be deleted due to read-only filesystem attributes or permission restrictions, the error must be recorded in the audit report without failing the deletion of other empty directories or interrupting the database sync.
- **Concurrent modifications**: If another process places a file into an empty directory between the candidate scan and directory removal, directory deletion must safely fail (standard directory removal requires emptiness) and leave the new file unharmed.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: During duplicate file pruning execution, the system MUST identify all subdirectories under the pruned target root that become empty as a result of duplicate file deletions.
- **FR-002**: The system MUST recursively remove empty directories bottom-up (from innermost leaf directory up through ancestor directories).
- **FR-003**: In standard `dedup prune`, the system MUST NEVER delete the prune target root directory itself unless explicitly configured with `preserve_prune_root: false` (e.g. In `organize` without `--keep-source-dir`).
- **FR-004**: The system MUST NEVER delete or modify any directory located within a protected canonical master archive (primary master or replica backup master paths) or system/home roots (`/Volumes/*`, `/Users/*`, `/`).
- **FR-005**: The system MUST treat directories that contain only orphaned system metadata files (e.g., `.DS_Store`, `Thumbs.db`) or 0-byte dummy files as empty by removing the disposable files and deleting the containing folder.
- **FR-006**: The system MUST preserve any directory that contains one or more remaining non-zero-byte user files or non-empty child directories.
- **FR-007**: In dry-run preview mode, the system MUST calculate and report the number of empty directories eligible for removal without modifying the filesystem.
- **FR-008**: In execution mode, the prune report and terminal summary MUST display the count of empty directories successfully removed.
- **FR-009**: If an empty directory cannot be removed due to filesystem permission or lock constraints, the system MUST log a diagnostic warning, increment the error count in the report, and continue processing remaining directories without crashing.
- **FR-010**: The system MUST support an on-demand `--clean-empty-dirs` flag in `dedup prune` to sweep and remove orphaned empty directories across a target drive.
- **FR-011**: The system MUST purge obsolete 0-byte database records from MongoDB during prune execution.

### Key Entities *(include if feature involves data)*

- **Prune Execution Report**: Audit entity expanded to track `empty_directories_removed` alongside files deleted, bytes reclaimed, and database records purged.
- **Directory Cleanup Scope**: Bounded filesystem hierarchy defined by the target prune root as the upper ceiling and deleted duplicate file locations as the leaves.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of subdirectories that become completely empty after duplicate file pruning are deleted from the filesystem when execution is enabled.
- **SC-002**: 0% of prune target root directories or protected canonical master directories are deleted under any circumstance.
- **SC-003**: In dry-run preview mode, 0 files and 0 directories are deleted or modified on disk.
- **SC-004**: The prune execution summary reports the exact count of empty directories removed.
- **SC-005**: Directory cleanup adds less than 5% overhead to total prune execution time.

## Assumptions

- Empty directory cleanup is performed immediately following the successful deletion of duplicate files within the prune operation.
- The user has sufficient filesystem permissions to delete empty folders within the target directory.
- Operating system metadata files like `.DS_Store` left behind in otherwise vacated media folders are considered disposable artifacts of the vacated directory.
- The prune target path specified by the user may be either a mounted drive root or an arbitrary directory subfolder (e.g., `/Volumes/SP-1/SPSA/unorganized/Photos`).
