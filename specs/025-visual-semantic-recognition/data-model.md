# Data Model: Visual Semantic Recognition, Face Search & Smart Albums

## Overview

This document specifies the domain entities, value objects, database schema extensions, and validation rules for visual semantic enrichment, facial recognition, OCR text indexing, and virtual smart albums.

### Core Invariant: Non-Destructive Media Immutability
- **Zero In-Place Modifications**: Canonical master media files (JPEG, HEIC, PNG, RAW, TIFF, videos) are strictly read-only and immutable.
- **Externalized Semantic Facets**: All visual labels, face coordinates, embeddings, emotions, OCR text, and album associations are stored strictly as external facets in MongoDB, never written into the binary headers or byte payloads of original media files.
- **Derivative Lineage for AI Edits**: Any future image editing or generative transformations must create new derivative files in a separate edits folder, retaining the original master in pristine, untouched condition with parent-child linkage.

---

## 1. Document Extension: `VisualSemanticMetadata`

The existing `MediaMetadataDocument` is extended with an optional field:
```rust
pub visual: Option<VisualSemanticMetadata>
```

### Entity Fields

```rust
/// Comprehensive visual semantic metadata extracted from image content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualSemanticMetadata {
    /// Schema/model version used for extraction (e.g., "1.0.0")
    pub model_version: String,
    
    /// Timestamp when visual extraction was executed
    pub analyzed_at: DateTime<Utc>,
    
    /// List of detected physical objects in the image
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<DetectedObject>,
    
    /// Recognized scene types and celebration/event occasions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occasions: Vec<DetectedOccasion>,
    
    /// Detected human faces with localized expressions and person cluster associations
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<DetectedFace>,
    
    /// Extracted in-image text blocks and word tokens
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_snippets: Vec<ExtractedTextSnippet>,
    
    /// Aggregated token array for fast full-text searching
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_tokens: Vec<String>,
}
```

---

## 2. Supporting Value Objects

### `DetectedObject`
Represents an object identified by computer vision (e.g. teddy bear, bicycle, phone, tv).

| Field | Type | Description | Validation |
| :--- | :--- | :--- | :--- |
| `label` | `String` | Normalized lower-case object class (e.g. `"teddy_bear"`, `"bicycle"`) | Required, non-empty |
| `confidence` | `f32` | Detection confidence probability score | Range: `0.0 ..= 1.0` |
| `bounding_box` | `Option<BoundingBox>` | Normalized coordinates `[x_min, y_min, x_max, y_max]` | Values in `0.0 ..= 1.0` |

### `DetectedOccasion`
Represents a recognized scene context or social event.

| Field | Type | Description | Validation |
| :--- | :--- | :--- | :--- |
| `category` | `String` | Occasion type (e.g. `"birthday"`, `"christmas"`, `"halloween"`, `"party"`, `"vacation"`) | Required |
| `confidence` | `f32` | Classification confidence score | Range: `0.0 ..= 1.0` |

### `DetectedFace`
Represents a detected face instance localized within the photo.

| Field | Type | Description | Validation |
| :--- | :--- | :--- | :--- |
| `face_id` | `Uuid` | Unique identifier for this face occurrence | Required, UUID v4 |
| `bounding_box` | `BoundingBox` | Spatial boundary in image coordinates | Normalized `0.0 ..= 1.0` |
| `confidence` | `f32` | Face detection confidence | Range: `0.0 ..= 1.0` |
| `expressions` | `Vec<FacialExpression>` | Dominant facial expressions with confidence | Sorted descending by score |
| `person_id` | `Option<Uuid>` | Assigned `PersonCluster` identifier | References `PersonCluster.id` |
| `embedding` | `Option<Vec<f32>>` | 512-dimensional normalized face feature vector | Exactly 512 floats (if present) |

### `FacialExpression`
Represents an emotional expression classification.

| Field | Type | Description | Example Values |
| :--- | :--- | :--- | :--- |
| `label` | `ExpressionLabel` | Categorical emotion name | `Smile`, `Happy`, `Sad`, `Crying`, `Excited`, `Neutral` |
| `score` | `f32` | Probability score | Range: `0.0 ..= 1.0` |

### `ExtractedTextSnippet`
Represents readable text discovered via optical character recognition.

| Field | Type | Description | Validation |
| :--- | :--- | :--- | :--- |
| `text` | `String` | Raw recognized text phrase | Non-empty |
| `tokens` | `Vec<String>` | Normalized lowercase word tokens | Stripped of punctuation |
| `confidence` | `f32` | OCR confidence score | Range: `0.0 ..= 1.0` |
| `bounding_box` | `Option<BoundingBox>` | Spatial boundary of the text block | Normalized `0.0 ..= 1.0` |

---

## 3. Dedicated Collection: `PersonCluster`

Represents an aggregated identity representing a recurring individual across the photo catalog.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonCluster {
    /// Unique identity UUID
    pub id: Uuid,
    
    /// User-assigned display name (e.g. "Maya", "Grandpa")
    pub display_name: Option<String>,
    
    /// Representative face crop or master document ID for avatar display
    pub avatar_asset_id: Option<Uuid>,
    
    /// Centroid 512-d feature vector representing the average identity embedding
    pub centroid_embedding: Vec<f32>,
    
    /// Total count of media items featuring this person
    pub total_photo_count: u32,
    
    /// Timestamp when this cluster was established
    pub created_at: DateTime<Utc>,
    
    /// Timestamp when this identity was last updated
    pub updated_at: DateTime<Utc>,
}
```

---

## 4. Dedicated Collection: `SmartAlbum`

Represents a virtual, query-driven photo album collection.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartAlbum {
    /// Unique album UUID
    pub id: Uuid,
    
    /// Title of the album (e.g. "2024 - Maya's Birthday Party")
    pub title: String,
    
    /// User or system description
    pub description: Option<String>,
    
    /// Rule criteria that define membership in this smart album
    pub criteria: SmartAlbumCriteria,
    
    /// Cached list of canonical master document IDs belonging to this album
    pub asset_ids: Vec<Uuid>,
    
    /// Designated cover photo asset ID
    pub cover_asset_id: Option<Uuid>,
    
    /// Creation and update timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartAlbumCriteria {
    pub date_start: Option<DateTime<Utc>>,
    pub date_end: Option<DateTime<Utc>>,
    pub person_ids: Vec<Uuid>,
    pub required_objects: Vec<String>,
    pub required_occasions: Vec<String>,
    pub text_query: Option<String>,
    pub expressions: Vec<String>,
}
```

---

## 5. MongoDB Indexes

To guarantee sub-500ms multi-modal searches across 100,000+ assets:

```javascript
// 1. Array index on detected object classes
db.media_assets.createIndex({ "visual.objects.label": 1 });

// 2. Compound index on person identity and year
db.media_assets.createIndex({ "visual.faces.person_id": 1, "temporal.year": 1 });

// 3. Multikey index on facial expressions
db.media_assets.createIndex({ "visual.faces.expressions.label": 1 });

// 4. Multikey index on occasion categories
db.media_assets.createIndex({ "visual.occasions.category": 1 });

// 5. Full-text search index on search tokens and OCR words
db.media_assets.createIndex({
  "visual.search_tokens": "text",
  "visual.text_snippets.text": "text"
});
```
