"""
Immich REST API Client.
Provides high-throughput search, person tagging retrieval, local filesystem resolution,
asset upload, and album creation.
"""

import os
from pathlib import Path
from datetime import datetime
from typing import Dict, List, Optional, Any
import requests


class ImmichClient:
    """Client for interacting with local Immich instance."""

    def __init__(
        self,
        base_url: Optional[str] = None,
        api_key: Optional[str] = None,
        local_library_path: Optional[str] = None,
        timeout: int = 15,
    ):
        # Auto-load .env from project root if not already set
        if not os.getenv("IMMICH_API_KEY"):
            for env_candidate in [Path(".env"), Path("../.env"), Path(__file__).resolve().parent.parent.parent / ".env"]:
                if env_candidate.exists():
                    try:
                        with open(env_candidate, "r") as f:
                            for line in f:
                                line = line.strip()
                                if line and not line.startswith("#") and "=" in line:
                                    k, v = line.split("=", 1)
                                    os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))
                        if os.getenv("IMMICH_API_KEY"):
                            break
                    except Exception:
                        pass

        self.base_url = (base_url or os.getenv("IMMICH_INSTANCE_URL", "http://localhost:2283")).rstrip("/")
        self.api_key = api_key or os.getenv("IMMICH_API_KEY", "")
        self.local_library_path = Path(
            local_library_path
            or os.getenv("IMMICH_LOCAL_LIBRARY", "/Users/spsenthil/Documents/spsa/immich-app/library")
        )
        self.timeout = timeout
        self.session = requests.Session()
        if self.api_key:
            self.session.headers.update({
                "x-api-key": self.api_key,
                "Accept": "application/json",
            })

    def is_configured(self) -> bool:
        """Returns True if API key is provided."""
        return bool(self.api_key and self.api_key != "your_api_key_here")

    def test_connection(self) -> Dict[str, Any]:
        """Verify connectivity to Immich server and check API key auth."""
        res = self.session.get(f"{self.base_url}/api/server/version", timeout=self.timeout)
        res.raise_for_status()
        version_data = res.json()

        auth_status = False
        user_info = None
        if self.is_configured():
            try:
                me_res = self.session.get(f"{self.base_url}/api/users/me", timeout=self.timeout)
                if me_res.status_code == 200:
                    auth_status = True
                    user_info = me_res.json()
            except Exception:
                auth_status = False

        return {
            "status": "connected",
            "base_url": self.base_url,
            "version": version_data,
            "authenticated": auth_status,
            "user": user_info.get("name") or user_info.get("email") if user_info else None,
            "local_storage_exists": self.local_library_path.exists(),
        }

    def list_people(self, with_hidden: bool = False) -> List[Dict[str, Any]]:
        """List all recognized people in Immich with their names and IDs."""
        params = {"withHidden": "true" if with_hidden else "false"}
        # Immich v3 uses /api/people; earlier versions use /api/person
        try:
            res = self.session.get(f"{self.base_url}/api/people", params=params, timeout=self.timeout)
            if res.status_code == 200:
                data = res.json()
                return data.get("people", data) if isinstance(data, dict) else data
        except Exception:
            pass

        res = self.session.get(f"{self.base_url}/api/person", params=params, timeout=self.timeout)
        res.raise_for_status()
        data = res.json()
        return data.get("people", data) if isinstance(data, dict) else data

    def get_person_assets(self, person_id: str) -> List[Dict[str, Any]]:
        """Retrieve all assets containing a specific person."""
        try:
            res = self.session.get(f"{self.base_url}/api/person/{person_id}/assets", timeout=self.timeout)
            if res.status_code == 200:
                return res.json()
        except Exception:
            pass

        # Fallback to search metadata
        return self.search_metadata(person_ids=[person_id])

    def search_smart(self, query: str, clip_threshold: Optional[float] = None) -> List[Dict[str, Any]]:
        """
        Semantic natural language search using Immich's CLIP embeddings.
        e.g. query='birthday cake', 'beach sunset', 'smiling children'.
        """
        payload: Dict[str, Any] = {"query": query}
        if clip_threshold is not None:
            payload["clipThreshold"] = clip_threshold

        res = self.session.post(f"{self.base_url}/api/search/smart", json=payload, timeout=self.timeout)
        res.raise_for_status()
        data = res.json()
        assets = data.get("assets", {}).get("items", []) if isinstance(data, dict) else data
        return assets

    def search_metadata(
        self,
        person_ids: Optional[List[str]] = None,
        taken_after: Optional[str] = None,
        taken_before: Optional[str] = None,
        city: Optional[str] = None,
        country: Optional[str] = None,
        is_favorite: Optional[bool] = None,
        media_type: Optional[str] = None,  # "IMAGE" or "VIDEO"
    ) -> List[Dict[str, Any]]:
        """Query assets using structured EXIF, time, and identity filters."""
        payload: Dict[str, Any] = {}
        if person_ids:
            payload["personIds"] = person_ids
        if taken_after:
            payload["takenAfter"] = taken_after
        if taken_before:
            payload["takenBefore"] = taken_before
        if city:
            payload["city"] = city
        if country:
            payload["country"] = country
        if is_favorite is not None:
            payload["isFavorite"] = is_favorite
        if media_type:
            payload["type"] = media_type

        res = self.session.post(f"{self.base_url}/api/search/metadata", json=payload, timeout=self.timeout)
        res.raise_for_status()
        data = res.json()
        assets = data.get("assets", {}).get("items", []) if isinstance(data, dict) else data
        return assets

    def get_asset_info(self, asset_id: str) -> Dict[str, Any]:
        """Fetch detailed metadata for a single asset."""
        try:
            res = self.session.get(f"{self.base_url}/api/assets/{asset_id}", timeout=self.timeout)
            if res.status_code == 200:
                return res.json()
        except Exception:
            pass

        res = self.session.get(f"{self.base_url}/api/asset/asset-info/{asset_id}", timeout=self.timeout)
        res.raise_for_status()
        return res.json()

    def resolve_local_path(self, asset: Dict[str, Any]) -> Optional[Path]:
        """
        Resolves the local host filesystem path for an Immich asset.
        Immich docker uses `/data/...`, which maps to `local_library_path` on host.
        """
        original_path = asset.get("originalPath")
        if not original_path:
            return None

        # Convert /data/... container path to host local path
        if original_path.startswith("/data/"):
            rel_path = original_path[len("/data/"):]
            candidate = self.local_library_path / rel_path
            if candidate.exists():
                return candidate

        # If already absolute host path
        host_candidate = Path(original_path)
        if host_candidate.exists():
            return host_candidate

        return None

    def upload_asset(
        self,
        file_path: Path,
        created_at: Optional[datetime] = None,
        device_id: str = "ImmichAIStudio",
    ) -> Dict[str, Any]:
        """
        Uploads a video or image file to Immich via multipart POST /api/assets.
        """
        if not file_path.exists():
            raise FileNotFoundError(f"File to upload does not exist: {file_path}")

        if created_at:
            now_str = created_at.strftime("%Y-%m-%dT%H:%M:%SZ")
        else:
            now_str = datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ")

        data = {
            "deviceAssetId": f"reel-{file_path.stem}",
            "deviceId": device_id,
            "fileCreatedAt": now_str,
            "fileModifiedAt": now_str,
            "isFavorite": "false",
        }

        ext = file_path.suffix.lower()
        mime_type = "video/mp4" if ext in [".mp4", ".mov", ".m4v"] else "image/jpeg"

        with open(file_path, "rb") as f:
            files = {
                "assetData": (file_path.name, f, mime_type)
            }
            res = self.session.post(f"{self.base_url}/api/assets", data=data, files=files, timeout=self.timeout * 4)
            if res.status_code not in [200, 201]:
                f.seek(0)
                res = self.session.post(f"{self.base_url}/api/asset/upload", data=data, files=files, timeout=self.timeout * 4)

            res.raise_for_status()
            return res.json()

    def create_album(
        self,
        album_name: str,
        description: Optional[str] = None,
        asset_ids: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        """Creates a new album in Immich with optional initial assets."""
        payload: Dict[str, Any] = {"albumName": album_name}
        if description:
            payload["description"] = description
        if asset_ids:
            payload["assetIds"] = asset_ids

        res = self.session.post(f"{self.base_url}/api/albums", json=payload, timeout=self.timeout)
        if res.status_code not in [200, 201]:
            res = self.session.post(f"{self.base_url}/api/album", json=payload, timeout=self.timeout)

        res.raise_for_status()
        return res.json()
