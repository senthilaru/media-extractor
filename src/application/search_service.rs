use std::path::PathBuf;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::domain::errors::ExtractionError;
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::ports::MetadataStorageSink;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchOutputFormat {
    Table,
    Json,
    Paths,
}

impl std::str::FromStr for SearchOutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(SearchOutputFormat::Table),
            "json" => Ok(SearchOutputFormat::Json),
            "paths" => Ok(SearchOutputFormat::Paths),
            other => Err(format!("Unknown format '{other}', expected table, json, or paths")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchCriteria {
    pub query: Option<String>,
    pub person: Option<String>,
    pub cluster_id: Option<uuid::Uuid>,
    pub expression: Option<String>,
    pub object: Option<String>,
    pub occasion: Option<String>,
    pub text: Option<String>,
    pub year: Option<u32>,
}

pub struct SearchService {
    storage: Arc<dyn MetadataStorageSink>,
}

impl SearchService {
    pub fn new(storage: Arc<dyn MetadataStorageSink>) -> Self {
        Self { storage }
    }

    /// Evaluates search criteria against canonical masters in storage
    pub async fn execute_search(
        &self,
        criteria: &SearchCriteria,
    ) -> Result<Vec<MediaMetadataDocument>, ExtractionError> {
        let all_masters = self
            .storage
            .query_catalog_for_visual()
            .await
            .map_err(|e| ExtractionError::InferenceFailed {
                path: PathBuf::new(),
                message: format!("Search query error: {e}"),
            })?;

        let target_person_id = if let Some(cid) = criteria.cluster_id {
            Some(cid)
        } else if let Some(ref person_name) = criteria.person {
            let clusters = self.storage.list_person_clusters().await.unwrap_or_default();
            let name_lower = person_name.to_lowercase();
            clusters
                .iter()
                .find(|c| {
                    c.display_name
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&name_lower))
                        .unwrap_or(false)
                })
                .map(|c| c.id)
        } else {
            None
        };

        let results: Vec<MediaMetadataDocument> = all_masters
            .into_iter()
            .filter(|doc| self.matches_criteria(doc, criteria, target_person_id))
            .collect();

        Ok(results)
    }

    fn matches_criteria(
        &self,
        doc: &MediaMetadataDocument,
        criteria: &SearchCriteria,
        target_person_id: Option<uuid::Uuid>,
    ) -> bool {
        // 1. Calendar year filter
        if let Some(target_year) = criteria.year {
            if doc.temporal.year as u32 != target_year {
                return false;
            }
        }

        let visual = match &doc.visual {
            Some(v) => v,
            None => {
                // If visual criteria are requested but document has no visual metadata, reject
                if criteria.object.is_some()
                    || criteria.occasion.is_some()
                    || criteria.expression.is_some()
                    || criteria.text.is_some()
                    || criteria.person.is_some()
                    || criteria.cluster_id.is_some()
                    || criteria.query.is_some()
                {
                    return false;
                }
                return true;
            }
        };

        // 2. Person / Cluster filter
        if criteria.person.is_some() || criteria.cluster_id.is_some() {
            match target_person_id {
                Some(pid) => {
                    let has_person = visual.faces.iter().any(|f| f.person_id == Some(pid));
                    if !has_person {
                        return false;
                    }
                }
                None => return false, // Person named in criteria not found in any cluster
            }
        }

        // 2. Object filter
        if let Some(ref obj_target) = criteria.object {
            let target_lower = obj_target.to_lowercase();
            let has_obj = visual
                .objects
                .iter()
                .any(|o| o.label.to_lowercase().contains(&target_lower));
            if !has_obj {
                return false;
            }
        }

        // 3. Occasion filter
        if let Some(ref occ_target) = criteria.occasion {
            let target_lower = occ_target.to_lowercase();
            let has_occ = visual
                .occasions
                .iter()
                .any(|occ| occ.category.to_lowercase().contains(&target_lower));
            if !has_occ {
                return false;
            }
        }

        // 4. Expression filter
        if let Some(ref expr_target) = criteria.expression {
            let target_lower = expr_target.to_lowercase();
            let has_expr = visual.faces.iter().any(|f| {
                f.expressions
                    .iter()
                    .any(|e| e.label.to_lowercase().contains(&target_lower))
            });
            if !has_expr {
                return false;
            }
        }

        // 5. In-image text / OCR filter
        if let Some(ref text_target) = criteria.text {
            let target_lower = text_target.to_lowercase();
            let has_text = visual.text_snippets.iter().any(|s| {
                s.text.to_lowercase().contains(&target_lower)
                    || s.tokens.iter().any(|t| t.to_lowercase().contains(&target_lower))
            }) || visual
                .search_tokens
                .iter()
                .any(|t| t.to_lowercase().contains(&target_lower));

            if !has_text {
                return false;
            }
        }

        // 6. Generic query keyword filter (searches objects, occasions, tokens, and filename)
        if let Some(ref q) = criteria.query {
            let q_lower = q.to_lowercase();
            let match_obj = visual.objects.iter().any(|o| o.label.to_lowercase().contains(&q_lower));
            let match_occ = visual.occasions.iter().any(|o| o.category.to_lowercase().contains(&q_lower));
            let match_text = visual.search_tokens.iter().any(|t| t.to_lowercase().contains(&q_lower));
            let match_filename = doc.file_context.file_name.to_lowercase().contains(&q_lower);

            if !(match_obj || match_occ || match_text || match_filename) {
                return false;
            }
        }

        true
    }

    /// Renders search results to string according to requested format
    pub fn format_results(&self, docs: &[MediaMetadataDocument], format: SearchOutputFormat) -> String {
        match format {
            SearchOutputFormat::Paths => {
                docs.iter()
                    .map(|d| d.file_context.file_path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            SearchOutputFormat::Json => {
                serde_json::to_string_pretty(docs).unwrap_or_else(|_| "[]".to_string())
            }
            SearchOutputFormat::Table => {
                if docs.is_empty() {
                    return "No matching media assets found.".to_string();
                }
                let mut out = format!(
                    "{:<36} | {:<4} | {:<25} | {:<30}\n",
                    "ID", "Year", "Objects", "Path"
                );
                out.push_str(&"-".repeat(105));
                out.push('\n');

                for doc in docs.iter().take(50) {
                    let objects_str = doc
                        .visual
                        .as_ref()
                        .map(|v| {
                            v.objects
                                .iter()
                                .map(|o| o.label.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();

                    let truncated_obj = if objects_str.len() > 24 {
                        format!("{}...", &objects_str[..21])
                    } else {
                        objects_str
                    };

                    let path_str = doc.file_context.file_path.display().to_string();
                    let truncated_path = if path_str.len() > 30 {
                        format!("...{}", &path_str[path_str.len() - 27..])
                    } else {
                        path_str
                    };

                    out.push_str(&format!(
                        "{:<36} | {:<4} | {:<25} | {:<30}\n",
                        doc.id, doc.temporal.year, truncated_obj, truncated_path
                    ));
                }

                if docs.len() > 50 {
                    out.push_str(&format!("\n... and {} more assets.", docs.len() - 50));
                }
                out
            }
        }
    }
}
