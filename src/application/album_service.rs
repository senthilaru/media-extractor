use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::domain::errors::{ExtractionError, StorageError};
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::ports::MetadataStorageSink;
use crate::domain::smart_album::{SmartAlbum, SmartAlbumCriteria};

#[derive(Debug, Clone)]
pub struct AlbumSynthesisOptions {
    pub year: Option<u32>,
    pub min_photos: usize,
    pub dry_run: bool,
}

impl Default for AlbumSynthesisOptions {
    fn default() -> Self {
        Self {
            year: None,
            min_photos: 3,
            dry_run: false,
        }
    }
}

pub struct SmartAlbumSynthesisService {
    storage: Arc<dyn MetadataStorageSink>,
}

impl SmartAlbumSynthesisService {
    pub fn new(storage: Arc<dyn MetadataStorageSink>) -> Self {
        Self { storage }
    }

    /// Synthesizes virtual smart albums based on temporal proximity and occasions
    pub async fn synthesize_albums(
        &self,
        options: &AlbumSynthesisOptions,
    ) -> Result<Vec<SmartAlbum>, ExtractionError> {
        info!("Beginning smart album synthesis: {:?}", options);

        let all_masters = self
            .storage
            .query_canonical_masters()
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: PathBuf::new(),
                message: format!("Failed querying canonical masters: {e}"),
            })?;

        let person_clusters = self
            .storage
            .list_person_clusters()
            .await
            .unwrap_or_default();

        let cluster_name_map: HashMap<Uuid, String> = person_clusters
            .into_iter()
            .filter_map(|c| c.display_name.map(|name| (c.id, name)))
            .collect();

        // Filter candidates by year if requested
        let candidates: Vec<MediaMetadataDocument> = all_masters
            .into_iter()
            .filter(|doc| {
                if let Some(y) = options.year {
                    if doc.temporal.year as u32 != y {
                        return false;
                    }
                }
                doc.visual.is_some()
            })
            .collect();

        // Group by (year, occasion category)
        let mut grouped: HashMap<(i32, String), Vec<MediaMetadataDocument>> = HashMap::new();

        for doc in candidates {
            if let Some(ref vis) = doc.visual {
                for occ in &vis.occasions {
                    let key = (doc.temporal.year, occ.category.clone());
                    grouped.entry(key).or_default().push(doc.clone());
                }
            }
        }

        let mut generated_albums = Vec::new();

        for ((year, occasion), docs) in grouped {
            if docs.len() < options.min_photos {
                continue;
            }

            // Identify prominent people in these photos
            let mut person_counts: HashMap<Uuid, usize> = HashMap::new();
            for doc in &docs {
                if let Some(ref vis) = doc.visual {
                    for face in &vis.faces {
                        if let Some(pid) = face.person_id {
                            *person_counts.entry(pid).or_default() += 1;
                        }
                    }
                }
            }

            // Find top person name if present
            let top_person_name = person_counts
                .into_iter()
                .max_by_key(|&(_, count)| count)
                .and_then(|(pid, _)| cluster_name_map.get(&pid).cloned());

            let occasion_title = occasion
                .split('_')
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            let title = if let Some(ref person_name) = top_person_name {
                format!("{} - {}'s {}", year, person_name, occasion_title)
            } else {
                format!("{} - {}", year, occasion_title)
            };

            let cover_id = SmartAlbum::select_cover_photo(&docs);
            let asset_ids: Vec<Uuid> = docs.iter().map(|d| d.id).collect();

            let criteria = SmartAlbumCriteria {
                date_start: None,
                date_end: None,
                person_ids: Vec::new(),
                required_objects: Vec::new(),
                required_occasions: vec![occasion.clone()],
                text_query: None,
                expressions: Vec::new(),
            };

            let album = SmartAlbum::new(
                title,
                Some(format!("Automatically synthesized album for {} {}", year, occasion)),
                criteria,
                asset_ids,
                cover_id,
            );

            if !options.dry_run {
                if let Err(e) = self.storage.save_smart_album(&album).await {
                    tracing::warn!("Failed saving smart album {}: {}", album.title, e);
                }
            }

            generated_albums.push(album);
        }

        info!("Smart album synthesis completed: {} albums generated.", generated_albums.len());
        Ok(generated_albums)
    }

    /// Lists established smart albums
    pub async fn list_albums(&self) -> Result<Vec<SmartAlbum>, StorageError> {
        self.storage.list_smart_albums().await
    }

    /// Exports album photos as symlinks without duplicating master files on disk
    pub async fn export_album_as_symlinks(
        &self,
        album_id: Uuid,
        destination_dir: &Path,
    ) -> Result<usize, ExtractionError> {
        let album = self
            .storage
            .get_smart_album_by_id(album_id)
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: destination_dir.to_path_buf(),
                message: format!("Failed finding album: {e}"),
            })?
            .ok_or_else(|| ExtractionError::FileNotFound(PathBuf::from(album_id.to_string())))?;

        if !destination_dir.exists() {
            fs::create_dir_all(destination_dir).map_err(|e| ExtractionError::Io {
                path: destination_dir.to_path_buf(),
                source: e,
            })?;
        }

        let mut exported_count = 0;
        for asset_id in &album.asset_ids {
            if let Ok(Some(doc)) = self.storage.find_document_by_id(*asset_id).await {
                let target_path = &doc.file_context.file_path;
                let link_name = destination_dir.join(&doc.file_context.file_name);
                if !link_name.exists() {
                    #[cfg(unix)]
                    let _ = std::os::unix::fs::symlink(target_path, &link_name);
                    exported_count += 1;
                }
            }
        }

        Ok(exported_count)
    }
}
