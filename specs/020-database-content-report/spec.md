# Feature Specification: Database Content & Health Audit Report

**Feature Branch**: `020-database-content-report`

**Created**: 2026-11-14

**Status**: Draft

**Input**: User description: "i want to create a report of the content in database like how many files are there, group by extensions, how many copied, how many yet to replicate, how many are duplicates, how many are marked for deletes, how many dont have exif, how many have problems. when I use some subcommand, it should list top3 items filepath to check manually. also suggest the command what to execute next"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Comprehensive Database Media Summary Report (Priority: P1) 🎯 MVP

As a media collection curator or operator, I want to execute a single CLI reporting command that queries the media database catalog and outputs a structured health and status report—including total file count, breakdown by file extension, counts of assets copied to master storage, pending replica synchronizations, duplicate assets, assets marked for deletion, assets missing EXIF/metadata, and assets flagged with processing errors—so that I have an immediate high-level overview of the library state and pending operational workloads.

**Why this priority**: Delivers immediate baseline visibility into database holdings, storage distribution, and catalog hygiene without requiring manual database queries or external analytics tools.

**Independent Test**: Populate or mock a media catalog with various file types, statuses (ingested, pending replication, duplicates, marked for deletion, missing EXIF, error records). Run the report command. Verify that the output displays an accurate tabular summary reflecting the exact counts for each metric.

**Acceptance Scenarios**:

1. **Given** a populated database containing 1,500 cataloged media items (e.g., 1,000 JPEG, 300 HEIC, 200 MP4), **When** the operator runs the reporting command without extra flags, **Then** the terminal renders a formatted summary displaying:
   - Total cataloged file count (1,500)
   - Breakdown grouped by file extension with counts
   - Ingested / copied asset count
   - Assets yet to replicate count
   - Duplicate asset count
   - Assets marked for deletion count
   - Assets lacking EXIF metadata / timestamp count
   - Assets flagged with extraction or validation problems count
2. **Given** an empty database with no cataloged records, **When** the reporting command executes, **Then** the system gracefully outputs a zero-state summary indicating zero media files found and suggests running a scan/ingest command rather than failing or erroring out.

---

### User Story 2 - Targeted Manual Inspection with Top Sample Filepaths (Priority: P2)

As a media curator who sees non-zero counts of problematic or pending assets (such as corrupted files, missing EXIF tags, pending duplicates, or uncopied files), I want to execute an inspection subcommand or option (e.g., inspecting a specific category) that lists the top 3 file paths associated with that category, so that I can manually locate, examine, and verify those specific files on disk before taking corrective or batch actions.

**Why this priority**: Operators require tangible verification targets to validate data integrity anomalies before committing to destructive prunes, metadata overrides, or reprocessing.

**Independent Test**: With known problem files or un-replicated files in the database, invoke the inspection subcommand specifying the category (or sample flag). Verify that up to 3 valid file paths matching that specific status category are output with clear descriptive details.

**Acceptance Scenarios**:

1. **Given** 25 media files flagged with metadata extraction problems, **When** the operator invokes the inspection subcommand for problem items, **Then** the terminal outputs exactly 3 file paths from that problem set along with a brief description of the recorded error.
2. **Given** a category with fewer than 3 items (e.g., exactly 2 files missing EXIF), **When** the operator requests the top samples for missing EXIF items, **Then** the command outputs all available items (2 paths) without padding, placeholders, or out-of-bounds errors.
3. **Given** a category with zero items (e.g., zero duplicates), **When** the operator requests inspection of that category, **Then** the command cleanly reports that no items match the criteria.

---

### User Story 3 - Contextual Next-Action Guidance (Priority: P3)

As an operator reviewing report findings, I want the reporting tool to analyze the current catalog state and suggest the exact command(s) to execute next (e.g., recommending deduplication if duplicates exist, replica sync if un-replicated files exist, ingestion if unmastered files exist, or confirming system readiness if all operations are up to date), so that I do not have to memorize workflow sequence logic or drive configurations.

**Why this priority**: Prevents operator errors and guides users through the multi-stage ingestion and organization lifecycle based on actual catalog state.

**Independent Test**: Seed the database in various state profiles (e.g., pending replication, pending ingestion, pending duplicate pruning, and fully clean). Execute the report command for each profile and verify the corresponding recommended CLI command is displayed in the "Suggested Next Action" section.

**Acceptance Scenarios**:

1. **Given** un-replicated master files detected in the database, **When** the report finishes rendering, **Then** the output displays a prominent "Recommended Next Action" suggesting the replica synchronization command.
2. **Given** assets marked for deletion or identified duplicates awaiting prune, **When** the report finishes, **Then** the output suggests the organize or prune command.
3. **Given** a completely synchronized, deduped, and healthy collection with 0 pending actions, **When** the report finishes, **Then** the output confirms the library is fully organized and up to date, suggesting no pending corrective commands.

---

### Edge Cases

- **Large Catalogs**: How does the system handle catalogs containing millions of records? Aggregations and count summaries must complete within acceptable interactive CLI response times without exhausting client memory.
- **Missing or Disconnected Storage Backend**: How does the command react if the database service is unreachable or credentials fail? It must emit a clear, actionable diagnostic message indicating connection failure and exit with a non-zero status code without dumping raw stack traces.
- **Unrecognized or Missing Extensions**: How are files without an extension or with malformed dot-files cataloged in the extension breakdown? They must be grouped under a distinct `[none]` or `[unknown]` category rather than omitted.
- **Mixed File Path Encodings / Non-ASCII Names**: When displaying top 3 filepaths, how are paths with Unicode characters, spaces, or non-UTF8 bytes handled? They must be properly escaped or displayed cleanly without corrupting terminal layouts.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide a dedicated reporting command (accessible via the CLI) to generate a summary of media assets stored in the database catalog.
- **FR-002**: The report MUST display the total count of cataloged media files.
- **FR-003**: The report MUST display a breakdown of file counts grouped by file extension (e.g., jpg, png, heic, mp4, mov), sorted by count in descending order.
- **FR-004**: The report MUST display the total count of media files copied / ingested to canonical master storage.
- **FR-005**: The report MUST display the total count of media assets that are yet to be replicated across configured replica locations.
- **FR-006**: The report MUST display the total count of duplicate media assets identified in the catalog.
- **FR-007**: The report MUST display the total count of media assets currently marked for deletion or pruning.
- **FR-008**: The report MUST display the total count of media assets lacking EXIF metadata or creation timestamps.
- **FR-009**: The report MUST display the total count of media assets flagged with processing problems, decoding errors, or validation issues.
- **FR-010**: The system MUST provide an inspection capability (via subcommand or category filter) that lists up to 3 file paths for items matching a selected category (e.g., `problems`, `missing-exif`, `duplicates`, `unreplicated`, `marked-for-delete`).
- **FR-011**: When fewer than 3 items exist in an inspected category, the system MUST display all available item paths without error.
- **FR-012**: The report output MUST conclude with an intelligent "Suggested Next Action" recommending the next logical CLI command to run based on prioritized catalog state (e.g., ingest, sync-replicas, dedup/prune, or confirmation that the library is in a steady state).
- **FR-013**: The system MUST support an optional machine-readable output format (such as JSON) for integration into scripts and automated dashboards.

### Key Entities

- **Catalog Media Summary**: An aggregated snapshot of the media database containing total item count, extension distribution, and categorized tallies for ingestion, replication, deduplication, metadata completeness, and error states.
- **Extension Grouping**: A pairing of file extension identifier (normalized lowercase) and the corresponding tally of media assets sharing that extension.
- **Status Category**: A classification of media records representing operational state (`IngestedMaster`, `PendingReplication`, `Duplicate`, `MarkedForDelete`, `MissingExif`, `Problematic`).
- **Inspection Sample**: A lightweight record containing a matching asset's file path, identifier, and relevant status detail (e.g., specific error reason or replica discrepancy) retrieved for operator verification.
- **Command Recommendation**: An actionable suggestion comprising an explanation of why the action is needed and the exact CLI command string the operator can run next.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Operators can view the complete database summary report within 3 seconds for catalogs containing up to 1,000,000 media assets.
- **SC-002**: Inspection queries for top 3 item filepaths return results in under 1 second.
- **SC-003**: 100% of reported status categories (copied, yet to replicate, duplicates, marked for deletion, missing EXIF, problems) reflect the exact state of records in the database.
- **SC-004**: The command recommendation accurately points to the most critical pending workflow step in 100% of tested catalog state scenarios.
- **SC-005**: Reports render legibly in standard terminal environments (80+ columns) without truncated columns or misaligned table borders.

## Assumptions

- **Access to Database**: The user running the report command has configured database connection credentials via standard environment variables or configuration files.
- **Canonical Master & Replica State**: Assets are classified as "copied" if they have an active master path record, and "yet to replicate" if configured replica storage targets lack confirmed replica entries.
- **Default Sample Limit**: The default inspection sample limit is 3 items, but the system can accept an optional count parameter if the user desires more or fewer sample paths.
- **Read-Only Operation**: The reporting and inspection commands are strictly read-only and do not alter any file system assets or database records.
