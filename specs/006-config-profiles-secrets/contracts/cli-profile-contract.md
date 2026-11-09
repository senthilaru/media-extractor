# CLI Contract: Profiles and Externalized Secrets

**Feature**: `006-config-profiles-secrets`  
**Contract Version**: `1.0.0`  

---

## 1. CLI Options

### 1.1 `--profile <NAME>`
- **Short option**: None (or `-p` if unambiguous)
- **Long option**: `--profile <PROFILE>`
- **Possible values**: `local`, `production` (case-insensitive)
- **Default value**: `local` (or value of `APP_PROFILE` / `MEDIA_PROFILE` environment variable if present)
- **Description**: "Operational profile (local, production). Determines configuration validation rules and defaults."

### 1.2 `--mongodb-uri <URI>`
- **Aliases**: `--db-uri`
- **Environment fallback**: If omitted on CLI, checked in `MONGODB_URI` environment variable.
- **Password interpolation**:
  - If URI contains `<MONGODB_PASSWORD>`, `<password>`, or `${MONGODB_PASSWORD}`, value is replaced with `MONGODB_PASSWORD` environment variable.

---

## 2. Exit Codes

| Code | Condition |
|---|---|
| `0` | Success (scan and database operations completed normally). |
| `1` | General runtime error, filesystem I/O error, or decoder failure. |
| `2` | Configuration error: unknown profile, missing mandatory credentials in `production` mode, unparseable URI. |
| `3` | Database connectivity failure or authentication failure. |

---

## 3. Stderr Telemetry & Log Output

In all profiles, sensitive credentials MUST be masked in startup logs and diagnostic outputs:
```text
[INFO] Active profile: local
[INFO] Connecting to MongoDB at mongodb://media_app:***@localhost:27017/media_index_db?authSource=media_index_db...
```

If `production` profile encounters a missing password or invalid credential:
```text
[ERROR] Configuration error (production profile): MongoDB password is required. Please set MONGODB_PASSWORD environment variable or provide authenticated MONGODB_URI.
```
Exit code: `2`.
