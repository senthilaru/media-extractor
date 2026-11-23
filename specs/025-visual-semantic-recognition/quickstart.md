# Quickstart & Verification Guide: Visual Semantic Recognition

This guide provides end-to-end scenarios to test and validate the visual enrichment, face search, and smart albums capabilities.

---

## 1. Prerequisites

1. MongoDB running locally at `localhost:27017` with credentials from `.env` or defaults.
2. A populated media catalog (or run a quick scan on a sample directory containing photos with people, objects, and text).
3. Built binary in release profile:
   ```bash
   cargo build --release
   ```

---

## 2. Validation Scenario 1: Object & Scene Enrichment (Phase 1)

**Goal**: Verify that physical objects and occasions are extracted from photos and searchable.

1. **Run visual enrichment on target directory**:
   ```bash
   target/release/metadata-extractor enrich --source /Volumes/SP-1/SPSA/media/master/2024 --enable-objects --min-confidence 0.65
   ```
2. **Verify CLI feedback**:
   - Check that the progress bar displays item processing rates (e.g. `15.2 items/sec`) and elapsed duration.
   - Summary indicates detected object count and scene classifications.
3. **Search by object**:
   ```bash
   target/release/metadata-extractor search --object "bicycle"
   target/release/metadata-extractor search --object "teddy_bear"
   ```
   **Expected Outcome**: Returns the file paths of all images containing bicycles or teddy bears.

---

## 3. Validation Scenario 2: Face Recognition & Expression Search (Phase 2)

**Goal**: Detect faces, cluster recurring identities, assign names, and search by emotion.

1. **Extract faces and expressions**:
   ```bash
   target/release/metadata-extractor enrich --source /Volumes/SP-1/SPSA/media/master/2024 --enable-faces
   ```
2. **List detected face clusters**:
   ```bash
   target/release/metadata-extractor faces list --unnamed-only
   ```
   **Expected Output**: Displays clusters with face counts and representative timestamps:
   ```text
   Cluster ID                             Photos  Sample Date
   ------------------------------------  ------  -----------
   a3f81e22-0941-4cb7-91a5-349019b8821a      42  2024-07-14
   b7194cc1-1209-411a-8109-1240188bb010      18  2024-07-14
   ```
3. **Assign human identity to cluster**:
   ```bash
   target/release/metadata-extractor faces name --cluster-id a3f81e22-0941-4cb7-91a5-349019b8821a --name "Maya"
   ```
4. **Search by person and emotion**:
   ```bash
   target/release/metadata-extractor search --person "Maya" --expression "smile"
   ```
   **Expected Outcome**: Returns only pictures containing Maya where a smile/happy expression was detected.

---

## 4. Validation Scenario 3: In-Image Visual Text Search (Phase 3)

**Goal**: Find images by words physically printed on signs, shirts, or party banners.

1. **Extract visual text**:
   ```bash
   target/release/metadata-extractor enrich --source /Volumes/SP-1/SPSA/media/master/2024 --enable-ocr
   ```
2. **Search for words in images**:
   ```bash
   target/release/metadata-extractor search --text "Birthday"
   ```
   **Expected Outcome**: Returns photos with party banners or signs containing the word "Birthday".

---

## 5. Validation Scenario 4: Smart Album Generation (Phase 4)

**Goal**: Synthesize named virtual albums without creating duplicate files on disk.

1. **Generate albums**:
   ```bash
   target/release/metadata-extractor albums generate --year 2024
   ```
   **Expected Output**:
   ```text
   Generated 4 Smart Albums:
   - "2024 - Maya's Birthday Party" (48 assets)
   - "2024 - Cycling Trip" (32 assets)
   - "2024 - Halloween Gathering" (65 assets)
   - "2024 - Christmas Celebration" (94 assets)
   ```
2. **Verify single-copy disk invariant**:
   - Verify that disk space usage on `/Volumes/SP-1` has not changed (zero new media files copied).
   - Verify that album records in MongoDB contain references to existing canonical master paths.
