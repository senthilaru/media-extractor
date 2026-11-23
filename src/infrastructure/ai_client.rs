use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum AiClientError {
    #[error("HTTP IPC request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to spawn sidecar process: {0}")]
    ProcessSpawn(std::io::Error),

    #[error("AI sidecar service timed out waiting for health check: {0}")]
    Timeout(String),

    #[error("AI sidecar returned status {0}: {1}")]
    ServiceError(u16, String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedObjectDto {
    pub label: String,
    pub confidence: f32,
    #[serde(alias = "box", default)]
    pub box_coordinates: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedOccasionDto {
    pub tag: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectSceneResponse {
    pub file_path: String,
    pub objects: Vec<DetectedObjectDto>,
    pub occasions: Vec<DetectedOccasionDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedFaceDto {
    pub box_coordinates: [f32; 4],
    pub confidence: f32,
    pub expression: String,
    pub expression_score: f32,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceResponse {
    pub file_path: String,
    pub faces: Vec<DetectedFaceDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedTextSnippetDto {
    pub text: String,
    pub box_coordinates: Option<[f32; 4]>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResponse {
    pub file_path: String,
    pub text_snippets: Vec<ExtractedTextSnippetDto>,
    pub search_tokens: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BatchPathsRequest<'a> {
    file_paths: &'a [String],
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HealthResponse {
    status: String,
    service: String,
}

#[derive(Clone)]
pub struct AiSidecarClient {
    base_url: String,
    http_client: reqwest::Client,
    child_process: Arc<Mutex<Option<Child>>>,
}

impl AiSidecarClient {
    pub fn new(base_url: Option<String>) -> Self {
        let url = base_url.unwrap_or_else(|| {
            std::env::var("AI_SERVICE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".to_string())
        });

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client for AI sidecar");

        Self {
            base_url: url,
            http_client,
            child_process: Arc::new(Mutex::new(None)),
        }
    }

    /// Check if the AI sidecar is responsive
    pub async fn check_health(&self) -> Result<bool, AiClientError> {
        let endpoint = format!("{}/health", self.base_url);
        match self.http_client.get(&endpoint).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let health: HealthResponse = resp.json().await?;
                    Ok(health.status == "ok")
                } else {
                    Ok(false)
                }
            }
            Err(_) => Ok(false),
        }
    }

    /// Ensure sidecar is running; if not, spawn it and wait until healthy
    pub async fn ensure_service_running(&self, sidecar_script_dir: Option<&Path>) -> Result<(), AiClientError> {
        if self.check_health().await.unwrap_or(false) {
            debug!("AI sidecar already running and healthy at {}", self.base_url);
            return Ok(());
        }

        info!("AI sidecar not detected at {}. Attempting to spawn process...", self.base_url);

        let script_dir = sidecar_script_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("ai-service"));

        let service_script = script_dir.join("service.py");
        if !service_script.exists() {
            warn!("AI sidecar script not found at {}. Assuming external management.", service_script.display());
            return Ok(());
        }

        let python_cmd = if let Ok(venv_py) = std::env::var("VIRTUAL_ENV") {
            let p = PathBuf::from(venv_py).join("bin/python3");
            if p.exists() { p.to_string_lossy().to_string() } else { "python3".to_string() }
        } else {
            "python3".to_string()
        };

        let child = Command::new(&python_cmd)
            .arg(&service_script)
            .current_dir(&script_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(AiClientError::ProcessSpawn)?;

        {
            let mut lock = self.child_process.lock().unwrap();
            *lock = Some(child);
        }

        // Wait up to 5 seconds for health endpoint to respond
        for attempt in 1..=25 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if self.check_health().await.unwrap_or(false) {
                info!("AI sidecar successfully spawned and ready (attempt {})", attempt);
                return Ok(());
            }
        }

        Err(AiClientError::Timeout(format!(
            "Sidecar process failed to become ready at {} within 5 seconds",
            self.base_url
        )))
    }

    /// Batch infer objects and occasions
    pub async fn enrich_objects_scenes(&self, paths: &[PathBuf]) -> Result<Vec<ObjectSceneResponse>, AiClientError> {
        let string_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let endpoint = format!("{}/v1/enrich/objects-scenes", self.base_url);

        let resp = self
            .http_client
            .post(&endpoint)
            .json(&BatchPathsRequest { file_paths: &string_paths })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(AiClientError::ServiceError(status, body));
        }

        let results: Vec<ObjectSceneResponse> = resp.json().await?;
        Ok(results)
    }

    /// Batch infer faces, expressions, and embeddings
    pub async fn enrich_faces(&self, paths: &[PathBuf]) -> Result<Vec<FaceResponse>, AiClientError> {
        let string_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let endpoint = format!("{}/v1/enrich/faces", self.base_url);

        let resp = self
            .http_client
            .post(&endpoint)
            .json(&BatchPathsRequest { file_paths: &string_paths })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(AiClientError::ServiceError(status, body));
        }

        let results: Vec<FaceResponse> = resp.json().await?;
        Ok(results)
    }

    /// Batch infer text OCR
    pub async fn enrich_ocr(&self, paths: &[PathBuf]) -> Result<Vec<OcrResponse>, AiClientError> {
        let string_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let endpoint = format!("{}/v1/enrich/ocr", self.base_url);

        let resp = self
            .http_client
            .post(&endpoint)
            .json(&BatchPathsRequest { file_paths: &string_paths })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(AiClientError::ServiceError(status, body));
        }

        let results: Vec<OcrResponse> = resp.json().await?;
        Ok(results)
    }
}

impl Drop for AiSidecarClient {
    fn drop(&mut self) {
        if let Ok(mut lock) = self.child_process.lock() {
            if let Some(mut child) = lock.take() {
                debug!("Terminating spawned AI sidecar child process");
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
