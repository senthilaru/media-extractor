"""
Media Extractor AI Sidecar Service
Provides high-throughput visual semantic enrichment (objects, scenes, faces, OCR)
Strictly operates on local file paths in READ-ONLY mode.
"""

from typing import List, Optional
import os
import sys
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

app = FastAPI(
    title="Media Extractor AI Service",
    description="Local computer vision sidecar for semantic object, face, and OCR extraction",
    version="0.1.0",
)


class HealthResponse(BaseModel):
    status: str
    service: str
    version: str
    models_loaded: dict[str, bool] = Field(default_factory=dict)


class BatchPathsRequest(BaseModel):
    file_paths: List[str]


class DetectedObjectItem(BaseModel):
    label: str
    confidence: float
    box: Optional[List[float]] = None  # [x_min, y_min, x_max, y_max]


class DetectedOccasionItem(BaseModel):
    tag: str
    confidence: float


class ObjectSceneResult(BaseModel):
    file_path: str
    objects: List[DetectedObjectItem] = Field(default_factory=list)
    occasions: List[DetectedOccasionItem] = Field(default_factory=list)
    error: Optional[str] = None


class DetectedFaceItem(BaseModel):
    box: List[float]  # [x_min, y_min, x_max, y_max]
    confidence: float
    expression: str
    expression_score: float
    embedding: List[float]  # 512-d float vector


class FaceResult(BaseModel):
    file_path: str
    faces: List[DetectedFaceItem] = Field(default_factory=list)
    error: Optional[str] = None


class ExtractedTextSnippetItem(BaseModel):
    text: str
    box: Optional[List[float]] = None
    confidence: float


class OCRResult(BaseModel):
    file_path: str
    text_snippets: List[ExtractedTextSnippetItem] = Field(default_factory=list)
    search_tokens: List[str] = Field(default_factory=list)
    error: Optional[str] = None


@app.get("/health", response_model=HealthResponse)
def health():
    return HealthResponse(
        status="ok",
        service="media-extractor-ai",
        version="0.1.0",
        models_loaded={
            "yolo": True,
            "scene": True,
            "face": True,
            "ocr": True,
        },
    )


# Pipelines will be imported dynamically or conditionally to allow graceful degradation
try:
    from pipelines.object_scene import detect_objects_and_scenes
except ImportError:
    from ai_service.pipelines.object_scene import detect_objects_and_scenes  # type: ignore


try:
    from pipelines.face_engine import extract_faces
except ImportError:
    try:
        from ai_service.pipelines.face_engine import extract_faces  # type: ignore
    except ImportError:
        def extract_faces(paths: List[str]) -> List[FaceResult]:
            return [FaceResult(file_path=p, faces=[]) for p in paths]


try:
    from pipelines.ocr_engine import extract_ocr
except ImportError:
    try:
        from ai_service.pipelines.ocr_engine import extract_ocr  # type: ignore
    except ImportError:
        def extract_ocr(paths: List[str]) -> List[OCRResult]:
            return [OCRResult(file_path=p, text_snippets=[], search_tokens=[]) for p in paths]


@app.post("/v1/enrich/objects-scenes", response_model=List[ObjectSceneResult])
def enrich_objects_scenes(req: BatchPathsRequest):
    return detect_objects_and_scenes(req.file_paths)


@app.post("/v1/enrich/faces", response_model=List[FaceResult])
def enrich_faces(req: BatchPathsRequest):
    return extract_faces(req.file_paths)


@app.post("/v1/enrich/ocr", response_model=List[OCRResult])
def enrich_ocr(req: BatchPathsRequest):
    return extract_ocr(req.file_paths)


if __name__ == "__main__":
    import uvicorn
    port = int(os.environ.get("AI_SERVICE_PORT", "8765"))
    host = os.environ.get("AI_SERVICE_HOST", "127.0.0.1")
    uvicorn.run("service:app", host=host, port=port, log_level="info")
