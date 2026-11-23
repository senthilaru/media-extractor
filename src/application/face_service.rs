use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::domain::errors::{ExtractionError, StorageError};
use crate::domain::person_cluster::{PersonCluster, FACE_CLUSTER_THRESHOLD_COSINE_DISTANCE};
use crate::domain::ports::MetadataStorageSink;

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
            .query_canonical_masters()
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
}
