use std::path::PathBuf;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::domain::errors::{ExtractionError, StorageError};
use crate::domain::person_cluster::{PersonCluster, FACE_CLUSTER_THRESHOLD_COSINE_DISTANCE};
use crate::domain::ports::MetadataStorageSink;
use crate::domain::visual_models::BoundingBox;

/// Represents a photo matching a person cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterPhotoMatch {
    pub file_path: PathBuf,
    pub doc_id: Uuid,
    pub date: Option<String>,
    pub face_confidence: f32,
    pub expression: Option<String>,
    pub expression_score: Option<f32>,
    pub bounding_box: BoundingBox,
}

/// Comprehensive details of a person cluster and its matching media assets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterDetails {
    pub cluster: PersonCluster,
    pub top_photos: Vec<ClusterPhotoMatch>,
    pub total_photos: usize,
}

pub struct FaceManagementService {
    storage: Arc<dyn MetadataStorageSink>,
}

impl FaceManagementService {
    pub fn new(storage: Arc<dyn MetadataStorageSink>) -> Self {
        Self { storage }
    }

    /// Clusters detected faces across cataloged media assets
    pub async fn cluster_faces(&self) -> Result<usize, ExtractionError> {
        info!("Discovering and clustering unassigned faces...");

        let masters = self
            .storage
            .query_catalog_for_visual()
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: PathBuf::new(),
                message: format!("Failed querying canonical masters: {e}"),
            })?;

        let mut existing_clusters = self
            .storage
            .list_person_clusters()
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: PathBuf::new(),
                message: format!("Failed listing person clusters: {e}"),
            })?;

        let mut doc_updates = Vec::new();
        let mut new_clusters_created = 0;

        for doc in masters {
            if let Some(mut visual) = doc.visual {
                let mut modified = false;

                for face in &mut visual.faces {
                    if face.person_id.is_none() {
                        if let Some(ref emb) = face.embedding {
                            // Find closest matching cluster within threshold
                            let mut best_match: Option<(usize, f32)> = None;
                            for (idx, cluster) in existing_clusters.iter().enumerate() {
                                let dist = cluster.cosine_distance(emb);
                                if dist <= FACE_CLUSTER_THRESHOLD_COSINE_DISTANCE {
                                    match best_match {
                                        None => best_match = Some((idx, dist)),
                                        Some((_, best_dist)) if dist < best_dist => {
                                            best_match = Some((idx, dist));
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            if let Some((idx, _)) = best_match {
                                existing_clusters[idx].incorporate_face(emb);
                                face.person_id = Some(existing_clusters[idx].id);
                                modified = true;
                            } else {
                                // Create new cluster
                                let new_cluster = PersonCluster::new(emb.clone(), Some(doc.id));
                                face.person_id = Some(new_cluster.id);
                                existing_clusters.push(new_cluster);
                                new_clusters_created += 1;
                                modified = true;
                            }
                        }
                    }
                }

                if modified {
                    doc_updates.push((doc.id, visual));
                }
            }
        }

        // Persist all clusters
        for cluster in &existing_clusters {
            let _ = self.storage.save_person_cluster(cluster).await;
        }

        // Persist face assignments
        if !doc_updates.is_empty() {
            let _ = self.storage.update_visual_metadata_batch(&doc_updates).await;
        }

        info!(
            "Clustering completed: {} new person clusters created, {} documents updated.",
            new_clusters_created,
            doc_updates.len()
        );

        Ok(new_clusters_created)
    }

    /// Lists established person clusters
    pub async fn list_clusters(
        &self,
        unnamed_only: bool,
        min_photos: Option<usize>,
    ) -> Result<Vec<PersonCluster>, StorageError> {
        // Automatically cluster any newly extracted faces in the catalog
        let _ = self.cluster_faces().await;
        let clusters = self.storage.list_person_clusters().await?;
        let filtered = clusters
            .into_iter()
            .filter(|c| {
                if unnamed_only && c.display_name.is_some() {
                    return false;
                }
                if let Some(min) = min_photos {
                    if (c.total_photo_count as usize) < min {
                        return false;
                    }
                }
                true
            })
            .collect();

        Ok(filtered)
    }

    /// Assigns a human name to a cluster ID
    pub async fn name_cluster(&self, cluster_id: Uuid, name: &str) -> Result<bool, StorageError> {
        self.storage.update_person_name(cluster_id, name).await
    }

    /// Merges source cluster into target cluster
    pub async fn merge_clusters(&self, source_id: Uuid, target_id: Uuid) -> Result<bool, StorageError> {
        self.storage.merge_person_clusters(source_id, target_id).await
    }

    /// Retrieves matching photo assets for a specific person cluster, sorted by face confidence descending
    pub async fn get_cluster_photos(
        &self,
        cluster_id: Uuid,
        limit: Option<usize>,
    ) -> Result<Option<ClusterDetails>, ExtractionError> {
        // Automatically cluster any newly extracted faces in the catalog
        let _ = self.cluster_faces().await;

        let clusters = self.storage.list_person_clusters().await.map_err(|e| {
            ExtractionError::InferenceFailed {
                path: PathBuf::new(),
                message: format!("Failed listing clusters: {e}"),
            }
        })?;

        let cluster = match clusters.into_iter().find(|c| c.id == cluster_id) {
            Some(c) => c,
            None => return Ok(None),
        };

        let masters = self
            .storage
            .query_catalog_for_visual()
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: PathBuf::new(),
                message: format!("Failed querying canonical masters: {e}"),
            })?;

        let mut matches = Vec::new();

        for doc in masters {
            if let Some(ref visual) = doc.visual {
                for face in &visual.faces {
                    if face.person_id == Some(cluster_id) {
                        let top_expression = face
                            .expressions
                            .iter()
                            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));

                        matches.push(ClusterPhotoMatch {
                            file_path: doc.file_context.file_path.clone(),
                            doc_id: doc.id,
                            date: Some(doc.temporal.iso_timestamp.clone()),
                            face_confidence: face.confidence,
                            expression: top_expression.map(|e| e.label.clone()),
                            expression_score: top_expression.map(|e| e.score),
                            bounding_box: face.bounding_box.clone(),
                        });
                    }
                }
            }
        }

        // Sort by face confidence descending so the clearest, highest-quality faces are on top
        matches.sort_by(|a, b| {
            b.face_confidence
                .partial_cmp(&a.face_confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_photos = matches.len();
        let max_items = limit.unwrap_or(10);
        let top_photos = matches.into_iter().take(max_items).collect();

        Ok(Some(ClusterDetails {
            cluster,
            top_photos,
            total_photos,
        }))
    }
}
