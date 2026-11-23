# Research: Visual Semantic Recognition, Face Search, and Smart Albums

## Architectural Decisions & Research Findings

### 1. Local Inference Architecture & Rust Integration Strategy

- **Decision**: Abstract visual semantic extraction behind an extensible Domain Port (`VisualInferencePort` / `VisualFeatureExtractor`), with an initial high-throughput on-device inference adapter utilizing ONNX Runtime (`ort` crate) and macOS Apple Silicon Vision Framework integration where native CoreML acceleration is available.
- **Rationale**:
  - Compliance with Constitution Principle I (M1 Silicon Concurrency) and Principle VI (Zero External Data Leakage). All visual processing executes 100% locally on-device.
  - Decoupling inference behind DDD traits ensures that model weights, runtime engines (ONNX Runtime, CoreML, Tract, or TensorRT), and hardware accelerators can be swapped without modifying domain entities or catalog workflows.
  - The `ort` crate supports Execution Providers for CoreML / Apple Neural Engine on macOS M1, providing sub-50ms inference per photo without consuming excessive CPU.
- **Alternatives Considered**:
  - *Cloud Vision APIs (Google Cloud Vision, AWS Rekognition)*: Strictly rejected due to user privacy, offline operational requirements, network bandwidth constraints for terabyte libraries, and ongoing API costs.
  - *Pure Python subprocess orchestration*: Rejected due to high serialization overhead, process management fragility, and violation of the project's single-binary high-performance Rust architecture.
  - *`tract` (pure Rust ONNX runner)*: Tract is lightweight but lacks native GPU/ANE hardware acceleration for complex multi-model pipelines (Face + YOLO + OCR).

---

### 2. Multi-Model Pipeline Separation & Phased Extraction

- **Decision**: Structure the visual pipeline into modular, independently executable extractors corresponding to the 4 spec phases:
  1. `ObjectSceneExtractor`: Detects 80+ standard object classes (COCO ontology: teddy bear, bicycle, phone, laptop, tv, remote, flowers) and scene contexts (party, holiday, outdoor, birthday).
  2. `FaceAndExpressionExtractor`: Detects face bounding boxes, extracts 512-dimensional normalized face embeddings (e.g., MobileFaceNet / ArcFace), and predicts expression probabilities (smile, happy, sad, crying, neutral).
  3. `TextOcrExtractor`: Performs text line detection and recognition (OCR) on image crops, emitting word tokens and bounding boxes.
  4. `AlbumSynthesisService`: Rules engine and clustering aggregator that queries enriched catalog records to generate named virtual albums.
- **Rationale**:
  - Follows Interface Segregation and Single Responsibility principles. Users can run `--enrich-objects`, `--enrich-faces`, or `--enrich-text` independently or combined.
  - Compute-heavy stages (like high-res OCR or deep face embeddings) can be skipped or deferred for fast initial indexing.
- **Alternatives Considered**:
  - *Monolithic single-pass model*: While multimodal vision-language models (e.g. CLIP/LLaVA) exist, running a 4GB+ LLM on tens of thousands of photos is too slow for batch ingestion on commodity hardware, whereas dedicated specialized lightweight models (YOLOv8-nano + MobileFaceNet + DBNet/CRNN) run concurrently at 30–60 images/second on Apple Silicon.

---

### 3. Face Embedding Representation & Clustering Algorithm

- **Decision**:
  - Store a normalized 512-element floating-point vector (`Vec<f32>`) per detected face.
  - Implement incremental agglomerative clustering / DBSCAN with cosine distance metric in `src/domain/person_cluster.rs`.
  - A threshold distance (default: cosine distance $\le 0.38$) associates a face with an existing `PersonCluster`. Faces that cannot be matched above confidence form a new tentative cluster awaiting user naming.
- **Rationale**:
  - Cosine distance on normalized embeddings provides robust face verification across aging, angles, and lighting.
  - Clustering in memory over database-persisted embeddings allows offline reconciliation and instant re-clustering when the user merges or splits identities.
- **Alternatives Considered**:
  - *K-Means clustering*: Rejected because the total number of distinct people ($K$) in a photo archive is unknown a priori and grows continuously over time.
  - *Strict hash-based matching*: Rejected because facial appearance varies across poses and lighting, requiring metric embedding spaces rather than exact keys.

---

### 4. Smart Album Synthesis & Zero Storage Inflation Invariant

- **Decision**:
  - Smart albums MUST be purely virtual entities persisted in a dedicated MongoDB collection (`smart_albums`) or represented as queryable collection manifests.
  - A `SmartAlbum` document contains:
    - `id`: Unique UUID
    - `name`: Generated or user-edited title (e.g., `"2024 - Maya's 5th Birthday Party"`)
    - `criteria`: Structured query definition (temporal range, required person IDs, occasion tag, required objects, location radius)
    - `asset_ids`: Array of referenced canonical master UUIDs
    - `cover_asset_id`: Best quality photo chosen based on `MediaQualityRank`
  - Under no circumstances does album synthesis move, copy, or duplicate physical media files on the file system, preserving the single-copy canonical master invariant established in Spec 010 and Spec 024.
- **Rationale**:
  - Preserves disk space across multi-terabyte drives. A single photo can belong to multiple albums (e.g., `"Family Vacation 2023"`, `"Liam's Childhood"`, `"Bicycle Rides"`) with zero extra bytes on disk.
- **Alternatives Considered**:
  - *Filesystem symlinks / hardlinks*: Fragile across external drives and exFAT/FAT32 filesystems which do not support POSIX symlinks or hardlinks.

---

### 5. Multi-Modal Search Indexing Strategy

- **Decision**: Leverage compound text indexing and array multikey indexing in MongoDB:
  - Text index over `visual.text_tokens`, `visual.objects.label`, `visual.scenes.label`, and `visual.detected_faces.person_name`.
  - Filter facets on `visual.detected_faces.expressions`, `temporal.year`, and `camera.make`.
  - Combine textual full-text query with structured MongoDB `$match` filters to support combined queries (e.g., `{ "visual.detected_faces.person_name": "Maya", "visual.objects.label": "bicycle", "temporal.year": 2024 }`).
- **Rationale**:
  - Provides sub-50ms search latency without introducing external search cluster infrastructure (e.g. Elasticsearch).
- **Alternatives Considered**:
  - *External vector database (Milvus / Qdrant)*: Overkill for local CLI/daemon deployments with <1M photos, adding unnecessary operational dependencies. MongoDB multikey + compound indexes satisfy the performance criteria cleanly.

---

### 6. Strict Non-Destructive Operations & Master Immutability Invariant

- **Decision**: Enforce an inviolable non-destructive policy across all visual semantic extraction, facial recognition, OCR indexing, smart album creation, and future AI image modifications:
  1. **Read-Only Media Access**: The extraction pipeline opens media files in strictly read-only mode (`fs::File::open`), never obtaining write handles or re-encoding container streams.
  2. **Zero In-Place Metadata Infiltration**: No EXIF, XMP, IPTC, or visual tag data is ever written directly into the binary payload of original master files. All semantic attributes exist exclusively as external metadata facets in MongoDB or optional sidecar documents.
  3. **Derivative Model for Future Image Edits**: Any upcoming image editing (e.g. AI upscaling, inpainting, background removal) must output a new derivative asset file in a designated edits/derivative directory, preserving the original canonical master in its pristine, bit-for-bit identical state with full cryptographic verification (`BLAKE3`).
- **Rationale**:
  - Compliance with Core Constitution Principles: Principle I (Safe I/O), Principle II (Domain Purity), and Principle V (Fault Isolation).
  - Protects user archives against irrecoverable data loss, accidental re-compression degradation, EXIF corruption, and unintended file modifications across external storage drives.
- **Alternatives Considered**:
  - *In-place EXIF tagging (e.g. writing visual keywords directly into JPEG/HEIC headers)*: Strictly rejected due to risk of bit rot, corruption of proprietary camera RAW maker notes, and invalidating existing BLAKE3 cryptographic hashes.
