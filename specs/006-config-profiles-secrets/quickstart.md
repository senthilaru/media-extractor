# Quickstart Guide: Running with Profiles & Externalized Secrets

**Feature**: `006-config-profiles-secrets`  

---

## Prerequisites
- Rust 1.80+
- Optional: Local MongoDB instance for live ingestion tests

---

## 1. Setup Environment Configuration

Copy the sample environment file:
```bash
cp .env.example .env
```

Edit `.env` to supply your credentials:
```bash
APP_PROFILE=local
MONGODB_URI=mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db
MONGODB_PASSWORD=your_actual_password_here
```

---

## 2. Scenario 1: Running in Local Profile with Externalized Secret

Run the scan using the local profile with credentials sourced from `.env` or environment variables:
```bash
export MONGODB_PASSWORD="your_actual_password_here"

./target/release/metadata-extractor tests/fixtures/valid_samples \
  --profile local \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"
```

**Expected Result**:
- Startup logs:
  `Connecting to MongoDB at mongodb://media_app:***@localhost:27017/media_index_db?authSource=media_index_db...`
- Credentials remain masked on stdout/stderr.
- Ingestion completes with exit code `0`.

---

## 3. Scenario 2: Running with Environment Variable Only (No CLI DB flags)

```bash
export APP_PROFILE=local
export MONGODB_URI="mongodb://media_app:your_actual_password_here@localhost:27017/media_index_db?authSource=media_index_db"

./target/release/metadata-extractor tests/fixtures/valid_samples
```

**Expected Result**:
- System detects `MONGODB_URI` from ambient environment and routes batches to MongoDB.

---

## 4. Scenario 3: Production Profile Enforcement

Run with production profile but omit required password:
```bash
unset MONGODB_PASSWORD

./target/release/metadata-extractor tests/fixtures/valid_samples \
  --profile production \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"
```

**Expected Result**:
- Fails fast in $< 100$ms with exit code `2`.
- Error message: `Configuration error (production profile): MongoDB password is required...`
