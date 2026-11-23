"""
Media Extractor AI Sidecar Service
Provides high-throughput visual semantic enrichment (objects, scenes, faces, OCR).
Strictly operates on local file paths in READ-ONLY mode.
Supports FastAPI when installed, with seamless fallback to Python standard library http.server.
"""

from typing import List, Optional, Dict, Any
import os
import sys
import json
from pathlib import Path

# Add current directory to sys.path so pipelines can be imported cleanly
current_dir = Path(__file__).resolve().parent
if str(current_dir) not in sys.path:
    sys.path.insert(0, str(current_dir))

# Import pipelines
try:
    from pipelines.object_scene import detect_objects_and_scenes
except Exception as e:
    import traceback
    print(f"[AI Service] Warning importing object_scene: {e}", file=sys.stderr)
    traceback.print_exc()
    def detect_objects_and_scenes(paths: List[str]) -> List[Dict[str, Any]]:
        return [{"file_path": p, "objects": [], "occasions": [], "error": None} for p in paths]

try:
    from pipelines.face_engine import extract_faces
except Exception as e:
    import traceback
    print(f"[AI Service] Warning importing face_engine: {e}", file=sys.stderr)
    traceback.print_exc()
    def extract_faces(paths: List[str]) -> List[Dict[str, Any]]:
        return [{"file_path": p, "faces": [], "error": None} for p in paths]

try:
    from pipelines.ocr_engine import extract_ocr
except Exception as e:
    import traceback
    print(f"[AI Service] Warning importing ocr_engine: {e}", file=sys.stderr)
    traceback.print_exc()
    def extract_ocr(paths: List[str]) -> List[Dict[str, Any]]:
        return [{"file_path": p, "text_snippets": [], "search_tokens": [], "error": None} for p in paths]


# Check if FastAPI and Uvicorn are available
HAS_FASTAPI = False
try:
    from fastapi import FastAPI
    from pydantic import BaseModel, Field
    import uvicorn
    HAS_FASTAPI = True
except ImportError:
    HAS_FASTAPI = False


if HAS_FASTAPI:
    app = FastAPI(
        title="Media Extractor AI Service",
        description="Local computer vision sidecar for semantic object, face, and OCR extraction",
        version="0.1.0",
    )

    class BatchPathsRequest(BaseModel):
        file_paths: List[str]

    @app.get("/health")
    def health():
        try:
            from pipelines.object_scene import HAS_YOLO
        except Exception:
            HAS_YOLO = False
        try:
            from pipelines.face_engine import HAS_INSIGHTFACE
        except Exception:
            HAS_INSIGHTFACE = False
        try:
            from pipelines.ocr_engine import HAS_VISION
        except Exception:
            HAS_VISION = False

        return {
            "status": "ok",
            "service": "media-extractor-ai",
            "has_yolo": HAS_YOLO,
            "has_faces": HAS_INSIGHTFACE,
            "has_ocr": HAS_VISION,
            "version": "0.1.0",
        }

    @app.post("/v1/enrich/objects-scenes")
    def enrich_objects_scenes(req: BatchPathsRequest):
        return detect_objects_and_scenes(req.file_paths)

    @app.post("/v1/enrich/faces")
    def enrich_faces(req: BatchPathsRequest):
        return extract_faces(req.file_paths)

    @app.post("/v1/enrich/ocr")
    def enrich_ocr(req: BatchPathsRequest):
        return extract_ocr(req.file_paths)

    def run_server(host: str, port: int):
        uvicorn.run(app, host=host, port=port, log_level="info", timeout_keep_alive=120)

else:
    # Standard library fallback server
    from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler

    class FallbackHandler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def _send_json(self, status_code: int, data: Any):
            payload = json.dumps(data).encode("utf-8")
            self.send_response(status_code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.send_header("Connection", "keep-alive")
            self.end_headers()
            self.wfile.write(payload)

        def do_GET(self):
            if self.path == "/health":
                self._send_json(200, {
                    "status": "ok",
                    "service": "media-extractor-ai",
                    "has_yolo": False,
                    "has_faces": False,
                    "has_ocr": False,
                    "version": "0.1.0 (standard-library fallback)",
                })
            else:
                self._send_json(404, {"error": "Not found"})

        def do_POST(self):
            try:
                content_length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(content_length).decode("utf-8") if content_length > 0 else "{}"
                data = json.loads(body) if body else {}
                paths = data.get("file_paths", [])

                if self.path == "/v1/enrich/objects-scenes":
                    results = detect_objects_and_scenes(paths)
                    self._send_json(200, results)
                elif self.path == "/v1/enrich/faces":
                    results = extract_faces(paths)
                    self._send_json(200, results)
                elif self.path == "/v1/enrich/ocr":
                    results = extract_ocr(paths)
                    self._send_json(200, results)
                else:
                    self._send_json(404, {"error": "Endpoint not found"})
            except Exception as e:
                import traceback
                print(f"[AI Service] Error in POST {self.path}: {e}", file=sys.stderr)
                traceback.print_exc()
                try:
                    self._send_json(500, {"error": str(e)})
                except Exception:
                    pass

        def log_message(self, format, *args):
            pass  # Keep stderr clean

    def run_server(host: str, port: int):
        server = ThreadingHTTPServer((host, port), FallbackHandler)
        print(f"[AI Service] Started on http://{host}:{port} (standard library fallback mode)")
        server.serve_forever()


if __name__ == "__main__":
    port = int(os.environ.get("AI_SERVICE_PORT", "8765"))
    host = os.environ.get("AI_SERVICE_HOST", "127.0.0.1")
    run_server(host, port)
