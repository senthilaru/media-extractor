# Feature Specification: Direct MongoDB Storage Sink

**Feature Branch**: `005-mongodb-storage-sink`  
**Created**: 2026-11-08  
**Status**: Draft  
**Input**: User description: "i want to save the output file into local mongodb mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Direct Media Metadata Persistence to MongoDB (Priority: P1) 🎯 MVP

As a media library administrator or developer, I want to provide a database connection URI directly to the scanner command so that all discovered and extracted media metadata documents are automatically ingested into my local or remote document database during the scan without needing intermediate export files or manual shell pipe operations.

**Why this priority**: Directly persisting records to a document database eliminates disk I/O for intermediate files, removes complex multi-step orchestration (scanning to NDJSON and then running import utilities), and enables immediate querying of media assets while scans are still in flight.

**Independent Test**: Execute the scanner targeting a media directory with a database connection string parameter; verify that records appear in the target collection with complete metadata, timestamps, and fingerprints upon completion.

**Acceptance Scenarios**:

1. **Given** a directory of media files and a valid database connection string (e.g. `mongodb://user:pass@localhost:27017/dbname?authSource=dbname`), **When** the scanner is executed with the database target, **Then** all extracted metadata documents are streamed and stored directly into the designated database collection.
2. **Given** a single media file path and a database target, **When** executed, **Then** the single document is extracted and inserted or updated in the database collection.
3. **Given** a custom collection name option (or default fallback collection such as `media_assets`), **When** specified, **Then** documents are routed to that specific collection within the target database.

---

### User Story 2 - Idempotent Upsert & Re-Scan Synchronization (Priority: P2)

As an archivist repeatedly indexing folders and external drives, I want re-scanning a directory to update existing records or insert only newly discovered assets rather than creating duplicate entries, so that my database catalog remains clean, consistent, and deduplicated across multiple runs.

**Why this priority**: Large media collections are frequently re-scanned as new photos and videos are imported. Without idempotent upsert semantics, repeated scans pollute collections with duplicate records or fail with unique index conflicts.

**Independent Test**: Run a scan targeting a folder into an empty collection; note the document count. Run the exact same scan a second time; verify the document count remains unchanged and existing documents reflect updated scan timestamps without duplicates.

**Acceptance Scenarios**:

1. **Given** a database collection containing previously ingested media records, **When** a re-scan is performed on the same files, **Then** existing records are updated based on their unique file identity (file path and content signature) rather than duplicated.
2. **Given** a folder where several new files were added and existing files were modified, **When** re-scanned, **Then** new assets are inserted and modified assets are refreshed with new metadata and fingerprint hashes.
3. **Given** a database collection, **When** initialized for storage, **Then** appropriate query and uniqueness indexes (e.g., file path, capture year/month, cryptographic hash) are created automatically if they do not already exist.

---

### User Story 3 - Connection Resilience, Authentication & Ingestion Telemetry (Priority: P3)

As an infrastructure operator running batch scans, I want the system to validate database connectivity and credentials before starting lengthy directory walks, gracefully handle temporary database hiccups with automatic retries, and display live insertion throughput and error counts on standard error, so that I have complete operational confidence in the ingest process.

**Why this priority**: When indexing terabytes of data across thousands of directories, finding out that credentials were misconfigured after an hour of filesystem walking causes severe user frustration. In addition, transient connection drops must not abort an otherwise successful multi-hour scan.

**Independent Test**: Provide an invalid database authentication URI; verify the command fails fast before filesystem walking with a descriptive connection error. Simulate a temporary network pause during ingestion; verify the system retries without dropping records.

**Acceptance Scenarios**:

1. **Given** an invalid or unreachable database URI, **When** the scanner starts, **Then** it validates the connection immediately and exits with a clear, user-friendly diagnostic message before starting traversal.
2. **Given** transient database network blips during a live scan, **When** individual batch writes encounter transient timeouts, **Then** the system automatically retries with exponential backoff before marking items as failed.
3. **Given** live ingestion into the database, **When** running, **Then** the progress telemetry on standard error reports the number of documents successfully written to the database, insertion rate, and any write failures.

---

### Edge Cases

- **Database Server Unavailable or Network Dropouts**: If the database server terminates mid-scan, the system must buffer batches up to channel limits, attempt retries with backoff, and if unrecoverable, emit a clean error summary without crashing with unhandled exceptions.
- **Authentication Failure or Insufficient Privileges**: If provided credentials lack write permissions to the specified database or collection, report the exact authorization failure upfront before scanning starts.
- **Simultaneous Database Storage and File Output**: The user can specify both a database target and a local output file (or stdout); both destinations must receive the metadata stream without data loss.
- **Large Batches and Write Rate Mismatch**: When filesystem traversal and metadata extraction produce records faster than the database can write, the system must apply backpressure to avoid unbounded memory accumulation.
- **Special Characters in Connection Credentials**: Passwords containing URL-encoded special characters (e.g. `@`, `:`, `/`, `?`, `#`) in the connection string must be parsed and authenticated correctly.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide an optional CLI argument (e.g. `--mongodb-uri` or `--db-uri`) accepting a standard database connection string, supporting credentials, host, port, target database name, and authentication options (e.g. `?authSource=...`).
- **FR-002**: System MUST support an optional CLI argument (e.g. `--collection` or `--db-collection`) to specify the target collection name, defaulting to a sensible collection name (`media_assets`) if omitted.
- **FR-003**: System MUST validate database connectivity and authentication credentials during command initialization before initiating directory traversal or media extraction.
- **FR-004**: System MUST perform idempotent bulk upsert operations into the target collection, using the media file's unique filesystem path as the primary document key or selector.
- **FR-005**: System MUST ensure foundational indexes exist on the target collection upon connection initialization (including unique index on file path, compound indexes on temporal facets like year/month, and index on cryptographic hash).
- **FR-006**: System MUST stream metadata documents to the database using bounded batch channels, applying backpressure to traversal and extraction workers to prevent memory consumption from exceeding 100 MB.
- **FR-007**: System MUST support retrying transient database write failures using exponential backoff before classifying a document write as failed.
- **FR-008**: System MUST isolate write errors on individual documents or batches, recording write failure tallies in telemetry while allowing the recursive media scan to proceed uninterrupted.
- **FR-009**: System MUST support concurrent output destinations, allowing users to persist to the database while simultaneously writing to an output file (`-o, --output`) or streaming to standard output.
- **FR-010**: System MUST report live database write metrics on standard error, including total documents written, write throughput (docs/sec), active queue depth, and write error counts.
- **FR-011**: System MUST flush all buffered documents and cleanly terminate database client connection pools upon scan completion or graceful termination (SIGINT/Ctrl+C).
- **FR-012**: System MUST sanitize connection strings in log messages and telemetry output to prevent leaking plaintext credentials and passwords.

---

### Key Entities

- **DatabaseSinkConfig**: Configuration parameters defining the database target.
  - `connection_uri`: Full database connection string including host, port, credentials, and parameters.
  - `database_name`: Extracted or explicitly configured target database.
  - `collection_name`: Target collection for media metadata documents.
  - `batch_size`: Maximum number of documents per bulk write operation.
  - `max_retries`: Maximum retry attempts for transient write failures.
- **IngestionSession**: Active database persistence session tracking metrics.
  - `total_inserted`: Count of newly created documents.
  - `total_updated`: Count of refreshed/upserted documents.
  - `total_failed`: Count of write failures encountered.
  - `write_latency_ms`: Moving average write latency per batch.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can persist extracted metadata directly to a local or remote document database by supplying a single connection URI argument to the unified command.
- **SC-002**: 100% of discovered media documents are successfully stored in the target database collection without requiring intermediate disk export files.
- **SC-003**: Re-running a scan over an identical directory structure results in 0 duplicate records and 100% updated or verified records.
- **SC-004**: Ingestion throughput into a local database exceeds 500 documents per second under standard multi-core hardware.
- **SC-005**: Database connectivity and credential validity are verified in under 2 seconds during initialization, failing fast before filesystem traversal if invalid.
- **SC-006**: Total application memory consumption remains strictly bounded under 100 MB during continuous database streaming across 50,000+ files.
- **SC-007**: Connection string credentials and passwords are never emitted in plaintext to console logs, error messages, or telemetry streams.

---

## Assumptions

- A running MongoDB instance (version 4.4+) is accessible at the specified connection URI when the option is invoked.
- If no database connection argument is provided, the scanner defaults to stdout streaming / file sink behavior without attempting database connections.
- The target database user account has permissions to read, write, and create indexes on the specified database and collection.
- Default collection name is `media_assets` when `--collection` is not explicitly supplied.
- Upsert operations use the file's canonical path as the primary identity key, matching previous scans on the same machine/mount point.
