# Quickstart & Validation Guide: Incremental Media Scan

**Feature**: `008-incremental-media-scan`  
**Date**: 2026-11-09  
**Status**: Ready for Validation  

---

## 1. Overview

This guide details validation scenarios proving that:
1. Initial scan extracts and indexes all files.
2. Immediate re-scan against the same directory skips 100% of unmodified files in seconds.
3. Adding a new file or modifying an existing file triggers extraction only for the modified items.
4. Using `--force` forces full re-extraction regardless of database state.

---

## 2. Validation Scenarios

### Scenario 1: Re-Scan Skipped Files in Seconds
```bash
# 1. Run initial scan into database
./target/release/metadata-extractor /spsa/temp --fast

# 2. Run the exact same scan command immediately
./target/release/metadata-extractor /spsa/temp --fast
```

**Expected Outcome**:
- Second run completes in $< 5$ seconds.
- Summary reports:
  `Items Skipped: 13,420 (Already indexed & unmodified)`
  `Items Extracted: 0`
- Zero unnecessary disk reads or container parsing.

---

### Scenario 2: Detecting New and Modified Files
```bash
# Add a new photo and update a timestamp on an existing photo in /spsa/temp
touch /spsa/temp/new_photo.jpg
touch /spsa/temp/existing_photo.jpg

# Run scan again
./target/release/metadata-extractor /spsa/temp --fast
```

**Expected Outcome**:
- Only `new_photo.jpg` and `existing_photo.jpg` undergo extraction.
- All other 13,420 files are skipped.
- Database records updated for modified and new items.

---

### Scenario 3: Force Re-Extraction
```bash
# Force re-scan of entire archive
./target/release/metadata-extractor /spsa/temp --fast --force
```

**Expected Outcome**:
- All 13,422 files undergo full extraction.
- Summary reports 0 skipped items.
