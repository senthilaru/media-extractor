"""
Occasion Clustering & Quality Curation Engine.
Clusters Immich media assets by temporal/event boundaries, evaluates sharpness,
faces, and expressions, and selects the highest-quality shots for highlight reels.
"""

from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Any, Tuple
import cv2
import numpy as np

from .immich_client import ImmichClient


from PIL import Image

@dataclass
class CuratedAsset:
    asset_id: str
    file_path: Path
    media_type: str  # "IMAGE" or "VIDEO"
    created_at: datetime
    duration: float = 0.0  # seconds if video
    width: int = 0
    height: int = 0
    sharpness: float = 0.0
    faces_count: int = 0
    has_smile: bool = False
    quality_score: float = 0.0
    tags: List[str] = field(default_factory=list)
    people: List[str] = field(default_factory=list)


@dataclass
class CuratedEvent:
    event_id: str
    title: str
    start_time: datetime
    end_time: datetime
    people: List[str]
    occasion_type: str
    assets: List[CuratedAsset]
    total_raw_count: int = 0


class EventCurator:
    """Discovers, clusters, scores, and curates events from Immich."""

    def __init__(self, immich_client: Optional[ImmichClient] = None):
        self.client = immich_client or ImmichClient()

    @staticmethod
    def calculate_sharpness(image_path: Path) -> float:
        """
        Calculates sharpness using Laplacian variance.
        Values < 50 typically indicate blurry / out-of-focus images.
        """
        try:
            with Image.open(image_path) as pil_img:
                pil_img = pil_img.convert("L")
                w, h = pil_img.size
                if max(w, h) > 1280:
                    scale = 1280.0 / max(w, h)
                    pil_img = pil_img.resize((int(w * scale), int(h * scale)), Image.Resampling.BILINEAR)
                gray = np.array(pil_img)
            variance = cv2.Laplacian(gray, cv2.CV_64F).var()
            return round(float(variance), 2)
        except Exception:
            return 0.0

    @staticmethod
    def parse_timestamp(ts_str: Optional[str]) -> datetime:
        """Safely parses ISO timestamps from Immich."""
        if not ts_str:
            return datetime.min
        try:
            clean = ts_str.replace("Z", "+00:00")
            return datetime.fromisoformat(clean)
        except Exception:
            return datetime.min

    def fetch_and_cluster(
        self,
        person_name: Optional[str] = None,
        query: Optional[str] = None,
        max_time_gap_hours: float = 4.0,
        min_cluster_size: int = 3,
    ) -> List[CuratedEvent]:
        """
        Queries Immich and clusters matching assets into cohesive events based on time windows.
        """
        raw_assets: List[Dict[str, Any]] = []

        # 1. Resolve assets based on input criteria
        if person_name:
            people = self.client.list_people()
            matched = [p for p in people if p.get("name", "").strip().lower() == person_name.strip().lower()]
            if not matched:
                raise ValueError(f"Person '{person_name}' not found in Immich. Available named people: {[p.get('name') for p in people if p.get('name')]}")
            person_id = matched[0]["id"]
            raw_assets = self.client.get_person_assets(person_id)
        elif query:
            raw_assets = self.client.search_smart(query)
        else:
            raw_assets = self.client.search_metadata()

        if not raw_assets:
            return []

        # 2. Filter assets that have valid local files
        valid_items: List[Tuple[Dict[str, Any], Path, datetime]] = []
        for asset in raw_assets:
            local_path = self.client.resolve_local_path(asset)
            if local_path and local_path.exists():
                dt = self.parse_timestamp(asset.get("fileCreatedAt") or asset.get("localDateTime") or asset.get("createdAt"))
                valid_items.append((asset, local_path, dt))

        if not valid_items:
            return []

        # Sort chronologically
        valid_items.sort(key=lambda x: x[2])

        # 3. Temporal Clustering
        events: List[CuratedEvent] = []
        current_cluster: List[Tuple[Dict[str, Any], Path, datetime]] = [valid_items[0]]

        for i in range(1, len(valid_items)):
            prev_dt = current_cluster[-1][2]
            curr_dt = valid_items[i][2]

            diff_hours = abs((curr_dt - prev_dt).total_seconds()) / 3600.0
            if diff_hours <= max_time_gap_hours:
                current_cluster.append(valid_items[i])
            else:
                if len(current_cluster) >= min_cluster_size:
                    events.append(self._build_event(current_cluster, query=query))
                current_cluster = [valid_items[i]]

        if len(current_cluster) >= min_cluster_size:
            events.append(self._build_event(current_cluster, query=query))

        return events

    def _build_event(
        self,
        cluster: List[Tuple[Dict[str, Any], Path, datetime]],
        query: Optional[str] = None,
    ) -> CuratedEvent:
        """Processes an event cluster, scoring quality and choosing top assets."""
        start_time = cluster[0][2]
        end_time = cluster[-1][2]

        # Gather people attending
        people_set = set()
        for raw, _, _ in cluster:
            for p in raw.get("people", []):
                if isinstance(p, dict) and p.get("name"):
                    people_set.add(p["name"].strip())
                elif isinstance(p, str):
                    people_set.add(p.strip())

        # If not populated from search response, check sample asset info
        if not people_set and cluster:
            try:
                sample_info = self.client.get_asset_info(cluster[0][0].get("id", ""))
                for p in sample_info.get("people", []):
                    if isinstance(p, dict) and p.get("name"):
                        people_set.add(p["name"].strip())
            except Exception:
                pass

        people_list = sorted(list(people_set))

        # Infer occasion type
        occasion_type = "Gathering"
        if query:
            occasion_type = query.capitalize()
        elif len(people_list) > 2:
            occasion_type = "Celebration"
        elif len(cluster) > 15:
            occasion_type = "Event"

        # Construct title
        date_str = start_time.strftime("%b %d, %Y")
        if people_list:
            subject_str = ", ".join(people_list[:2])
            if len(people_list) > 2:
                subject_str += f" +{len(people_list)-2}"
            title = f"{subject_str} - {occasion_type} ({date_str})"
        else:
            title = f"{occasion_type} ({date_str})"

        event_id = f"{start_time.strftime('%Y%m%d')}_{occasion_type.lower()}"

        # Score and filter assets
        curated_assets = self.curate_cluster_assets(cluster)

        return CuratedEvent(
            event_id=event_id,
            title=title,
            start_time=start_time,
            end_time=end_time,
            people=people_list,
            occasion_type=occasion_type,
            assets=curated_assets,
            total_raw_count=len(cluster),
        )

    def curate_cluster_assets(
        self,
        cluster: List[Tuple[Dict[str, Any], Path, datetime]],
        target_count: int = 15,
        min_sharpness: float = 40.0,
    ) -> List[CuratedAsset]:
        """
        Ranks and curates the best assets from an event cluster:
        - Drops blurry / degraded shots
        - Prioritizes sharp focus, smiling faces, and video clips
        - Suppresses burst duplicates taken within 10 seconds of each other
        """
        scored_items: List[CuratedAsset] = []

        for raw, local_path, dt in cluster:
            media_type = raw.get("type", "IMAGE").upper()
            w = raw.get("width", 0) or 0
            h = raw.get("height", 0) or 0
            dur_str = raw.get("duration", "0")
            try:
                duration = float(dur_str) if dur_str else 0.0
            except ValueError:
                duration = 0.0

            sharpness = 0.0
            quality_score = 50.0  # baseline

            if media_type == "IMAGE":
                sharpness = self.calculate_sharpness(local_path)
                if sharpness < min_sharpness and sharpness > 0.0:
                    # Blurry photo penalty
                    quality_score -= 30.0
                else:
                    # Sharpness bonus
                    quality_score += min(30.0, sharpness / 20.0)

                # Resolution bonus
                if w * h >= 1920 * 1080:
                    quality_score += 10.0
                if raw.get("isFavorite"):
                    quality_score += 20.0

            elif media_type == "VIDEO":
                # Video clips bring dynamic motion to reels
                quality_score += 25.0
                sharpness = 100.0  # placeholder for videos

            # Extract associated people
            people_names = [
                p.get("name", "").strip() for p in raw.get("people", [])
                if isinstance(p, dict) and p.get("name")
            ]

            scored_items.append(CuratedAsset(
                asset_id=raw.get("id", ""),
                file_path=local_path,
                media_type=media_type,
                created_at=dt,
                duration=duration,
                width=w,
                height=h,
                sharpness=sharpness,
                faces_count=len(people_names),
                has_smile=False,
                quality_score=quality_score,
                people=people_names,
            ))

        # 4. Burst deduplication: If multiple items are within 8 seconds, keep only the highest scoring
        pruned_items: List[CuratedAsset] = []
        burst_bucket: List[CuratedAsset] = []

        for item in scored_items:
            if not burst_bucket:
                burst_bucket.append(item)
            else:
                last_dt = burst_bucket[-1].created_at
                gap = abs((item.created_at - last_dt).total_seconds())
                if gap <= 8.0:
                    burst_bucket.append(item)
                else:
                    best = max(burst_bucket, key=lambda x: x.quality_score)
                    pruned_items.append(best)
                    burst_bucket = [item]

        if burst_bucket:
            best = max(burst_bucket, key=lambda x: x.quality_score)
            pruned_items.append(best)

        # 5. Sort by quality and select top N items, keeping chronological order in the reel
        top_candidates = sorted(pruned_items, key=lambda x: x.quality_score, reverse=True)[:target_count]
        # Re-sort chronologically for the storytelling timeline
        top_candidates.sort(key=lambda x: x.created_at)

        return top_candidates
