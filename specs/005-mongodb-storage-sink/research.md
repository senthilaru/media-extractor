# Technical Research: Direct MongoDB Storage Sink

**Feature**: `005-mongodb-storage-sink`  
**Date**: 2026-11-08  
**Status**: Completed  

---

## 1. MongoDB Driver & Asynchronous Runtime Integration

### Decision
Use the official `mongodb` crate (version `3.1.0` or `3.2+`) configured for Tokio async runtime:
```toml
mongodb = { version = "3.2", default-features = false, features = ["tokio-runtime", "compat-3-0-0"] }
```

### Rationale
- The official driver natively supports asynchronous connection pooling, authentication mechanisms (SCRAM-SHA-1, SCRAM-SHA-256), and connection strings with query options (`?authSource=...`).
- It seamlessly integrates with our existing `tokio = "1.41"` runtime without spawning auxiliary thread pools or blocking executor threads.
- Direct BSON serialization is supported via `serde` derive on our existing `MediaMetadataDocument` domain aggregate.

### Alternatives Considered
- **Spawning `mongoimport` CLI subprocesses**: Fragile, requires users to have external tools installed, incurs high subprocess overhead, and prevents real-time error isolation.
- **Synchronous MongoDB driver (`mongodb` sync feature)**: Violates Constitution Principle I by blocking asynchronous worker tasks on database network I/O.

---

## 2. Hexagonal Domain Boundary & Port Abstraction

### Decision
In accordance with Constitution Principle II (Domain-Driven Design & SOLID Boundaries), the domain layer MUST NOT depend on the `mongodb` or `bson` crates.

We define an explicit asynchronous storage port in `src/domain/ports.rs`:
```rust
#[async_trait::async_trait]
pub trait MetadataStorageSink: Send + Sync {
    /// Validates database reachability and credentials.
    async fn ping(&self) -> Result<(), StorageError>;

    /// Ensures required query and uniqueness indexes exist on the target collection.
    async fn ensure_indexes(&self) -> Result<(), StorageError>;

    /// Persists a batch of metadata documents using idempotent bulk upsert semantics.
    async fn save_batch(&self, docs: &[MediaMetadataDocument]) -> Result<BatchWriteStats, StorageError>;

    /// Flushes buffered operations and closes connection pools.
    async fn flush(&self) -> Result<(), StorageError>;
}
```

The concrete adapter `MongoDbStorageSink` lives entirely in `src/infrastructure/mongodb_sink.rs`.

### Rationale
Decoupling storage ports from concrete database drivers allows:
1. Unit and pipeline testing without requiring live MongoDB instances (using mock sinks).
2. Future extensions to other document stores (e.g. SQLite, PostgreSQL JSONB, MeiliSearch) with zero domain modifications.

---

## 3. High-Throughput Idempotent Bulk Upsert Strategy

### Decision
Use unordered bulk write operations (`bulk_write`) with `UpdateOneModel` and `upsert: true`:
- **Selector Filter**: `{"file_context.file_path": doc.file_context.file_path}`
- **Update Document**: `{"$set": doc}`
- **Execution Mode**: `ordered: false`

### Rationale
- **Idempotency**: Scanning a folder twice updates existing records with the latest metadata/hash snapshot rather than failing with duplicate key errors or accumulating duplicate documents.
- **Unordered Parallelism**: `ordered: false` enables MongoDB to execute chunked writes concurrently across database worker threads and continue writing valid records even if an individual document encounters a write constraint.
- **Throughput**: Grouping 500 documents per bulk operation achieves $> 2,000$ documents/sec on local MongoDB instances while generating minimal network round-trips.

---

## 4. Automatic Indexing & Performance Optimization

### Decision
Upon client initialization and connection verification, the adapter automatically invokes index creation if indexes do not already exist:
1. **Uniqueness Index**: `{ "file_context.file_path": 1 }` (unique constraint ensuring path identity).
2. **Compound Timeline Index**: `{ "temporal.year": 1, "temporal.month": 1, "temporal.day": 1 }`.
3. **Cryptographic Identity Index**: `{ "fingerprint.cryptographic_hash": 1 }`.
4. **Perceptual Visual Hash Index**: `{ "fingerprint.phash": 1 }`.

### Rationale
- Creating indexes upfront guarantees sub-millisecond timeline and duplicate lookup performance from day one.
- Performing `ensure_indexes` once per scan session incurs negligible overhead (< 50 ms) while preventing duplicate index definition requests during high-frequency batch writes.

---

## 5. Security & Credential Masking

### Decision
Implement URI credential masking in `DatabaseSinkConfig`:
```rust
pub fn sanitized_uri(&self) -> String {
    // Mask password segment: mongodb://user:***@host:port/db?authSource=...
}
```
All CLI logs, error messages, and telemetry traces MUST print `sanitized_uri()` to prevent leaking credentials into terminal outputs or system logs.
