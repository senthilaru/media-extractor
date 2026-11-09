# Data Model: Direct MongoDB Storage Sink

**Feature**: `005-mongodb-storage-sink`  
**Date**: 2026-11-08  
**Status**: Completed  

---

## 1. Domain Entities & Value Objects

### 1.1 `DatabaseSinkConfig`

Encapsulates configuration parameters for connecting and persisting to the target document store.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseSinkConfig {
    /// Full database connection string (e.g. mongodb://user:pass@host:port/db?authSource=db)
    pub connection_uri: String,

    /// Target database name (parsed from URI or overridden)
    pub database_name: String,

    /// Target collection for media metadata documents (default: "media_assets")
    pub collection_name: String,

    /// Maximum number of documents per bulk write operation (default: 500)
    pub batch_size: usize,

    /// Connection and socket timeout in seconds (default: 10)
    pub timeout_seconds: u64,

    /// Maximum retries for transient connection/write timeouts (default: 3)
    pub max_retries: u32,
}
```

#### Validation Rules:
- `connection_uri` must start with `mongodb://` or `mongodb+srv://`.
- `collection_name` must not be empty and must not contain restricted characters (`$`, null bytes).
- `batch_size` must be $> 0$ and $\le 5,000$.
- `sanitized_uri()` must replace plaintext password components with `***`.

---

### 1.2 `BatchWriteStats`

Telemetry and accounting record returned after completing a bulk write operation.

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchWriteStats {
    /// Number of existing records matched by selector
    pub matched_count: u64,

    /// Number of existing records whose content was updated
    pub modified_count: u64,

    /// Number of newly inserted records
    pub upserted_count: u64,

    /// Number of documents that failed to write
    pub failed_count: u64,
}
```

---

### 1.3 `StorageError`

Strongly-typed error enum describing database connectivity and persistence failures.

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Database connection failed to {uri}: {message}")]
    ConnectionFailed {
        uri: String,
        message: String,
    },

    #[error("Database authentication failed on {database}: {message}")]
    AuthenticationFailed {
        database: String,
        message: String,
    },

    #[error("Index initialization failed on collection '{collection}': {message}")]
    IndexCreationFailed {
        collection: String,
        message: String,
    },

    #[error("Bulk write failed on collection '{collection}': {message}")]
    BulkWriteFailed {
        collection: String,
        message: String,
        is_transient: bool,
    },

    #[error("Failed to serialize document to BSON: {0}")]
    SerializationError(String),
}
```

---

## 2. MongoDB Document Schema Mapping

The domain aggregate `MediaMetadataDocument` maps directly to MongoDB BSON documents with zero manual transformations:

| Field Path | BSON Type | Index Specification | Purpose |
|---|---|---|---|
| `_id` | `UUID` / `ObjectId` | Primary Key | Document unique ID |
| `file_context.file_path` | `String` | **Unique Index (`{ "file_context.file_path": 1 }`)** | Idempotent upsert key |
| `file_context.extension` | `String` | — | File format filter |
| `file_context.media_category` | `String` | Index | `Photo`, `Video`, `RawPhoto` filter |
| `temporal.year` | `Int32` | Compound Index (`{ "temporal.year": 1, "temporal.month": 1 }`) | Year calendar facet |
| `temporal.month` | `Int32` | Compound Index | Month calendar facet |
| `temporal.day` | `Int32` | — | Day calendar facet |
| `temporal.timestamp_utc` | `ISODate` / `String` | — | Normalized UTC timestamp |
| `camera.make` | `String` | Optional Index | Camera manufacturer |
| `camera.model` | `String` | Optional Index | Camera model |
| `fingerprint.cryptographic_hash`| `String` | **Index (`{ "fingerprint.cryptographic_hash": 1 }`)** | BLAKE3 exact duplicate query |
| `fingerprint.phash` | `Int64` | **Index (`{ "fingerprint.phash": 1 }`)** | 64-bit DCT pHash visual similarity |
| `extraction_status.status` | `String` | — | `Success`, `Partial`, `Failed` |

---

## 3. Idempotent Upsert Operation Structure

For every document in a batch, the adapter generates an `UpdateOneModel`:

```json
{
  "q": { "file_context.file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3" },
  "u": {
    "$set": {
      "id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
      "file_context": { ... },
      "temporal": { ... },
      "camera": { ... },
      "exposure": { ... },
      "dimensions": { ... },
      "fingerprint": { ... },
      "extraction_status": { ... },
      "ingested_at": ISODate("2026-11-08T12:00:00Z")
    }
  },
  "upsert": true
}
```
