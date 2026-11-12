use chrono::{DateTime, Utc};
use futures::stream::TryStreamExt;
use mongodb::bson::{doc, Bson, Document};
use mongodb::options::{ClientOptions, FindOptions, IndexOptions, UpdateOneModel, WriteModel};
use mongodb::{Client, Collection, Database, IndexModel, Namespace};
use std::collections::HashMap;
use std::time::Duration;

use crate::domain::dedup_models::{DeduplicationMetadata, DeduplicationStatus};
use crate::domain::errors::StorageError;
use crate::domain::metadata_models::{
    DatabaseSinkConfig, FilterEvaluationResult, MediaMetadataDocument,
};
use crate::domain::models::DiscoveredMediaItem;
use crate::domain::ports::{BatchWriteStats, MetadataStorageSink};

/// Production MongoDB storage sink adapter providing idempotent bulk upserts.
#[derive(Clone)]
pub struct MongoDbStorageSink {
    config: DatabaseSinkConfig,
    client: Client,
    database: Database,
    collection: Collection<Document>,
}

impl MongoDbStorageSink {
    /// Connects to MongoDB, validates connection/credentials, and initializes the collection handle.
    pub async fn connect(config: DatabaseSinkConfig) -> Result<Self, StorageError> {
        let mut client_options =
            ClientOptions::parse(&config.connection_uri)
                .await
                .map_err(|e| StorageError::ConnectionFailed {
                    uri: config.sanitized_uri(),
                    message: format!("Invalid connection string: {}", e),
                })?;

        client_options.connect_timeout = Some(Duration::from_secs(config.timeout_seconds));
        client_options.server_selection_timeout = Some(Duration::from_secs(config.timeout_seconds));
        client_options.app_name = Some("metadata-extractor".to_string());

        let client =
            Client::with_options(client_options).map_err(|e| StorageError::ConnectionFailed {
                uri: config.sanitized_uri(),
                message: format!("Client initialization failed: {}", e),
            })?;

        let database = client.database(&config.database_name);
        let collection = database.collection::<Document>(&config.collection_name);

        let sink = Self {
            config,
            client,
            database,
            collection,
        };

        // Fail-fast verification
        sink.ping().await?;

        Ok(sink)
    }

    /// Accessor for configuration snapshot.
    pub fn config(&self) -> &DatabaseSinkConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl MetadataStorageSink for MongoDbStorageSink {
    async fn ping(&self) -> Result<(), StorageError> {
        // Execute ping command against the target database
        self.database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("Authentication failed") || err_str.contains("auth") {
                    StorageError::AuthenticationFailed {
                        database: self.config.database_name.clone(),
                        message: err_str,
                    }
                } else {
                    StorageError::ConnectionFailed {
                        uri: self.config.sanitized_uri(),
                        message: err_str,
                    }
                }
            })?;

        Ok(())
    }

    async fn ensure_indexes(&self) -> Result<(), StorageError> {
        let indexes = vec![
            // Unique index on file path for idempotent upserts
            IndexModel::builder()
                .keys(doc! { "file_context.file_path": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
            // Compound timeline index for calendar queries
            IndexModel::builder()
                .keys(doc! { "temporal.year": 1, "temporal.month": 1, "temporal.day": 1 })
                .build(),
            // Index for exact duplicate lookup
            IndexModel::builder()
                .keys(doc! { "fingerprint.cryptographic_hash": 1 })
                .build(),
            // Index for visual perceptual hash lookup
            IndexModel::builder()
                .keys(doc! { "fingerprint.phash": 1 })
                .build(),
            // Index for deduplication status queries
            IndexModel::builder()
                .keys(doc! { "dedup.status": 1 })
                .build(),
        ];

        self.collection.create_indexes(indexes).await.map_err(|e| {
            StorageError::IndexCreationFailed {
                collection: self.config.collection_name.clone(),
                message: e.to_string(),
            }
        })?;

        Ok(())
    }

    async fn save_batch(
        &self,
        docs: &[MediaMetadataDocument],
    ) -> Result<BatchWriteStats, StorageError> {
        if docs.is_empty() {
            return Ok(BatchWriteStats::default());
        }

        let mut models: Vec<WriteModel> = Vec::with_capacity(docs.len());

        for doc in docs {
            let bson_doc = doc_to_bson_document(doc)?;

            let filter = doc! {
                "file_context.file_path": doc.file_context.file_path.to_string_lossy().to_string()
            };

            let update = doc! {
                "$set": bson_doc
            };

            let update_model = UpdateOneModel::builder()
                .namespace(Namespace::new(
                    &self.config.database_name,
                    &self.config.collection_name,
                ))
                .filter(filter)
                .update(update)
                .upsert(true)
                .build();

            models.push(WriteModel::UpdateOne(update_model));
        }

        let mut attempts = 0u32;
        let mut backoff = Duration::from_millis(100);

        loop {
            attempts += 1;
            let result = self.client.bulk_write(models.clone()).ordered(false).await;

            match result {
                Ok(bulk_res) => {
                    return Ok(BatchWriteStats {
                        matched_count: bulk_res.matched_count.max(0) as u64,
                        modified_count: bulk_res.modified_count.max(0) as u64,
                        upserted_count: bulk_res.upserted_count.max(0) as u64,
                        failed_count: 0,
                    });
                }
                Err(err) => {
                    let is_transient = err
                        .contains_label(mongodb::error::TRANSIENT_TRANSACTION_ERROR)
                        || err.contains_label(mongodb::error::UNKNOWN_TRANSACTION_COMMIT_RESULT)
                        || err.to_string().contains("timed out")
                        || err.to_string().contains("connection closed");

                    if is_transient && attempts <= self.config.max_retries {
                        tracing::warn!(
                            attempt = attempts,
                            max_retries = self.config.max_retries,
                            error = %err,
                            "Transient MongoDB write error; retrying with backoff"
                        );
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                        continue;
                    }

                    return Err(StorageError::BulkWriteFailed {
                        collection: self.config.collection_name.clone(),
                        message: err.to_string(),
                        is_transient,
                    });
                }
            }
        }
    }

    async fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn filter_unmodified_items(
        &self,
        items: &[DiscoveredMediaItem],
    ) -> Result<FilterEvaluationResult, StorageError> {
        if items.is_empty() {
            return Ok(FilterEvaluationResult::default());
        }

        let paths: Vec<String> = items
            .iter()
            .map(|item| item.path.to_string_lossy().to_string())
            .collect();

        let filter = doc! {
            "file_context.file_path": { "$in": &paths }
        };

        let projection = doc! {
            "file_context.file_path": 1,
            "file_context.file_size_bytes": 1,
            "file_context.modified_at": 1,
        };

        let find_options = FindOptions::builder().projection(projection).build();

        let mut attempts = 0;
        let mut backoff = Duration::from_millis(100);

        let mut cursor = loop {
            attempts += 1;
            match self
                .collection
                .find(filter.clone())
                .with_options(find_options.clone())
                .await
            {
                Ok(cursor) => break cursor,
                Err(err) => {
                    let is_transient = err
                        .contains_label(mongodb::error::TRANSIENT_TRANSACTION_ERROR)
                        || err.contains_label(mongodb::error::UNKNOWN_TRANSACTION_COMMIT_RESULT)
                        || err.to_string().contains("timed out")
                        || err.to_string().contains("connection closed");

                    if is_transient && attempts <= self.config.max_retries {
                        tracing::warn!(
                            attempt = attempts,
                            max_retries = self.config.max_retries,
                            error = %err,
                            "Transient MongoDB query error during incremental filtering; retrying"
                        );
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                        continue;
                    }

                    return Err(StorageError::QueryFailed {
                        collection: self.config.collection_name.clone(),
                        message: err.to_string(),
                    });
                }
            }
        };

        let mut existing_map: HashMap<String, (u64, Option<DateTime<Utc>>)> = HashMap::new();

        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| StorageError::QueryFailed {
                collection: self.config.collection_name.clone(),
                message: format!("Error reading cursor during incremental filtering: {}", e),
            })?
        {
            if let Ok(file_ctx) = doc.get_document("file_context") {
                let file_path = file_ctx.get_str("file_path").ok();
                let file_size: Option<u64> = match file_ctx.get("file_size_bytes") {
                    Some(Bson::Int64(i)) if *i >= 0 => Some(*i as u64),
                    Some(Bson::Int32(i)) if *i >= 0 => Some(*i as u64),
                    _ => None,
                };
                let modified_at: Option<DateTime<Utc>> = match file_ctx.get("modified_at") {
                    Some(Bson::DateTime(dt)) => {
                        DateTime::from_timestamp_millis(dt.timestamp_millis())
                    }
                    Some(Bson::String(s)) => DateTime::parse_from_rfc3339(s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc)),
                    _ => None,
                };

                if let (Some(path), Some(size)) = (file_path, file_size) {
                    existing_map.insert(path.to_string(), (size, modified_at));
                }
            }
        }

        let mut unmodified_items = Vec::new();
        let mut items_to_extract = Vec::new();
        let mut skipped_bytes: u64 = 0;
        let mut extract_bytes: u64 = 0;

        for item in items {
            let path_key = item.path.to_string_lossy();
            if let Some((stored_size, stored_mtime)) = existing_map.get(path_key.as_ref()) {
                let size_matches = item.file_size_bytes == *stored_size;

                let mtime_matches = match (item.modified_at, stored_mtime) {
                    (Some(item_st), Some(db_dt)) => {
                        let item_dt: DateTime<Utc> = item_st.into();
                        (item_dt.timestamp_millis() - db_dt.timestamp_millis()).abs() <= 1000
                    }
                    (None, None) => true,
                    _ => false,
                };

                if size_matches && mtime_matches {
                    skipped_bytes = skipped_bytes.saturating_add(item.file_size_bytes);
                    unmodified_items.push(item.clone());
                    continue;
                }
            }

            extract_bytes = extract_bytes.saturating_add(item.file_size_bytes);
            items_to_extract.push(item.clone());
        }

        Ok(FilterEvaluationResult {
            unmodified_items,
            items_to_extract,
            skipped_bytes,
            extract_bytes,
        })
    }

    async fn query_duplicate_groups(
        &self,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        let pipeline = vec![
            doc! {
                "$match": {
                    "fingerprint.cryptographic_hash": {
                        "$ne": Bson::Null,
                        "$exists": true
                    }
                }
            },
            doc! {
                "$group": {
                    "_id": "$fingerprint.cryptographic_hash",
                    "count": { "$sum": 1 },
                    "docs": { "$push": "$$ROOT" }
                }
            },
            doc! {
                "$match": {
                    "count": { "$gt": 1 }
                }
            },
        ];

        let mut cursor =
            self.collection
                .aggregate(pipeline)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Duplicate hash aggregation failed: {}", e),
                })?;

        let mut duplicate_groups = Vec::new();
        while let Some(result_doc) =
            cursor
                .try_next()
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Failed reading aggregation cursor: {}", e),
                })?
        {
            let hash_str = result_doc.get_str("_id").unwrap_or_default().to_string();

            if hash_str.is_empty() {
                continue;
            }

            let raw_docs = result_doc.get_array("docs").cloned().unwrap_or_default();

            let mut parsed_docs = Vec::with_capacity(raw_docs.len());
            for item in raw_docs {
                if let Bson::Document(d) = item {
                    match bson_document_to_doc(d) {
                        Ok(parsed) => parsed_docs.push(parsed),
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                hash = %hash_str,
                                "Skipping unparseable document in duplicate group"
                            );
                        }
                    }
                }
            }

            if parsed_docs.len() > 1 {
                duplicate_groups.push((hash_str, parsed_docs));
            }
        }

        Ok(duplicate_groups)
    }

    async fn update_dedup_batch(
        &self,
        updates: &[(uuid::Uuid, DeduplicationMetadata)],
    ) -> Result<usize, StorageError> {
        if updates.is_empty() {
            return Ok(0);
        }

        let mut total_modified = 0;
        for chunk in updates.chunks(1000) {
            let mut models: Vec<WriteModel> = Vec::with_capacity(chunk.len());
            for (doc_id, dedup_meta) in chunk {
                let json_meta = serde_json::to_value(dedup_meta).map_err(|e| {
                    StorageError::SerializationError(format!(
                        "Failed to serialize DeduplicationMetadata: {}",
                        e
                    ))
                })?;
                let bson_meta = json_to_bson(json_meta);

                let filter = doc! { "id": doc_id.to_string() };
                let update = doc! { "$set": { "dedup": bson_meta } };

                let update_model = UpdateOneModel::builder()
                    .namespace(Namespace::new(
                        &self.config.database_name,
                        &self.config.collection_name,
                    ))
                    .filter(filter)
                    .update(update)
                    .build();

                models.push(WriteModel::UpdateOne(update_model));
            }

            let bulk_res = self
                .client
                .bulk_write(models)
                .ordered(false)
                .await
                .map_err(|e| StorageError::BulkWriteFailed {
                    collection: self.config.collection_name.clone(),
                    message: e.to_string(),
                    is_transient: false,
                })?;

            total_modified += bulk_res.modified_count.max(0) as usize;
        }

        Ok(total_modified)
    }

    async fn query_removal_candidates(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let mut filter = doc! {
            "dedup.status": DeduplicationStatus::MarkForRemoval.to_string()
        };

        apply_mount_prefix_filter(&mut filter, mount_prefix);

        let mut cursor =
            self.collection
                .find(filter)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Failed querying removal candidates: {}", e),
                })?;

        let mut candidates = Vec::new();
        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| StorageError::QueryFailed {
                collection: self.config.collection_name.clone(),
                message: format!("Cursor error on removal candidates: {}", e),
            })?
        {
            match bson_document_to_doc(doc) {
                Ok(parsed) => candidates.push(parsed),
                Err(err) => {
                    tracing::warn!(error = %err, "Skipping unparseable removal candidate document");
                }
            }
        }

        Ok(candidates)
    }

    async fn query_unmastered_candidates(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let mut filter = doc! {
            "dedup.status": DeduplicationStatus::NeedsIngestToMaster.to_string()
        };

        apply_mount_prefix_filter(&mut filter, mount_prefix);

        let mut cursor =
            self.collection
                .find(filter)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Failed querying unmastered candidates: {}", e),
                })?;

        let mut candidates = Vec::new();
        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| StorageError::QueryFailed {
                collection: self.config.collection_name.clone(),
                message: format!("Cursor error on unmastered candidates: {}", e),
            })?
        {
            match bson_document_to_doc(doc) {
                Ok(parsed) => candidates.push(parsed),
                Err(err) => {
                    tracing::warn!(error = %err, "Skipping unparseable unmastered candidate document");
                }
            }
        }

        Ok(candidates)
    }

    async fn find_document_by_id(
        &self,
        doc_id: uuid::Uuid,
    ) -> Result<Option<MediaMetadataDocument>, StorageError> {
        let filter = doc! { "id": doc_id.to_string() };
        let opt_doc =
            self.collection
                .find_one(filter)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Failed finding document {}: {}", doc_id, e),
                })?;

        match opt_doc {
            Some(doc) => bson_document_to_doc(doc).map(Some),
            None => Ok(None),
        }
    }

    async fn mark_removed(&self, doc_id: uuid::Uuid) -> Result<(), StorageError> {
        let filter = doc! { "id": doc_id.to_string() };
        let update = doc! {
            "$set": {
                "dedup.status": DeduplicationStatus::Removed.to_string()
            }
        };

        self.collection
            .update_one(filter, update)
            .await
            .map_err(|e| StorageError::QueryFailed {
                collection: self.config.collection_name.clone(),
                message: format!("Failed marking document {} as removed: {}", doc_id, e),
            })?;

        Ok(())
    }

    async fn query_all_content_groups(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        let mut match_criteria = doc! {
            "fingerprint.cryptographic_hash": {
                "$ne": Bson::Null,
                "$exists": true
            }
        };

        apply_mount_prefix_filter(&mut match_criteria, mount_prefix);

        let pipeline = vec![
            doc! {
                "$match": match_criteria
            },
            doc! {
                "$group": {
                    "_id": "$fingerprint.cryptographic_hash",
                    "count": { "$sum": 1 },
                    "docs": { "$push": "$$ROOT" }
                }
            },
        ];

        let mut cursor =
            self.collection
                .aggregate(pipeline)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Universal content group aggregation failed: {}", e),
                })?;

        let mut content_groups = Vec::new();
        while let Some(result_doc) =
            cursor
                .try_next()
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Failed reading aggregation cursor: {}", e),
                })?
        {
            let hash_str = result_doc.get_str("_id").unwrap_or_default().to_string();

            if hash_str.is_empty() {
                continue;
            }

            let raw_docs = result_doc.get_array("docs").cloned().unwrap_or_default();
            let mut parsed_docs = Vec::with_capacity(raw_docs.len());

            for raw_val in raw_docs {
                if let Bson::Document(d) = raw_val {
                    match bson_document_to_doc(d) {
                        Ok(parsed) => parsed_docs.push(parsed),
                        Err(err) => {
                            tracing::warn!(
                                hash = %hash_str,
                                error = %err,
                                "Skipping unparseable document in content group"
                            );
                        }
                    }
                }
            }

            if !parsed_docs.is_empty() {
                content_groups.push((hash_str, parsed_docs));
            }
        }

        Ok(content_groups)
    }

    async fn query_canonical_masters(&self) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let filter = doc! {
            "dedup.status": DeduplicationStatus::CanonicalMaster.to_string()
        };

        let mut cursor =
            self.collection
                .find(filter)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Failed querying canonical masters: {}", e),
                })?;

        let mut masters = Vec::new();
        while let Some(raw_doc) =
            cursor
                .try_next()
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Cursor error on canonical masters: {}", e),
                })?
        {
            match bson_document_to_doc(raw_doc) {
                Ok(doc) => masters.push(doc),
                Err(err) => {
                    tracing::warn!(error = %err, "Skipping unparseable canonical master document");
                }
            }
        }

        Ok(masters)
    }

    async fn query_replica_documents(
        &self,
        replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let filter = doc! {
            "dedup.status": DeduplicationStatus::ReplicaKeep.to_string(),
            "dedup.drive_id": replica_drive_id
        };

        let mut cursor =
            self.collection
                .find(filter)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!(
                        "Failed querying replica documents for drive '{}': {}",
                        replica_drive_id, e
                    ),
                })?;

        let mut replicas = Vec::new();
        while let Some(raw_doc) =
            cursor
                .try_next()
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!("Cursor error on replica documents: {}", e),
                })?
        {
            match bson_document_to_doc(raw_doc) {
                Ok(doc) => replicas.push(doc),
                Err(err) => {
                    tracing::warn!(error = %err, "Skipping unparseable replica document");
                }
            }
        }

        Ok(replicas)
    }

    async fn count_unfingerprinted_records(&self) -> Result<u64, StorageError> {
        let filter = doc! {
            "$or": [
                { "fingerprint": { "$exists": false } },
                { "fingerprint": Bson::Null },
                { "fingerprint.cryptographic_hash": { "$exists": false } },
                { "fingerprint.cryptographic_hash": Bson::Null },
            ]
        };
        self.collection
            .count_documents(filter)
            .await
            .map_err(|e| StorageError::QueryFailed {
                collection: self.config.collection_name.clone(),
                message: format!("Failed counting unfingerprinted documents: {}", e),
            })
    }

    async fn link_canonical_masters(
        &self,
        links: &[(String, uuid::Uuid, std::path::PathBuf)],
    ) -> Result<usize, StorageError> {
        if links.is_empty() {
            return Ok(0);
        }

        let mut total_updated = 0usize;

        for (hash, master_id, master_path) in links {
            // 1. If any non-master records were NeedsIngestToMaster, update status to MarkForRemoval
            let filter_needs_ingest = doc! {
                "fingerprint.cryptographic_hash": hash,
                "dedup.status": DeduplicationStatus::NeedsIngestToMaster.to_string(),
            };
            let update_needs_ingest = doc! {
                "$set": {
                    "dedup.status": DeduplicationStatus::MarkForRemoval.to_string(),
                    "dedup.canonical_master_id": master_id.to_string(),
                    "dedup.canonical_master_path": master_path.to_string_lossy().to_string(),
                }
            };
            let _ = self
                .collection
                .update_many(filter_needs_ingest, update_needs_ingest)
                .await;

            // 2. For all non-master records sharing this hash, ensure canonical_master_id and canonical_master_path are set
            let filter_all = doc! {
                "fingerprint.cryptographic_hash": hash,
                "dedup.status": { "$ne": DeduplicationStatus::CanonicalMaster.to_string() }
            };
            let update_all = doc! {
                "$set": {
                    "dedup.canonical_master_id": master_id.to_string(),
                    "dedup.canonical_master_path": master_path.to_string_lossy().to_string(),
                }
            };

            let res = self
                .collection
                .update_many(filter_all, update_all)
                .await
                .map_err(|e| StorageError::QueryFailed {
                    collection: self.config.collection_name.clone(),
                    message: format!(
                        "Failed linking duplicates to canonical master {} for hash {}: {}",
                        master_id, hash, e
                    ),
                })?;

            total_updated += res.modified_count as usize;
        }

        Ok(total_updated)
    }
}

/// Converts a `serde_json::Value` to `mongodb::bson::Bson`, safely reinterpreting
/// unsigned 64-bit integers exceeding i64::MAX as two's complement signed i64.
fn json_to_bson(value: serde_json::Value) -> mongodb::bson::Bson {
    match value {
        serde_json::Value::Null => mongodb::bson::Bson::Null,
        serde_json::Value::Bool(b) => mongodb::bson::Bson::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                mongodb::bson::Bson::Int64(i)
            } else if let Some(u) = n.as_u64() {
                mongodb::bson::Bson::Int64(u as i64)
            } else if let Some(f) = n.as_f64() {
                mongodb::bson::Bson::Double(f)
            } else {
                mongodb::bson::Bson::Null
            }
        }
        serde_json::Value::String(s) => mongodb::bson::Bson::String(s),
        serde_json::Value::Array(arr) => {
            mongodb::bson::Bson::Array(arr.into_iter().map(json_to_bson).collect())
        }
        serde_json::Value::Object(map) => {
            let mut doc = Document::new();
            for (k, v) in map {
                doc.insert(k, json_to_bson(v));
            }
            mongodb::bson::Bson::Document(doc)
        }
    }
}

/// Safely serializes a `MediaMetadataDocument` into a BSON `Document`.
fn doc_to_bson_document(doc: &MediaMetadataDocument) -> Result<Document, StorageError> {
    let json_val = serde_json::to_value(doc).map_err(|e| {
        StorageError::SerializationError(format!(
            "Failed to serialize {}: {}",
            doc.file_context.file_path.display(),
            e
        ))
    })?;

    match json_to_bson(json_val) {
        mongodb::bson::Bson::Document(d) => Ok(d),
        _ => Err(StorageError::SerializationError(
            "Expected BSON document from media record".to_string(),
        )),
    }
}

/// Converts a `mongodb::bson::Bson` value to a `serde_json::Value`.
fn bson_to_json(value: mongodb::bson::Bson) -> serde_json::Value {
    match value {
        mongodb::bson::Bson::Null => serde_json::Value::Null,
        mongodb::bson::Bson::Boolean(b) => serde_json::Value::Bool(b),
        mongodb::bson::Bson::Int32(i) => serde_json::Value::Number(i.into()),
        mongodb::bson::Bson::Int64(i) => {
            if i >= 0 {
                serde_json::json!(i)
            } else {
                // In BSON, 64-bit unsigned integers (such as pHash) with high bit set
                // wrap around into negative i64 values. Reinterpret back to u64 for serde.
                serde_json::Value::Number(serde_json::Number::from(i as u64))
            }
        }
        mongodb::bson::Bson::Double(f) => {
            if let Some(n) = serde_json::Number::from_f64(f) {
                serde_json::Value::Number(n)
            } else {
                serde_json::Value::Null
            }
        }
        mongodb::bson::Bson::String(s) => serde_json::Value::String(s),
        mongodb::bson::Bson::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(bson_to_json).collect())
        }
        mongodb::bson::Bson::Document(doc) => {
            let mut map = serde_json::Map::new();
            for (k, v) in doc {
                map.insert(k, bson_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        mongodb::bson::Bson::DateTime(dt) => {
            let chrono_dt =
                chrono::DateTime::from_timestamp_millis(dt.timestamp_millis()).unwrap_or_default();
            serde_json::Value::String(chrono_dt.to_rfc3339())
        }
        mongodb::bson::Bson::Binary(bin) => serde_json::json!(bin.bytes),
        _ => serde_json::Value::Null,
    }
}

/// Safely deserializes a BSON `Document` into a `MediaMetadataDocument`.
pub fn bson_document_to_doc(doc: Document) -> Result<MediaMetadataDocument, StorageError> {
    let json_val = bson_to_json(mongodb::bson::Bson::Document(doc));
    serde_json::from_value(json_val).map_err(|e| {
        StorageError::DeserializationError(format!(
            "Failed to deserialize MediaMetadataDocument: {}",
            e
        ))
    })
}

/// Escapes special regex characters in a literal string.
fn regex_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '|' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Applies a path prefix match to a MongoDB query filter, supporting both raw and canonical paths.
fn apply_mount_prefix_filter(filter: &mut Document, mount_prefix: Option<&str>) {
    if let Some(prefix) = mount_prefix {
        let trimmed = prefix.trim();
        let trimmed_path =
            if trimmed.len() > 1 && (trimmed.ends_with('/') || trimmed.ends_with('\\')) {
                trimmed.trim_end_matches(['/', '\\'])
            } else {
                trimmed
            };
        let escaped = regex_escape(trimmed_path);
        let mut or_clauses =
            vec![doc! { "file_context.file_path": { "$regex": format!("^{}", escaped) } }];

        if let Ok(canon) = std::fs::canonicalize(trimmed_path) {
            let canon_str = canon.to_string_lossy().to_string();
            let canon_trimmed =
                if canon_str.len() > 1 && (canon_str.ends_with('/') || canon_str.ends_with('\\')) {
                    canon_str.trim_end_matches(['/', '\\'])
                } else {
                    &canon_str
                };
            if canon_trimmed != trimmed_path {
                let escaped_canon = regex_escape(canon_trimmed);
                or_clauses.push(
                    doc! { "file_context.file_path": { "$regex": format!("^{}", escaped_canon) } },
                );
            }
        }

        if or_clauses.len() == 1 {
            filter.insert(
                "file_context.file_path",
                doc! { "$regex": format!("^{}", escaped) },
            );
        } else {
            filter.insert("$or", or_clauses);
        }
    }
}
