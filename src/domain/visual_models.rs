use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Normalized bounding box coordinates [x_min, y_min, x_max, y_max] in range 0.0..=1.0
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

impl BoundingBox {
    pub fn new(x_min: f32, y_min: f32, x_max: f32, y_max: f32) -> Self {
        Self {
            x_min: x_min.clamp(0.0, 1.0),
            y_min: y_min.clamp(0.0, 1.0),
            x_max: x_max.clamp(0.0, 1.0),
            y_max: y_max.clamp(0.0, 1.0),
        }
    }

    pub fn from_array(arr: [f32; 4]) -> Self {
        Self::new(arr[0], arr[1], arr[2], arr[3])
    }

    pub fn from_slice(coords: &[f32]) -> Option<Self> {
        if coords.len() == 4 {
            Some(Self::new(coords[0], coords[1], coords[2], coords[3]))
        } else {
            None
        }
    }
}

/// A physical object identified by computer vision (e.g., teddy bear, bicycle, phone, tv)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedObject {
    pub label: String,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<BoundingBox>,
}

/// A recognized scene type or celebration/event occasion (e.g., birthday, christmas, party)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedOccasion {
    pub category: String,
    pub confidence: f32,
}

/// Categorical emotional expression classification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FacialExpression {
    pub label: String,
    pub score: f32,
}

/// A detected human face instance with emotional expression and identity vector
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedFace {
    pub face_id: Uuid,
    pub bounding_box: BoundingBox,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<FacialExpression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

/// Readable text discovered via optical character recognition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedTextSnippet {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<String>,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<BoundingBox>,
}

/// Comprehensive visual semantic metadata extracted from image content
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualSemanticMetadata {
    pub model_version: String,
    pub analyzed_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<DetectedObject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occasions: Vec<DetectedOccasion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<DetectedFace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_snippets: Vec<ExtractedTextSnippet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_tokens: Vec<String>,
}

impl Default for VisualSemanticMetadata {
    fn default() -> Self {
        Self {
            model_version: "1.0.0".to_string(),
            analyzed_at: Utc::now(),
            objects: Vec::new(),
            occasions: Vec::new(),
            faces: Vec::new(),
            text_snippets: Vec::new(),
            search_tokens: Vec::new(),
        }
    }
}
