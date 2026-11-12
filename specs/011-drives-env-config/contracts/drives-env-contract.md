# Contract: Drive Configuration and Environment Variables

## 1. File Contract: `drives.example.yaml`

A sample file committed to repository root demonstrating complete multi-drive configuration:

```yaml
# ==============================================================================
# Sample Drive Configuration Template for Media Metadata Extractor
# Copy this file to `drives.yaml` or configure paths via `.env`.
# ==============================================================================

version: "1.0"

# Retention priority groups (higher number = higher priority to keep)
groups:
  primary:
    priority: 100
    description: "Primary master drive group - source of truth"
  replica1:
    priority: 80
    description: "First backup replica group"
  replica2:
    priority: 70
    description: "Second backup replica group"
  staging:
    priority: 20
    description: "Temporary external drive or USB stick for ingestion"

# Configured drive instances
# Note: You can use ${ENV_VAR:-default} syntax to interpolate from .env
drives:
  - id: "primary_main"
    group: "primary"
    mount_path: "${DRIVE_PRIMARY_MOUNT:-/Volumes/DummyPrimary-1}"
    master_path: "${DRIVE_PRIMARY_MASTER:-/Volumes/DummyPrimary-1/SPSA/media/master}"

  - id: "replica_1"
    group: "replica1"
    mount_path: "${DRIVE_REPLICA1_MOUNT:-/Volumes/DummyReplica-1}"
    master_path: "${DRIVE_REPLICA1_MASTER:-/Volumes/DummyReplica-1/SPSA/media/master}"

  - id: "replica_2"
    group: "replica2"
    mount_path: "${DRIVE_REPLICA2_MOUNT:-/Volumes/DummyReplica-2}"
    master_path: "${DRIVE_REPLICA2_MASTER:-/Volumes/DummyReplica-2/SPSA/media/master}"

  - id: "staging_drive"
    group: "staging"
    mount_path: "${DRIVE_STAGING_MOUNT:-/Volumes/DummyStaging}"
```

---

## 2. Environment Variables Contract (`.env`)

Variables loaded from `.env` or process environment:

| Variable | Required | Description | Example |
|---|---|---|---|
| `DRIVE_PRIMARY_MOUNT` | **Yes** (if no YAML) | Primary drive mount path | `/Volumes/SP-1` |
| `DRIVE_PRIMARY_MASTER` | Optional | Canonical master path on primary drive | `/Volumes/SP-1/SPSA/media/master` |
| `DRIVE_PRIMARY_ID` | Optional | Custom drive ID for primary drive | `primary_main` |
| `DRIVE_REPLICA1_MOUNT` | Optional | Replica 1 drive mount path | `/Volumes/SP-2` |
| `DRIVE_REPLICA1_MASTER`| Optional | Canonical master path on replica 1 | `/Volumes/SP-2/SPSA/media/master` |
| `DRIVE_REPLICA2_MOUNT` | Optional | Replica 2 drive mount path | `/Volumes/SP-3` |
| `DRIVE_REPLICA2_MASTER`| Optional | Canonical master path on replica 2 | `/Volumes/SP-3/SPSA/media/master` |
| `MEDIA_DRIVES_CONFIG` | Optional | Explicit path to drive YAML/TOML file | `./drives.yaml` |

---

## 3. Rust Public API Contract

```rust
impl DrivesConfigLoader {
    /// Loads configuration with environment variable interpolation and fallback to .env variables.
    pub fn load_from_env_or_path(path: Option<&Path>) -> Result<DriveConfiguration, DeduplicationError>;

    /// Interpolates environment variables into a raw configuration string.
    pub fn interpolate_env_vars(content: &str) -> String;

    /// Synthesizes a DriveConfiguration directly from active environment variables.
    pub fn synthesize_from_env() -> Result<DriveConfiguration, DeduplicationError>;
}
```

---

## 4. CLI Contract: Deduplication Flags

```bash
metadata-extractor dedup [OPTIONS] [PATH]
```

| Flag | Short | Default | Description |
|---|---|---|---|
| `--config <PATH>` | `-c` | `drives.yaml` (or `.env` fallback) | Path to drive configuration YAML file. |
| `--channel-capacity <CAPACITY>` | *(none)* | `100` | Backpressure channel capacity (long-only to prevent `-c` collision). |
| `--dry-run` | *(none)* | `false` | Evaluate and preview classifications without modifying MongoDB. |

---

## 5. BSON Deserialization Contract

64-bit perceptual hashes stored in BSON as signed `Int64` are transparently reinterpreted into `u64`:

```rust
pub fn deserialize_u64_or_signed<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>;

pub fn deserialize_u64_or_signed_opt<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>;
```

    /// Synthesizes a DriveConfiguration directly from active environment variables.
    pub fn synthesize_from_env() -> Result<DriveConfiguration, DeduplicationError>;
}
```
