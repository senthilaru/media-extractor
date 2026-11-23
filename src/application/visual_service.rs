use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use chrono::Utc;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::errors::ExtractionError;
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::ports::{MetadataStorageSink, VisualInferencePort};
use crate::domain::visual_models::{
    DetectedFace, DetectedObject, DetectedOccasion, ExtractedTextSnippet,
    VisualSemanticMetadata,
};

#[derive(Debug, Clone)]
pub struct EnrichOptions {
    pub source: Option<PathBuf>,
    pub enable_objects: bool,
    pub enable_faces: bool,
    pub enable_ocr: bool,
    pub min_confidence: f32,
    pub batch_size: usize,
    pub force: bool,
    pub dry_run: bool,
}

impl Default for EnrichOptions {
    fn default() -> Self {
        Self {
            source: None,
            enable_objects: true,
            enable_faces: true,
            enable_ocr: true,
            min_confidence: 0.60,
            batch_size: 50,
            force: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnrichmentReport {
    pub total_candidates: usize,
    pub total_enriched: usize,
    pub objects_detected: usize,
    pub occasions_detected: usize,
    pub faces_detected: usize,
    pub ocr_snippets_extracted: usize,
    pub elapsed_seconds: f64,
}

pub struct VisualEnrichmentService {
    storage: Arc<dyn MetadataStorageSink>,
    vision: Arc<dyn VisualInferencePort>,
}

impl VisualEnrichmentService {
    pub fn new(
        storage: Arc<dyn MetadataStorageSink>,
        vision: Arc<dyn VisualInferencePort>,
    ) -> Self {
        Self { storage, vision }
    }

    pub async fn execute_enrichment(
        &self,
        options: &EnrichOptions,
    ) -> Result<EnrichmentReport, ExtractionError> {
        let start_time = Instant::now();
        info!("Beginning visual semantic enrichment run: {:?}", options);

        // Fetch candidate documents matching criteria: if source prefix is given, query by prefix.
        // Otherwise, fetch active catalog documents.
        let pool = if let Some(ref prefix) = options.source {
            let prefix_str = prefix.to_string_lossy();
            let mut docs = self
                .storage
                .query_by_source_path_prefix(&prefix_str)
                .await
                .map_err(|e| ExtractionError::InferenceFailed {
                    path: PathBuf::new(),
                    message: format!("Failed to query documents by source prefix: {e}"),
                })?;
            if docs.is_empty() {
                docs = self.storage.query_catalog_for_visual().await.map_err(|e| {
                    ExtractionError::InferenceFailed {
                        path: PathBuf::new(),
                        message: format!("Failed to query catalog for visual: {e}"),
                    }
                })?;
            }
            docs
        } else {
            self.storage
                .query_catalog_for_visual()
                .await
                .map_err(|e| ExtractionError::InferenceFailed {
                    path: PathBuf::new(),
                    message: format!("Failed to query catalog for visual: {e}"),
                })?
        };

        // Filter candidates based on source path prefix and existing visual status
        let candidates: Vec<MediaMetadataDocument> = pool
            .into_iter()
            .filter(|doc| {
                if let Some(ref prefix) = options.source {
                    let doc_str = doc.file_context.file_path.to_string_lossy();
                    let prefix_str = prefix.to_string_lossy();
                    if !doc_str.starts_with(prefix_str.as_ref())
                        && !doc_str.to_lowercase().starts_with(&prefix_str.to_lowercase())
                    {
                        return false;
                    }
                }
                if !options.force && doc.visual.is_some() {
                    return false;
                }
                // Only process accessible existing files
                doc.file_context.file_path.exists()
            })
            .collect();

        let total_candidates = candidates.len();
        if total_candidates == 0 {
            info!("No un-enriched media assets found matching criteria.");
            return Ok(EnrichmentReport::default());
        }

        let pb = ProgressBar::new(total_candidates as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message("Enriching visual features...");

        let mut report = EnrichmentReport {
            total_candidates,
            ..Default::default()
        };

        let batch_size = options.batch_size.max(1);
        for chunk in candidates.chunks(batch_size) {
            let paths: Vec<PathBuf> = chunk.iter().map(|d| d.file_context.file_path.clone()).collect();
            let mut chunk_visuals: Vec<(Uuid, VisualSemanticMetadata)> = Vec::with_capacity(chunk.len());

            // Initialize visual metadata mapping
            let mut visual_map: Vec<(Uuid, PathBuf, VisualSemanticMetadata)> = chunk
                .iter()
                .map(|doc| {
                    let initial_meta = doc.visual.clone().unwrap_or_else(|| VisualSemanticMetadata {
                        model_version: "1.0.0".to_string(),
                        analyzed_at: Utc::now(),
                        objects: Vec::new(),
                        occasions: Vec::new(),
                        faces: Vec::new(),
                        text_snippets: Vec::new(),
                        search_tokens: Vec::new(),
                    });
                    (doc.id, doc.file_context.file_path.clone(), initial_meta)
                })
                .collect();

            // 1. Infer Objects & Scenes if enabled
            if options.enable_objects {
                match self.vision.infer_objects_and_scenes(&paths).await {
                    Ok(obj_results) => {
                        for (path, objs, occs) in obj_results {
                            let filtered_objs: Vec<DetectedObject> = objs
                                .into_iter()
                                .filter(|o| o.confidence >= options.min_confidence)
                                .collect();
                            let filtered_occs: Vec<DetectedOccasion> = occs
                                .into_iter()
                                .filter(|occ| occ.confidence >= options.min_confidence)
                                .collect();

                            report.objects_detected += filtered_objs.len();
                            report.occasions_detected += filtered_occs.len();

                            if let Some((_, _, meta)) = visual_map.iter_mut().find(|(_, p, _)| p == &path) {
                                meta.objects = filtered_objs;
                                meta.occasions = filtered_occs;
                            }
                        }
                    }
                    Err(e) => warn!("Object/scene batch inference warning: {}", e),
                }
            }

            // 2. Infer Faces & Expressions if enabled
            if options.enable_faces {
                match self.vision.infer_faces(&paths).await {
                    Ok(face_results) => {
                        for (path, faces) in face_results {
                            let filtered_faces: Vec<DetectedFace> = faces
                                .into_iter()
                                .filter(|f| f.confidence >= options.min_confidence)
                                .collect();

                            report.faces_detected += filtered_faces.len();

                            if let Some((_, _, meta)) = visual_map.iter_mut().find(|(_, p, _)| p == &path) {
                                meta.faces = filtered_faces;
                            }
                        }
                    }
                    Err(e) => warn!("Face batch inference warning: {}", e),
                }
            }

            // 3. Infer OCR Text if enabled
            if options.enable_ocr {
                match self.vision.infer_ocr(&paths).await {
                    Ok(ocr_results) => {
                        for (path, snippets, tokens) in ocr_results {
                            let filtered_snippets: Vec<ExtractedTextSnippet> = snippets
                                .into_iter()
                                .filter(|s| s.confidence >= options.min_confidence)
                                .collect();

                            report.ocr_snippets_extracted += filtered_snippets.len();

                            if let Some((_, _, meta)) = visual_map.iter_mut().find(|(_, p, _)| p == &path) {
                                meta.text_snippets = filtered_snippets;
                                for token in tokens {
                                    if !meta.search_tokens.contains(&token) {
                                        meta.search_tokens.push(token);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => warn!("OCR batch inference warning: {}", e),
                }
            }

            for (doc_id, _, meta) in visual_map {
                chunk_visuals.push((doc_id, meta));
            }

            // Persist batch to MongoDB (unless dry run)
            if !options.dry_run {
                if let Err(e) = self.storage.update_visual_metadata_batch(&chunk_visuals).await {
                    warn!("Failed to persist visual metadata batch: {}", e);
                }
            }

            report.total_enriched += chunk.len();
            pb.inc(chunk.len() as u64);
        }

        pb.finish_with_message("Visual semantic enrichment completed.");
        report.elapsed_seconds = start_time.elapsed().as_secs_f64();
        Ok(report)
    }
}
