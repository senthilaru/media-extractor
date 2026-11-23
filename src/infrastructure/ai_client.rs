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
    #[serde(alias = "box")]
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
pub struct HealthResponse {
    pub status: String,
    pub service: Option<String>,
    #[serde(default)]
    pub has_yolo: bool,
    #[serde(default)]
    pub has_faces: bool,
    #[serde(default)]
    pub has_ocr: bool,
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
            .timeout(Duration::from_secs(300))
            .pool_idle_timeout(Duration::from_secs(10))
            .tcp_keepalive(Duration::from_secs(60))
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

    /// Query the full health and capabilities report from the sidecar
    pub async fn query_health(&self) -> Result<HealthResponse, AiClientError> {
        let endpoint = format!("{}/health", self.base_url);
        let resp = self.http_client.get(&endpoint).send().await?;
        let health: HealthResponse = resp.json().await?;
        Ok(health)
    }

    /// Ensure sidecar is running; if not, spawn it and wait until healthy
    pub async fn ensure_service_running(&self, sidecar_script_dir: Option<&Path>) -> Result<(), AiClientError> {
        if self.check_health().await.unwrap_or(false) {
            debug!("AI sidecar already running and healthy at {}", self.base_url);
            self.warn_if_models_missing().await;
            return Ok(());
        }

        info!("AI sidecar not detected at {}. Attempting to spawn process...", self.base_url);

        let script_dir = sidecar_script_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
                if PathBuf::from("ai-service/service.py").exists() {
                    PathBuf::from("ai-service")
                } else {
                    PathBuf::from(".")
                }
            });

        let service_script = script_dir.join("service.py");
        if !service_script.exists() {
            warn!("AI sidecar script not found at {}. Assuming external management.", service_script.display());
            return Ok(());
        }

        // Auto-bootstrap virtual environment if missing
        let mut venv_python = script_dir.join(".venv/bin/python3");
        if !venv_python.exists() {
            let setup_script = script_dir.join("setup.sh");
            if setup_script.exists() {
                eprintln!("================================================================================");
                eprintln!("[AI Service] First-time setup: Initializing AI virtual environment...");
                eprintln!("             Running ai-service/setup.sh to install PyTorch, YOLOv8 & InsightFace...");
                eprintln!("================================================================================");
                let _ = Command::new("bash")
                    .arg(&setup_script)
                    .current_dir(&script_dir)
                    .status();
                venv_python = script_dir.join(".venv/bin/python3");
            }
        }

        let script_abs = std::fs::canonicalize(&service_script)
            .unwrap_or_else(|_| service_script.clone());

        let (python_cmd, venv_dir) = if venv_python.exists() {
            let abs_venv_py = std::env::current_dir()
                .map(|cwd| cwd.join(&venv_python))
                .unwrap_or_else(|_| venv_python.clone());
            let abs_venv = std::env::current_dir()
                .map(|cwd| cwd.join(script_dir.join(".venv")))
                .unwrap_or_else(|_| script_dir.join(".venv"));
            (abs_venv_py.to_string_lossy().to_string(), Some(abs_venv))
        } else if let Ok(venv_py) = std::env::var("VIRTUAL_ENV") {
            let p = PathBuf::from(&venv_py).join("bin/python3");
            if p.exists() {
                (p.to_string_lossy().to_string(), Some(PathBuf::from(venv_py)))
            } else {
                ("python3".to_string(), None)
            }
        } else {
            ("python3".to_string(), None)
        };

        let mut cmd = Command::new(&python_cmd);
        cmd.arg(&script_abs)
            .current_dir(&script_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        if let Some(ref venv) = venv_dir {
            cmd.env("VIRTUAL_ENV", venv);
            let bin_dir = venv.join("bin");
            let existing_path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{}:{}", bin_dir.display(), existing_path));
        }

        let child = cmd.spawn().map_err(AiClientError::ProcessSpawn)?;

        {
            let mut lock = self.child_process.lock().unwrap();
            *lock = Some(child);
        }

        // Wait up to 15 seconds for health endpoint to respond
        for attempt in 1..=75 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if self.check_health().await.unwrap_or(false) {
                info!("AI sidecar successfully spawned and ready (attempt {})", attempt);
                self.warn_if_models_missing().await;
                return Ok(());
            }
        }

        Err(AiClientError::Timeout(format!(
            "Sidecar process failed to become ready at {} within 15 seconds",
            self.base_url
        )))
    }

    async fn warn_if_models_missing(&self) {
        if let Ok(health) = self.query_health().await {
            if !health.has_yolo {
                eprintln!("⚠️  [AI Notice] YOLOv8 is not active. Object recognition will be skipped.");
                eprintln!("   To enable object recognition, run: bash ai-service/setup.sh");
            }
            if !health.has_faces {
                eprintln!("⚠️  [AI Notice] InsightFace is not active. Facial recognition will be skipped.");
                eprintln!("   To enable face recognition, run: bash ai-service/setup.sh");
            }
        }
    }

    /// Helper to post JSON with automatic retry on broken pooled socket or transient IPC connection reset
    async fn post_with_retry<T: Serialize + ?Sized, R: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        payload: &T,
    ) -> Result<R, AiClientError> {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let resp_res = self
                .http_client
                .post(endpoint)
                .json(payload)
                .send()
                .await;

            match resp_res {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        let status = resp.status().as_u16();
                        let body = resp.text().await.unwrap_or_default();
                        return Err(AiClientError::ServiceError(status, body));
                    }
                    let data: R = resp.json().await?;
                    return Ok(data);
                }
                Err(e) => {
                    if attempts < 3 && (e.is_connect() || e.is_request() || e.is_timeout()) {
                        warn!(
                            "AI sidecar IPC request to {} failed ({}), retrying attempt {}...",
                            endpoint,
                            e,
                            attempts + 1
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                    return Err(AiClientError::Http(e));
                }
            }
        }
    }

    /// Batch infer objects and occasions
    pub async fn enrich_objects_scenes(&self, paths: &[PathBuf]) -> Result<Vec<ObjectSceneResponse>, AiClientError> {
        let string_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let endpoint = format!("{}/v1/enrich/objects-scenes", self.base_url);
        self.post_with_retry(&endpoint, &BatchPathsRequest { file_paths: &string_paths }).await
    }

    /// Batch infer faces, expressions, and embeddings
    pub async fn enrich_faces(&self, paths: &[PathBuf]) -> Result<Vec<FaceResponse>, AiClientError> {
        let string_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let endpoint = format!("{}/v1/enrich/faces", self.base_url);
        self.post_with_retry(&endpoint, &BatchPathsRequest { file_paths: &string_paths }).await
    }

    /// Batch infer text OCR
    pub async fn enrich_ocr(&self, paths: &[PathBuf]) -> Result<Vec<OcrResponse>, AiClientError> {
        let string_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let endpoint = format!("{}/v1/enrich/ocr", self.base_url);
        self.post_with_retry(&endpoint, &BatchPathsRequest { file_paths: &string_paths }).await
    }
}

impl Drop for AiSidecarClient {
    fn drop(&mut self) {
        // Only terminate child process if this is the last remaining owner
        if Arc::strong_count(&self.child_process) == 1 {
            if let Ok(mut lock) = self.child_process.lock() {
                if let Some(mut child) = lock.take() {
                    debug!("Terminating spawned AI sidecar child process");
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}
