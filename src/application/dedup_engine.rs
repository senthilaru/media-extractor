use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::time::Instant;
use uuid::Uuid;

use crate::domain::dedup_models::{
    CanonicalAsset, DeduplicationConfig, DeduplicationMetadata, DeduplicationStatus,
    DeduplicationSummary, DriveConfiguration, DuplicateCluster, DuplicateRelationship,
    DuplicateVariant, MediaFingerprint, MediaQualityRank,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::metadata_models::{MediaMetadataDocument, TimestampSource};

use crate::domain::models::MediaCategory;
use crate::domain::ports::{DeduplicationEngine, MetadataStorageSink, MultiIndexLookup};
use crate::infrastructure::drives_config::CanonicalPathMatcher;
use crate::infrastructure::multi_index::MultiIndexLookupTable;

/// Disjoint Set Union (Union-Find) for grouping candidate duplicate items.
#[derive(Debug, Default)]
struct DisjointSet {
    parent: HashMap<Uuid, Uuid>,
}

impl DisjointSet {
    fn new() -> Self {
        Self {
            parent: HashMap::new(),
        }
    }

    fn find(&mut self, id: Uuid) -> Uuid {
        let p = *self.parent.entry(id).or_insert(id);
        if p == id {
            id
        } else {
            let root = self.find(p);
            self.parent.insert(id, root);
            root
        }
    }

    fn union(&mut self, a: Uuid, b: Uuid) {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a != root_b {
            self.parent.insert(root_b, root_a);
        }
    }
}

/// Application service orchestrating the multi-tier media deduplication pipeline.
#[derive(Debug, Default, Clone)]
pub struct MediaDeduplicationEngine;

impl MediaDeduplicationEngine {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates canonical quality score to deterministically pick the primary master file.
    pub fn score_canonical_candidate(fp: &MediaFingerprint) -> f64 {
        let format_score = match fp.media_category {
            MediaCategory::RawPhoto => 300_000_000.0,
            MediaCategory::Photo => 200_000_000.0,
            MediaCategory::Video => 100_000_000.0,
            MediaCategory::Sidecar => 50_000_000.0,
        };

        let resolution_score = if let Some(dims) = fp.dimensions {
            (dims.width as f64 * dims.height as f64) * 10.0
        } else {
            0.0
        };

        let size_score = (fp.file_size_bytes as f64) / 1024.0;

        format_score + resolution_score + size_score
    }

    /// Computes the quality rank of a document for deduplication keeper selection.
    pub fn compute_quality_rank(
        doc: &MediaMetadataDocument,
        drive_priority: u32,
        is_in_master: bool,
    ) -> MediaQualityRank {
        let dims = doc.dimensions.or_else(|| {
            doc.fingerprint.as_ref().and_then(|fp| fp.dimensions)
        });
        let total_pixels = dims.map(|d| d.width as u64 * d.height as u64).unwrap_or(0);
        let file_size_bytes = doc.file_context.file_size_bytes;

        let format_tier = match doc.file_context.media_category {
            MediaCategory::RawPhoto => 4,
            MediaCategory::Photo => 3,
            MediaCategory::Video => 2,
            MediaCategory::Sidecar => 1,
        };

        let has_exif_date = doc.temporal.source != TimestampSource::FileSystemModified;
        let has_gps = doc.location.is_some();

        MediaQualityRank::new(
            total_pixels,
            file_size_bytes,
            format_tier,
            has_exif_date,
            has_gps,
            drive_priority,
            is_in_master,
        )
    }


    /// Forms a duplicate cluster given a group of identical or similar fingerprints.
    pub fn build_cluster(
        fps: &[MediaFingerprint],
        rel: DuplicateRelationship,
    ) -> Option<DuplicateCluster> {
        if fps.len() < 2 {
            return None;
        }

        // Sort to deterministically choose the canonical master: highest score first,
        // then earlier fingerprinted_at, tie-break path ascending
        let mut sorted: Vec<&MediaFingerprint> = fps.iter().collect();
        sorted.sort_by(|a, b| {
            let score_a = Self::score_canonical_candidate(a);
            let score_b = Self::score_canonical_candidate(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.fingerprinted_at.cmp(&b.fingerprinted_at))
                .then_with(|| a.file_path.cmp(&b.file_path))
        });

        let master = sorted[0];
        let canonical_asset = CanonicalAsset {
            file_path: master.file_path.clone(),
            file_size_bytes: master.file_size_bytes,
            dimensions: master.dimensions,
            media_category: master.media_category,
            quality_score: Self::score_canonical_candidate(master),
        };

        let mut variants = Vec::new();
        let mut total_reclaimable = 0u64;

        for &v in &sorted[1..] {
            let dist = master.min_hamming_distance(v);
            let variant_rel = if rel == DuplicateRelationship::ExactBitwise {
                DuplicateRelationship::ExactBitwise
            } else if let (Some(m_dims), Some(v_dims)) = (master.dimensions, v.dimensions) {
                let m_pixels = (m_dims.width as f64) * (m_dims.height as f64);
                let v_pixels = (v_dims.width as f64) * (v_dims.height as f64);
                if v_pixels <= 0.60 * m_pixels
                    || (v_dims.width as f64) <= 0.60 * (m_dims.width as f64)
                {
                    DuplicateRelationship::ThumbnailVariant
                } else if v.file_size_bytes < master.file_size_bytes {
                    DuplicateRelationship::RecompressedVariant
                } else {
                    DuplicateRelationship::PerceptualVisualDuplicate
                }
            } else if v.file_size_bytes < master.file_size_bytes {
                DuplicateRelationship::RecompressedVariant
            } else {
                DuplicateRelationship::PerceptualVisualDuplicate
            };

            let sim = if variant_rel == DuplicateRelationship::ExactBitwise {
                1.0
            } else if let Some(d) = dist {
                (64.0 - d as f64) / 64.0
            } else {
                0.9
            };

            variants.push(DuplicateVariant {
                file_path: v.file_path.clone(),
                file_size_bytes: v.file_size_bytes,
                relationship: variant_rel,
                hamming_distance: dist,
                similarity_score: (sim * 1000.0).round() / 1000.0,
                reclaimable_bytes: v.file_size_bytes,
            });

            total_reclaimable += v.file_size_bytes;
        }

        let cluster_relationship = if rel == DuplicateRelationship::ExactBitwise {
            DuplicateRelationship::ExactBitwise
        } else if variants
            .iter()
            .any(|v| v.relationship == DuplicateRelationship::ThumbnailVariant)
        {
            DuplicateRelationship::ThumbnailVariant
        } else if variants
            .iter()
            .any(|v| v.relationship == DuplicateRelationship::RecompressedVariant)
        {
            DuplicateRelationship::RecompressedVariant
        } else {
            DuplicateRelationship::PerceptualVisualDuplicate
        };

        Some(DuplicateCluster {
            cluster_id: Uuid::new_v4(),
            relationship_type: cluster_relationship,
            canonical_asset,
            variants,
            total_reclaimable_bytes: total_reclaimable,
            total_files: fps.len(),
        })
    }

    /// Evaluates Tier 1-3 exact bitwise deduplication across a set of fingerprints.
    pub fn find_exact_duplicates(
        fingerprints: &[MediaFingerprint],
    ) -> (Vec<DuplicateCluster>, usize) {
        let total = fingerprints.len();
        let mut clusters = Vec::new();

        // Tier 1: Group by file_size_bytes
        let mut size_buckets: HashMap<u64, Vec<&MediaFingerprint>> = HashMap::new();
        for fp in fingerprints {
            size_buckets.entry(fp.file_size_bytes).or_default().push(fp);
        }

        let mut candidate_pairs_pruned = 0usize;

        for (_size, group) in size_buckets {
            if group.len() < 2 {
                candidate_pairs_pruned += 1;
                continue;
            }

            // Tier 2: Group by sample_digest
            let mut sample_buckets: HashMap<&str, Vec<&MediaFingerprint>> = HashMap::new();
            for &fp in &group {
                if let Some(ref sample) = fp.sample_digest {
                    sample_buckets.entry(sample.as_str()).or_default().push(fp);
                } else {
                    sample_buckets.entry("").or_default().push(fp);
                }
            }

            for (_sample, sample_group) in sample_buckets {
                if sample_group.len() < 2 {
                    candidate_pairs_pruned += sample_group.len();
                    continue;
                }

                // Tier 3: Group by full cryptographic_hash
                let mut crypto_buckets: HashMap<&str, Vec<&MediaFingerprint>> = HashMap::new();
                for &fp in &sample_group {
                    crypto_buckets
                        .entry(fp.cryptographic_hash.as_str())
                        .or_default()
                        .push(fp);
                }

                for (_hash, crypto_group) in crypto_buckets {
                    if crypto_group.len() >= 2 {
                        let owned_fps: Vec<MediaFingerprint> =
                            crypto_group.into_iter().cloned().collect();
                        if let Some(cluster) =
                            Self::build_cluster(&owned_fps, DuplicateRelationship::ExactBitwise)
                        {
                            clusters.push(cluster);
                        }
                    } else {
                        candidate_pairs_pruned += crypto_group.len();
                    }
                }
            }
        }

        let pruned_pct = if total > 0 {
            (candidate_pairs_pruned as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        (clusters, pruned_pct as usize)
    }

    /// Evaluates Tier 4 perceptual near-duplicate clustering using Multi-Index Hashing.
    pub fn find_perceptual_duplicates(
        fingerprints: &[MediaFingerprint],
        config: &DeduplicationConfig,
    ) -> Vec<DuplicateCluster> {
        let mut table = MultiIndexLookupTable::new();
        let mut eligible_fps: HashMap<Uuid, &MediaFingerprint> = HashMap::new();

        for fp in fingerprints {
            // Guard: ignore low-entropy monochromatic/blank images
            if fp.is_low_entropy {
                continue;
            }
            if let Some(phash) = fp.perceptual_hash {
                table.insert(fp.id, phash);
                eligible_fps.insert(fp.id, fp);
            }
        }

        let mut dsu = DisjointSet::new();

        for (&id, &fp) in &eligible_fps {
            if let Some(phash) = fp.perceptual_hash {
                let candidates = table.query_candidates(phash, config.similarity_threshold);
                for (cand_id, _) in candidates {
                    if cand_id == id {
                        continue;
                    }
                    if let Some(cand_fp) = eligible_fps.get(&cand_id) {
                        let dist = fp.min_hamming_distance(cand_fp).unwrap_or(u32::MAX);
                        if dist <= config.similarity_threshold {
                            dsu.union(id, cand_id);
                        }
                    }
                }
            }
        }

        // Group by DSU root
        let mut groups: HashMap<Uuid, Vec<MediaFingerprint>> = HashMap::new();
        for (&id, &fp) in &eligible_fps {
            let root = dsu.find(id);
            groups.entry(root).or_default().push(fp.clone());
        }

        let mut clusters = Vec::new();
        for (_root, group) in groups {
            if group.len() >= 2 {
                if let Some(cluster) =
                    Self::build_cluster(&group, DuplicateRelationship::PerceptualVisualDuplicate)
                {
                    clusters.push(cluster);
                }
            }
        }

        clusters
    }

    /// Evaluates any content group (single unique asset or multi-document duplicate cluster)
    /// against drive hierarchies and assigns appropriate DeduplicationStatus.
    pub fn evaluate_content_group(
        docs: &[MediaMetadataDocument],
        matcher: &CanonicalPathMatcher,
    ) -> Vec<(Uuid, DeduplicationMetadata)> {
        if docs.is_empty() {
            return Vec::new();
        }

        let cluster_id = Uuid::new_v4();
        let evaluated_at = Utc::now();

        // 1. Analyze each document's path and drive context
        struct DocContext<'a> {
            doc: &'a MediaMetadataDocument,
            drive_id: Option<String>,
            drive_group: Option<String>,
            is_in_master: bool,
            quality_rank: MediaQualityRank,
        }

        let contexts: Vec<DocContext> = docs
            .iter()
            .map(|doc| {
                let drive_def = matcher.find_drive_for_path(&doc.file_context.file_path);
                let is_in_master = matcher.is_in_canonical_master(&doc.file_context.file_path);
                let (drive_id, drive_group, group_priority) = match drive_def {
                    Some(drive) => (
                        Some(drive.id.clone()),
                        Some(drive.group.clone()),
                        matcher.group_priority(&drive.group),
                    ),
                    None => (None, None, 0),
                };

                let quality_rank = Self::compute_quality_rank(doc, group_priority, is_in_master);

                DocContext {
                    doc,
                    drive_id,
                    drive_group,
                    is_in_master,
                    quality_rank,
                }
            })
            .collect();

        // 2. Identify primary master candidates (assets located in primary drive's canonical master path)
        let primary_master_candidates: Vec<&DocContext> = contexts
            .iter()
            .filter(|c| matcher.is_primary_master(&c.doc.file_context.file_path))
            .collect();

        let get_drive_index = |drive_id: Option<&str>| -> usize {
            drive_id
                .and_then(|id| matcher.config().drives.iter().position(|d| d.id == id))
                .unwrap_or(usize::MAX)
        };

        // If there are multiple copies on the SAME primary drive: true intra-drive conflict -> ConflictReview
        let mut seen_primary_drives = std::collections::HashSet::new();
        let has_intra_drive_conflict = primary_master_candidates
            .iter()
            .any(|c| !seen_primary_drives.insert(c.drive_id.as_deref()));

        if has_intra_drive_conflict {
            return contexts
                .iter()
                .map(|c| {
                    (
                        c.doc.id,
                        DeduplicationMetadata {
                            cluster_id,
                            status: DeduplicationStatus::ConflictReview,
                            canonical_master_id: None,
                            canonical_master_path: None,
                            drive_id: c.drive_id.clone(),
                            drive_group: c.drive_group.clone(),
                            is_in_master_path: c.is_in_master,
                            reclaimable_bytes: 0,
                            evaluated_at,
                            replication: None,
                        },
                    )
                })
                .collect();
        }

        // Case A: At least one copy exists in the Primary Master pool -> CanonicalMaster!
        if !primary_master_candidates.is_empty() {
            let mut sorted_primary_candidates = primary_master_candidates;
            sorted_primary_candidates.sort_by(|a, b| {
                let idx_a = get_drive_index(a.drive_id.as_deref());
                let idx_b = get_drive_index(b.drive_id.as_deref());
                idx_a.cmp(&idx_b).then_with(|| a.doc.id.cmp(&b.doc.id))
            });

            let canonical_master_ctx = sorted_primary_candidates[0];
            let canonical_master_id = canonical_master_ctx.doc.id;
            let canonical_master_path = canonical_master_ctx.doc.file_context.file_path.clone();

            // Check if any unmastered candidate outside master has strictly higher quality
            // (higher resolution or equal resolution with larger file size)
            let superior_external = contexts
                .iter()
                .filter(|c| c.doc.id != canonical_master_id && !c.is_in_master)
                .filter(|c| {
                    c.quality_rank.total_pixels > canonical_master_ctx.quality_rank.total_pixels
                        || (c.quality_rank.total_pixels == canonical_master_ctx.quality_rank.total_pixels
                            && c.quality_rank.file_size_bytes > canonical_master_ctx.quality_rank.file_size_bytes)
                })
                .max_by_key(|c| c.quality_rank);

            let superior_id = superior_external.map(|s| s.doc.id);

            // Enforce single copy per replica group across all replica pool volumes
            let mut chosen_replica_by_group: std::collections::HashMap<String, Uuid> =
                std::collections::HashMap::new();

            let mut eligible_replicas: Vec<&DocContext> = contexts
                .iter()
                .filter(|c| {
                    c.is_in_master
                        && c.doc.id != canonical_master_id
                        && c.drive_group
                            .as_deref()
                            .map(|g| !g.eq_ignore_ascii_case("primary"))
                            .unwrap_or(false)
                        && c.quality_rank
                            .content_quality_cmp(&canonical_master_ctx.quality_rank)
                            != std::cmp::Ordering::Less
                })
                .collect();

            eligible_replicas.sort_by(|a, b| {
                let idx_a = get_drive_index(a.drive_id.as_deref());
                let idx_b = get_drive_index(b.drive_id.as_deref());
                idx_a.cmp(&idx_b).then_with(|| a.doc.id.cmp(&b.doc.id))
            });

            for rep in eligible_replicas {
                if let Some(ref group) = rep.drive_group {
                    chosen_replica_by_group.entry(group.clone()).or_insert(rep.doc.id);
                }
            }

            return contexts
                .iter()
                .map(|c| {
                    let status = if Some(c.doc.id) == superior_id {
                        // Higher-resolution candidate discovered outside master -> upgrade master!
                        DeduplicationStatus::NeedsIngestToMaster
                    } else if c.doc.id == canonical_master_id {
                        DeduplicationStatus::CanonicalMaster
                    } else if let Some(ref group) = c.drive_group {
                        if chosen_replica_by_group.get(group) == Some(&c.doc.id) {
                            DeduplicationStatus::ReplicaKeep
                        } else {
                            DeduplicationStatus::MarkForRemoval
                        }
                    } else {
                        // Lower-resolution, smaller, or redundant duplicate -> mark for deletion
                        DeduplicationStatus::MarkForRemoval
                    };

                    let reclaimable_bytes = if status == DeduplicationStatus::MarkForRemoval {
                        c.doc.file_context.file_size_bytes
                    } else {
                        0
                    };

                    let replication = if status == DeduplicationStatus::CanonicalMaster {
                        c.doc.dedup.as_ref().and_then(|d| d.replication.clone())
                    } else {
                        None
                    };

                    (
                        c.doc.id,
                        DeduplicationMetadata {
                            cluster_id,
                            status,
                            canonical_master_id: Some(canonical_master_id),
                            canonical_master_path: Some(canonical_master_path.clone()),
                            drive_id: c.drive_id.clone(),
                            drive_group: c.drive_group.clone(),
                            is_in_master_path: c.is_in_master,
                            reclaimable_bytes,
                            evaluated_at,
                            replication,
                        },
                    )
                })
                .collect();
        }

        // Case B: Zero copies exist in the Primary Master path.
        // We select the single best candidate to be ingested to master:
        // Rank by MediaQualityRank (total_pixels descending, file_size_bytes descending,
        // format_tier, metadata, group_priority, is_in_master), then tie-break by deterministic doc ID.
        let mut sorted_contexts: Vec<&DocContext> = contexts.iter().collect();
        sorted_contexts.sort_by(|a, b| {
            b.quality_rank
                .cmp(&a.quality_rank)
                .then_with(|| a.doc.id.cmp(&b.doc.id))
        });

        let best_candidate_ctx = sorted_contexts[0];
        let best_candidate_id = best_candidate_ctx.doc.id;

        // Enforce single copy per replica group across all replica pool volumes in Case B
        let mut chosen_replica_by_group: std::collections::HashMap<String, Uuid> =
            std::collections::HashMap::new();

        let mut eligible_replicas: Vec<&DocContext> = contexts
            .iter()
            .filter(|c| {
                c.is_in_master
                    && c.doc.id != best_candidate_id
                    && c.drive_group
                        .as_deref()
                        .map(|g| !g.eq_ignore_ascii_case("primary"))
                        .unwrap_or(false)
                    && c.quality_rank
                        .content_quality_cmp(&best_candidate_ctx.quality_rank)
                        != std::cmp::Ordering::Less
            })
            .collect();

        eligible_replicas.sort_by(|a, b| {
            let idx_a = get_drive_index(a.drive_id.as_deref());
            let idx_b = get_drive_index(b.drive_id.as_deref());
            idx_a.cmp(&idx_b).then_with(|| a.doc.id.cmp(&b.doc.id))
        });

        for rep in eligible_replicas {
            if let Some(ref group) = rep.drive_group {
                chosen_replica_by_group.entry(group.clone()).or_insert(rep.doc.id);
            }
        }

        contexts
            .iter()
            .map(|c| {
                let status = if c.doc.id == best_candidate_id {
                    DeduplicationStatus::NeedsIngestToMaster
                } else if let Some(ref group) = c.drive_group {
                    if chosen_replica_by_group.get(group) == Some(&c.doc.id) {
                        DeduplicationStatus::ReplicaKeep
                    } else {
                        DeduplicationStatus::MarkForRemoval
                    }
                } else {
                    DeduplicationStatus::MarkForRemoval
                };

                let reclaimable_bytes = if status == DeduplicationStatus::MarkForRemoval {
                    c.doc.file_context.file_size_bytes
                } else {
                    0
                };

                (
                    c.doc.id,
                    DeduplicationMetadata {
                        cluster_id,
                        status,
                        canonical_master_id: None,
                        canonical_master_path: None,
                        drive_id: c.drive_id.clone(),
                        drive_group: c.drive_group.clone(),
                        is_in_master_path: c.is_in_master,
                        reclaimable_bytes,
                        evaluated_at,
                        replication: None,
                    },
                )
            })
            .collect()
    }

    /// Evaluates a cluster of duplicate media documents across drive hierarchies according to
    /// canonical master path rules (delegates to evaluate_content_group).
    pub fn evaluate_replica_cluster(
        docs: &[MediaMetadataDocument],
        matcher: &CanonicalPathMatcher,
    ) -> Vec<(Uuid, DeduplicationMetadata)> {
        Self::evaluate_content_group(docs, matcher)
    }

    /// Evaluates deduplication rules and persists classification updates for a list of content groups.
    pub async fn classify_and_persist_content_groups(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        content_groups: &[(String, Vec<MediaMetadataDocument>)],
        dry_run: bool,
        start_time: Instant,
    ) -> Result<DeduplicationSummary, DeduplicationError> {
        let matcher = CanonicalPathMatcher::new(config.clone());
        let is_tty = std::io::stderr().is_terminal();

        let mut total_files_scanned = 0usize;
        let mut unique_media_count = 0usize;
        let mut canonical_masters_count = 0usize;
        let mut replica_files_preserved = 0usize;
        let mut needs_ingest_files = 0usize;
        let mut conflict_review_files = 0usize;
        let mut duplicate_clusters_count = 0usize;
        let mut exact_duplicate_files = 0usize;
        let mut total_reclaimable_bytes = 0u64;

        let mut all_updates = Vec::new();

        for (_hash, docs) in content_groups {
            total_files_scanned += docs.len();

            let evaluations = Self::evaluate_content_group(docs, &matcher);
            let mut group_redundant_count = 0usize;
            let mut group_has_master = false;

            for (id, meta) in evaluations {
                match meta.status {
                    DeduplicationStatus::CanonicalMaster => {
                        group_has_master = true;
                        canonical_masters_count += 1;
                    }
                    DeduplicationStatus::ReplicaKeep => {
                        replica_files_preserved += 1;
                    }
                    DeduplicationStatus::NeedsIngestToMaster => {
                        needs_ingest_files += 1;
                        unique_media_count += 1;
                    }
                    DeduplicationStatus::MarkForRemoval => {
                        group_redundant_count += 1;
                        exact_duplicate_files += 1;
                        total_reclaimable_bytes += meta.reclaimable_bytes;
                    }
                    DeduplicationStatus::ConflictReview => {
                        conflict_review_files += 1;
                    }
                    DeduplicationStatus::Removed => {}
                }
                all_updates.push((id, meta));
            }

            if group_has_master {
                unique_media_count += 1;
            }

            if group_redundant_count > 0 {
                duplicate_clusters_count += 1;
            }
        }

        if !dry_run && !all_updates.is_empty() {
            let total_updates = all_updates.len();
            let update_pb = if is_tty {
                let p = indicatif::ProgressBar::new(total_updates as u64);
                p.set_style(
                    indicatif::ProgressStyle::default_bar()
                        .template("{spinner:.green} [{bar:28.cyan/blue}] {percent}% | {pos}/{len} records ({per_sec}) | Elapsed: {elapsed} | ETA: {eta}")
                        .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
                        .progress_chars("=> "),
                );
                p.enable_steady_tick(std::time::Duration::from_millis(150));
                Some(p)
            } else {
                eprintln!(
                    "Persisting {} classification updates to MongoDB...",
                    total_updates
                );
                None
            };

            for chunk in all_updates.chunks(1000) {
                sink.update_dedup_batch(chunk).await?;
                if let Some(ref p) = update_pb {
                    p.inc(chunk.len() as u64);
                }
            }

            if let Some(p) = update_pb {
                p.finish_and_clear();
            }
        }

        Ok(DeduplicationSummary {
            total_files_scanned,
            unique_media_count,
            duplicate_clusters_count,
            exact_duplicate_files,
            near_duplicate_files: 0,
            total_reclaimable_bytes,
            tier_pruned_percentage: 0.0,
            elapsed_seconds: start_time.elapsed().as_secs_f64(),
            canonical_masters_count,
            replica_files_preserved,
            needs_ingest_files,
            conflict_review_files,
        })
    }

    /// Runs scoped deduplication classification for a specific source path prefix.
    /// Fast O(incoming) audit: queries only content groups matching hashes in the source path.
    pub async fn run_scoped_classification(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        source_prefix: &str,
        dry_run: bool,
    ) -> Result<DeduplicationSummary, DeduplicationError> {
        let start = Instant::now();
        let is_tty = std::io::stderr().is_terminal();

        let pb = if is_tty {
            let p = indicatif::ProgressBar::new_spinner();
            p.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg} | Elapsed: {elapsed}")
                    .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
            );
            p.set_message("Querying scoped catalog hashes from MongoDB...");
            p.enable_steady_tick(std::time::Duration::from_millis(120));
            Some(p)
        } else {
            eprintln!("Querying scoped catalog hashes from MongoDB...");
            None
        };

        let hashes = sink.query_distinct_hashes_for_prefix(source_prefix).await?;

        if let Some(ref p) = pb {
            p.set_message(format!(
                "Auditing {} distinct content groups for source...",
                hashes.len()
            ));
        }

        let content_groups = sink.query_content_groups_for_hashes(&hashes).await?;

        if let Some(p) = pb {
            p.finish_and_clear();
        }

        self.classify_and_persist_content_groups(sink, config, &content_groups, dry_run, start)
            .await
    }

    /// Runs universal classification over all assets in the catalog (both unique and duplicates).
    pub async fn run_universal_classification(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        dry_run: bool,
    ) -> Result<DeduplicationSummary, DeduplicationError> {
        let start = Instant::now();
        let is_tty = std::io::stderr().is_terminal();

        let pb = if is_tty {
            let p = indicatif::ProgressBar::new_spinner();
            p.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg} | Elapsed: {elapsed}")
                    .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
            );
            p.set_message("Querying catalog content groups from MongoDB...");
            p.enable_steady_tick(std::time::Duration::from_millis(120));
            Some(p)
        } else {
            eprintln!("Querying catalog content groups from MongoDB...");
            None
        };

        // Step 1: Query all content groups (both unique count == 1 and duplicates count > 1)
        let content_groups = sink.query_all_content_groups(None).await?;

        if let Some(ref p) = pb {
            p.set_message(format!(
                "Classifying {} catalog groups...",
                content_groups.len()
            ));
        }

        if let Some(p) = pb {
            p.finish_and_clear();
        }

        self.classify_and_persist_content_groups(sink, config, &content_groups, dry_run, start)
            .await
    }

    /// Orchestrates database-driven deduplication:
    /// 1. Queries MongoDB for all duplicate cryptographic hash groups.
    /// 2. Evaluates each cluster using `evaluate_replica_cluster` with `CanonicalPathMatcher`.
    /// 3. If `dry_run` is false, persists `DeduplicationMetadata` updates to MongoDB in bulk.
    /// 4. Aggregates and returns `DeduplicationSummary`.
    pub async fn run_database_dedup(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        dry_run: bool,
    ) -> Result<DeduplicationSummary, DeduplicationError> {
        let start = Instant::now();
        let matcher = CanonicalPathMatcher::new(config.clone());
        let is_tty = std::io::stderr().is_terminal();

        let pb = if is_tty {
            let p = indicatif::ProgressBar::new_spinner();
            p.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg} | Elapsed: {elapsed}")
                    .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
            );
            p.set_message("Querying duplicate groups from MongoDB...");
            p.enable_steady_tick(std::time::Duration::from_millis(120));
            Some(p)
        } else {
            eprintln!("Querying duplicate groups from MongoDB...");
            None
        };

        // Step 1: Query all duplicate cryptographic groups from MongoDB
        let duplicate_groups = sink.query_duplicate_groups().await?;

        if let Some(ref p) = pb {
            p.set_message(format!(
                "Classifying {} duplicate groups...",
                duplicate_groups.len()
            ));
        }

        let mut all_updates: Vec<(Uuid, DeduplicationMetadata)> = Vec::new();
        let mut total_files_scanned = 0usize;
        let mut unique_media_count = 0usize;
        let mut canonical_masters_count = 0usize;
        let mut replica_files_preserved = 0usize;
        let mut needs_ingest_files = 0usize;
        let mut conflict_review_files = 0usize;
        let mut duplicate_clusters_count = 0usize;
        let mut exact_duplicate_files = 0usize;
        let mut total_reclaimable_bytes = 0u64;

        // Step 2: Evaluate replica hierarchy for each group
        for (_hash, docs) in &duplicate_groups {
            total_files_scanned += docs.len();
            let updates = Self::evaluate_replica_cluster(docs, &matcher);

            let mut group_redundant_count = 0usize;
            let mut group_has_master = false;

            for (id, meta) in updates {
                match meta.status {
                    DeduplicationStatus::MarkForRemoval => {
                        group_redundant_count += 1;
                        exact_duplicate_files += 1;
                        total_reclaimable_bytes += meta.reclaimable_bytes;
                    }
                    DeduplicationStatus::CanonicalMaster => {
                        group_has_master = true;
                        canonical_masters_count += 1;
                    }
                    DeduplicationStatus::ReplicaKeep => {
                        replica_files_preserved += 1;
                    }
                    DeduplicationStatus::NeedsIngestToMaster => {
                        needs_ingest_files += 1;
                        unique_media_count += 1;
                    }
                    DeduplicationStatus::ConflictReview => {
                        conflict_review_files += 1;
                    }
                    DeduplicationStatus::Removed => {}
                }
                all_updates.push((id, meta));
            }

            if group_has_master {
                unique_media_count += 1;
            }

            if group_redundant_count > 0 {
                duplicate_clusters_count += 1;
            }
        }

        if let Some(p) = pb {
            p.finish_and_clear();
        }

        // Step 3: Persist updates if not in dry-run mode
        if !dry_run && !all_updates.is_empty() {
            let total_updates = all_updates.len();
            let update_pb = if is_tty {
                let p = indicatif::ProgressBar::new(total_updates as u64);
                p.set_style(
                    indicatif::ProgressStyle::default_bar()
                        .template("{spinner:.green} [{bar:28.cyan/blue}] {percent}% | {pos}/{len} records ({per_sec}) | Elapsed: {elapsed} | ETA: {eta}")
                        .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
                        .progress_chars("=> "),
                );
                p.enable_steady_tick(std::time::Duration::from_millis(150));
                Some(p)
            } else {
                eprintln!(
                    "Persisting {} classification updates to MongoDB...",
                    total_updates
                );
                None
            };

            for chunk in all_updates.chunks(1000) {
                sink.update_dedup_batch(chunk).await?;
                if let Some(ref p) = update_pb {
                    p.inc(chunk.len() as u64);
                }
            }

            if let Some(p) = update_pb {
                p.finish_and_clear();
            }
        }

        let summary = DeduplicationSummary {
            total_files_scanned,
            unique_media_count,
            duplicate_clusters_count,
            exact_duplicate_files,
            near_duplicate_files: 0,
            total_reclaimable_bytes,
            tier_pruned_percentage: if total_files_scanned > 0 {
                (exact_duplicate_files as f64 / total_files_scanned as f64) * 100.0
            } else {
                0.0
            },
            elapsed_seconds: start.elapsed().as_secs_f64(),
            canonical_masters_count,
            replica_files_preserved,
            needs_ingest_files,
            conflict_review_files,
        };

        Ok(summary)
    }
}

#[async_trait::async_trait]
impl DeduplicationEngine for MediaDeduplicationEngine {
    async fn find_duplicates(
        &self,
        fingerprints: Vec<MediaFingerprint>,
        config: DeduplicationConfig,
    ) -> Result<(Vec<DuplicateCluster>, DeduplicationSummary), DeduplicationError> {
        let start = Instant::now();
        let total_scanned = fingerprints.len();

        // Step 1: Run Tier 1-3 Exact Bitwise deduplication
        let (exact_clusters, pruned_pct) = Self::find_exact_duplicates(&fingerprints);

        // Gather all variant file paths from exact clusters so they aren't redundantly processed in Tier 4
        let mut exact_variant_paths: HashSet<std::path::PathBuf> = HashSet::new();
        let mut exact_count = 0usize;
        let mut exact_reclaimable = 0u64;

        for c in &exact_clusters {
            for v in &c.variants {
                exact_variant_paths.insert(v.file_path.clone());
                exact_count += 1;
            }
            exact_reclaimable += c.total_reclaimable_bytes;
        }

        // Step 2: Run Tier 4 Perceptual Deduplication if enabled
        let mut all_clusters = exact_clusters;
        let mut near_count = 0usize;
        let mut near_reclaimable = 0u64;

        if config.enable_perceptual {
            // Filter out exact variants to prevent double-counting
            let perceptual_candidates: Vec<MediaFingerprint> = fingerprints
                .into_iter()
                .filter(|fp| !exact_variant_paths.contains(&fp.file_path))
                .collect();

            let perceptual_clusters =
                Self::find_perceptual_duplicates(&perceptual_candidates, &config);

            for c in &perceptual_clusters {
                near_count += c.variants.len();
                near_reclaimable += c.total_reclaimable_bytes;
            }

            all_clusters.extend(perceptual_clusters);
        }

        let total_reclaimable = exact_reclaimable + near_reclaimable;
        let unique = total_scanned.saturating_sub(exact_count + near_count);

        let summary = DeduplicationSummary {
            total_files_scanned: total_scanned,
            unique_media_count: unique,
            duplicate_clusters_count: all_clusters.len(),
            exact_duplicate_files: exact_count,
            near_duplicate_files: near_count,
            total_reclaimable_bytes: total_reclaimable,
            tier_pruned_percentage: pruned_pct as f64,
            elapsed_seconds: start.elapsed().as_secs_f64(),
            canonical_masters_count: all_clusters.len(),
            replica_files_preserved: 0,
            needs_ingest_files: 0,
            conflict_review_files: 0,
        };

        Ok((all_clusters, summary))
    }
}
