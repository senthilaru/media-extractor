use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use async_trait::async_trait;
use tracing::{error, warn};
use uuid::Uuid;

use crate::domain::errors::ExtractionError;
use crate::domain::ports::VisualInferencePort;
use crate::domain::visual_models::{
    BoundingBox, DetectedFace, DetectedObject, DetectedOccasion, ExtractedTextSnippet,
    FacialExpression,
};
use crate::infrastructure::ai_client::AiSidecarClient;

/// Strictly verifies that a media file is opened in READ-ONLY mode, ensuring
/// zero write/modify/truncate operations can happen to the original canonical master.
pub struct ReadOnlyFileGuard;

impl ReadOnlyFileGuard {
    /// Validates that the file exists and can be opened strictly for reading.
    /// Under no circumstance will write permissions be requested.
    pub fn verify_read_only(path: &Path) -> Result<std::fs::File, ExtractionError> {
        if !path.exists() {
            return Err(ExtractionError::FileNotFound(path.to_path_buf()));
        }

        OpenOptions::new()
            .read(true)
            .write(false)
            .create(false)
            .truncate(false)
            .open(path)
            .map_err(|e| {
                error!("Read-only access check failed for {}: {}", path.display(), e);
                ExtractionError::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
            })
    }
}

pub struct VisionInferenceAdapter {
    client: AiSidecarClient,
}

impl VisionInferenceAdapter {
    pub fn new(client: AiSidecarClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl VisualInferencePort for VisionInferenceAdapter {
    async fn is_available(&self) -> bool {
        self.client.check_health().await.unwrap_or(false)
    }

    async fn infer_objects_and_scenes(
        &self,
        file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<DetectedObject>, Vec<DetectedOccasion>)>, ExtractionError> {
        // Enforce read-only check on all input paths before IPC dispatch
        let valid_paths: Vec<PathBuf> = file_paths
            .iter()
            .filter(|p| {
                if let Err(e) = ReadOnlyFileGuard::verify_read_only(p) {
                    warn!("Skipping unreadable file {}: {}", p.display(), e);
                    false
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        if valid_paths.is_empty() {
            return Ok(Vec::new());
        }

        let resp_list = self
            .client
            .enrich_objects_scenes(&valid_paths)
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: valid_paths.first().cloned().unwrap_or_default(),
                message: format!("AI object/scene inference failed: {e}"),
            })?;

        let mut results = Vec::with_capacity(resp_list.len());
        for item in resp_list {
            let path = PathBuf::from(item.file_path);
            let objects = item
                .objects
                .into_iter()
                .map(|o| DetectedObject {
                    label: o.label.to_lowercase(),
                    confidence: o.confidence.clamp(0.0, 1.0),
                    bounding_box: o.box_coordinates.map(BoundingBox::from_array),
                })
                .collect();

            let occasions = item
                .occasions
                .into_iter()
                .map(|occ| DetectedOccasion {
                    category: occ.tag.to_lowercase(),
                    confidence: occ.confidence.clamp(0.0, 1.0),
                })
                .collect();

            results.push((path, objects, occasions));
        }

        Ok(results)
    }

    async fn infer_faces(
        &self,
        file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<DetectedFace>)>, ExtractionError> {
        let valid_paths: Vec<PathBuf> = file_paths
            .iter()
            .filter(|p| ReadOnlyFileGuard::verify_read_only(p).is_ok())
            .cloned()
            .collect();

        if valid_paths.is_empty() {
            return Ok(Vec::new());
        }

        let resp_list = self
            .client
            .enrich_faces(&valid_paths)
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: valid_paths.first().cloned().unwrap_or_default(),
                message: format!("AI face inference failed: {e}"),
            })?;

        let mut results = Vec::with_capacity(resp_list.len());
        for item in resp_list {
            let path = PathBuf::from(item.file_path);
            let faces = item
                .faces
                .into_iter()
                .map(|f| DetectedFace {
                    face_id: Uuid::new_v4(),
                    bounding_box: BoundingBox::from_array(f.box_coordinates),
                    confidence: f.confidence.clamp(0.0, 1.0),
                    expressions: vec![FacialExpression {
                        label: f.expression.to_lowercase(),
                        score: f.expression_score.clamp(0.0, 1.0),
                    }],
                    person_id: None,
                    embedding: if f.embedding.len() == 512 {
                        Some(f.embedding)
                    } else {
                        None
                    },
                })
                .collect();

            results.push((path, faces));
        }

        Ok(results)
    }

    async fn infer_ocr(
        &self,
        file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<ExtractedTextSnippet>, Vec<String>)>, ExtractionError> {
        let valid_paths: Vec<PathBuf> = file_paths
            .iter()
            .filter(|p| ReadOnlyFileGuard::verify_read_only(p).is_ok())
            .cloned()
            .collect();

        if valid_paths.is_empty() {
            return Ok(Vec::new());
        }

        let resp_list = self
            .client
            .enrich_ocr(&valid_paths)
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: valid_paths.first().cloned().unwrap_or_default(),
                message: format!("AI OCR inference failed: {e}"),
            })?;

        let mut results = Vec::with_capacity(resp_list.len());
        for item in resp_list {
            let path = PathBuf::from(item.file_path);
            let snippets = item
                .text_snippets
                .into_iter()
                .map(|s| {
                    let tokens: Vec<String> = s
                        .text
                        .split_whitespace()
                        .map(|w| {
                            w.trim_matches(|c: char| !c.is_alphanumeric())
                                .to_lowercase()
                        })
                        .filter(|w| !w.is_empty())
                        .collect();

                    ExtractedTextSnippet {
                        text: s.text,
                        tokens,
                        confidence: s.confidence.clamp(0.0, 1.0),
                        bounding_box: s.box_coordinates.map(BoundingBox::from_array),
                    }
                })
                .collect();

            let search_tokens = item
                .search_tokens
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect();

            results.push((path, snippets, search_tokens));
        }

        Ok(results)
    }
}
