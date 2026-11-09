# Data Model: Externalized Configuration, Profiles, and Secret Management

**Feature**: `006-config-profiles-secrets`  
**Date**: 2026-11-08  
**Status**: Completed  

---

## 1. Domain Entities & Value Objects

### 1.1 `AppProfile` (Enum)
Represents the runtime operational posture of the application.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
pub enum AppProfile {
    #[default]
    Local,
    Production,
}
```

- **Variants**:
  - `Local`: Developer workstation mode. Relaxed validation, verbose diagnostic messages allowed, fallback to local host endpoints if unconfigured.
  - `Production`: Hardened mode. Strict validation, mandatory authentication, requires explicit non-default credentials, sanitizes all error telemetry.
- **Parsing**: From string (case-insensitive: "local", "production", "prod", "dev").

---

### 1.2 `AppConfig` (Domain Configuration Struct)
Encapsulates runtime configuration resolved across CLI, environment, and profiles.

```rust
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub profile: AppProfile,
    pub database: Option<DatabaseSinkConfig>,
    pub backpressure_capacity: usize,
    pub batch_size: usize,
}
```

- **Validation Rules**:
  - If `profile == AppProfile::Production` and `database` is configured:
    - URI MUST contain explicit credentials (username and password).
    - Password MUST NOT be empty or a default placeholder (e.g. `<password>`, `<MONGODB_PASSWORD>`, `password`).
    - Failure to satisfy throws `StorageError::ConfigurationError` with a clear explanation before any filesystem or network I/O.

---

### 1.3 `SecretResolver` (Value Object / Utility)
Responsible for interpolating secrets into connection URIs.

- **Capabilities**:
  - Detects placeholders: `<password>`, `<MONGODB_PASSWORD>`, `${MONGODB_PASSWORD}`.
  - Detects username without password in URI: `mongodb://user@host:port/db`.
  - Replaces placeholders with URL-encoded secret value from `MONGODB_PASSWORD`.
  - Strips/masks secrets when rendering debug/display representations via `sanitized_uri()`.

---

## 2. Environment Variables Specification

| Variable Name | Description | Default (Local) | Default (Production) |
|---|---|---|---|
| `APP_PROFILE` or `MEDIA_PROFILE` | Active profile name | `local` | `local` |
| `MONGODB_URI` | Full connection URI (with or without placeholder) | None | Required if DB sink enabled |
| `MONGODB_PASSWORD` | Externalized password injected into URI | None | Required if URI has placeholder |
| `MONGODB_COLLECTION` | Target collection name | `media_assets` | `media_assets` |
| `MONGODB_DATABASE` | Target database name override | `media_index_db` | `media_index_db` |

---

## 3. Configuration Precedence Resolution

```text
┌──────────────────────────────────────────────┐
│ 1. CLI Arguments (--mongodb-uri, --profile)  │ (Highest Priority)
└──────────────────────┬───────────────────────┘
                       │ Fallback if omitted
                       ▼
┌──────────────────────────────────────────────┐
│ 2. Environment Variables (MONGODB_URI, etc.) │
└──────────────────────┬───────────────────────┘
                       │ Loaded at boot
                       ▼
┌──────────────────────────────────────────────┐
│ 3. Environment Files (.env.{profile}, .env)  │
└──────────────────────┬───────────────────────┘
                       │ Fallback if unconfigured
                       ▼
┌──────────────────────────────────────────────┐
│ 4. Built-in Profile Defaults                 │ (Lowest Priority)
└──────────────────────────────────────────────┘
```
