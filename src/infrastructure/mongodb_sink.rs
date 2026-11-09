use chrono::{DateTime, Utc};
use futures::stream::TryStreamExt;
use mongodb::bson::{doc, Bson, Document};
use mongodb::options::{ClientOptions, FindOptions, IndexOptions, UpdateOneModel, WriteModel};
use mongodb::{Client, Collection, Database, IndexModel, Namespace};
use std::collections::HashMap;
use std::time::Duration;

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
