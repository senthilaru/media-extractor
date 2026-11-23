use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const FACE_CLUSTER_THRESHOLD_COSINE_DISTANCE: f32 = 0.38;

/// An aggregated identity cluster representing a recurring individual across the photo catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonCluster {
    pub id: Uuid,
    pub display_name: Option<String>,
    pub avatar_asset_id: Option<Uuid>,
    pub centroid_embedding: Vec<f32>,
    pub total_photo_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PersonCluster {
    pub fn new(initial_embedding: Vec<f32>, avatar_asset_id: Option<Uuid>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            display_name: None,
            avatar_asset_id,
            centroid_embedding: normalize_vector(&initial_embedding),
            total_photo_count: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// Computes cosine similarity between this cluster's centroid and a query embedding
    pub fn cosine_similarity(&self, embedding: &[f32]) -> f32 {
        cosine_similarity(&self.centroid_embedding, embedding)
    }

    /// Computes cosine distance (1.0 - similarity)
    pub fn cosine_distance(&self, embedding: &[f32]) -> f32 {
        1.0 - self.cosine_similarity(embedding)
    }

    /// Checks if a face embedding matches this identity cluster within the similarity threshold
    pub fn matches(&self, embedding: &[f32], threshold: f32) -> bool {
        self.cosine_distance(embedding) <= threshold
    }

    /// Incorporates a new face embedding, updating the running centroid
    pub fn incorporate_face(&mut self, embedding: &[f32]) {
        if self.centroid_embedding.len() != embedding.len() {
            return;
        }

        let n = self.total_photo_count as f32;
        let mut new_centroid = Vec::with_capacity(embedding.len());
        for i in 0..embedding.len() {
            let updated = (self.centroid_embedding[i] * n + embedding[i]) / (n + 1.0);
            new_centroid.push(updated);
        }

        self.centroid_embedding = normalize_vector(&new_centroid);
        self.total_photo_count += 1;
        self.updated_at = Utc::now();
    }

    /// Merges another cluster into this one
    pub fn merge_with(&mut self, other: &PersonCluster) {
        if self.centroid_embedding.len() != other.centroid_embedding.len() {
            return;
        }

        let n1 = self.total_photo_count as f32;
        let n2 = other.total_photo_count as f32;
        let total = n1 + n2;

        if total > 0.0 {
            let mut merged_centroid = Vec::with_capacity(self.centroid_embedding.len());
            for i in 0..self.centroid_embedding.len() {
                let merged_val = (self.centroid_embedding[i] * n1 + other.centroid_embedding[i] * n2) / total;
                merged_centroid.push(merged_val);
            }
            self.centroid_embedding = normalize_vector(&merged_centroid);
        }

        self.total_photo_count += other.total_photo_count;
        if self.display_name.is_none() && other.display_name.is_some() {
            self.display_name = other.display_name.clone();
        }
        self.updated_at = Utc::now();
    }
}

pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn vector_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

pub fn normalize_vector(v: &[f32]) -> Vec<f32> {
    let norm = vector_norm(v);
    if norm > 0.0 {
        v.iter().map(|x| x / norm).collect()
    } else {
        v.to_vec()
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot = dot_product(a, b);
    let norm_a = vector_norm(a);
    let norm_b = vector_norm(b);
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
    }
}
