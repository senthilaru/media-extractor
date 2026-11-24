"""
Immich Creative Studio package.
Provides Immich API client, event curation, highlight reel generation, and Tamil music recommendation.
"""

from .immich_client import ImmichClient
from .curator import EventCurator, CuratedEvent, CuratedAsset
from .reel_generator import HighlightReelGenerator
from .tamil_music import TamilMusicManager, TAMIL_THEMES

__all__ = [
    "ImmichClient",
    "EventCurator",
    "CuratedEvent",
    "CuratedAsset",
    "HighlightReelGenerator",
    "TamilMusicManager",
    "TAMIL_THEMES",
]
