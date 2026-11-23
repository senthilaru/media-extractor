use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::metadata_models::MediaMetadataDocument;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartAlbumCriteria {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_start: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_end: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub person_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_objects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_occasions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_query: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<String>,
}

impl Default for SmartAlbumCriteria {
    fn default() -> Self {
        Self {
            date_start: None,
            date_end: None,
            person_ids: Vec::new(),
            required_objects: Vec::new(),
            required_occasions: Vec::new(),
            text_query: None,
            expressions: Vec::new(),
        }
    }
}

/// Represents a virtual, query-driven photo album collection without duplicating files on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartAlbum {
    pub id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub criteria: SmartAlbumCriteria,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_asset_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SmartAlbum {
    pub fn new(
        title: String,
        description: Option<String>,
        criteria: SmartAlbumCriteria,
        asset_ids: Vec<Uuid>,
        cover_asset_id: Option<Uuid>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title,
            description,
            criteria,
            asset_ids,
            cover_asset_id,
            created_at: now,
            updated_at: now,
        }
    }

    /// Selects the best cover asset from candidate media documents based on quality ranking
    pub fn select_cover_photo(candidates: &[MediaMetadataDocument]) -> Option<Uuid> {
        candidates
            .iter()
            .max_by_key(|doc| calculate_cover_score(doc))
            .map(|doc| doc.id)
    }
}

/// Computes an aesthetic and quality rank score for selecting an album cover photo
pub fn calculate_cover_score(doc: &MediaMetadataDocument) -> u32 {
    let mut score = 0u32;

    // Prefer images with faces smiling or happy
    if let Some(ref vis) = doc.visual {
        for face in &vis.faces {
            for expr in &face.expressions {
                let lbl = expr.label.to_lowercase();
                if lbl == "smile" || lbl == "happy" || lbl == "excited" {
                    score += 50;
                }
            }
        }

        // Add points for recognized occasions and objects
        score += (vis.occasions.len() * 10) as u32;
        score += (vis.objects.len() * 5) as u32;
    }

    // Add points for resolution / dimensions
    if let Some(ref dims) = doc.dimensions {
        if dims.width >= 1920 && dims.height >= 1080 {
            score += 20;
        }
    }

    // Add points for good exposure
    if let Some(ref exp) = doc.exposure {
        if exp.iso.map(|iso| iso <= 800).unwrap_or(false) {
            score += 10;
        }
    }

    score
}
