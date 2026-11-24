"""
Tamil Music & Sentimental Audio Recommendation & Downloader Engine.
Provides curated Tamil song hooks, sentiment-based matching (father-daughter,
mother-child, couples romance, celebration party, nostalgia), and automated
section trimming with audio fades.
"""

from pathlib import Path
from typing import Dict, List, Optional, Any, Tuple
import os
import subprocess
import json
import random

from .curator import CuratedEvent


TAMIL_THEMES: Dict[str, Dict[str, Any]] = {
    "father_daughter": {
        "description": "Father & Daughter / Son bonding, emotional lines",
        "tracks": [
            {
                "title": "Aanandha Yaazhai (Thangameenkal)",
                "artist": "Yuvan Shankar Raja",
                "query": "Aanandha Yaazhai audio song Thangameenkal",
                "start_time": 45,
                "tagline": "Aanandha yaazhai meettukinraai...",
            },
            {
                "title": "Kannaana Kanney (Viswasam)",
                "artist": "D. Imman / Sid Sriram",
                "query": "Kannaana Kanney lyrical Viswasam Sid Sriram",
                "start_time": 65,
                "tagline": "Kannaana kanney en meedhu saaindhaai...",
            },
            {
                "title": "Vaa Vaa En Devadhaye (Abhiyum Naanum)",
                "artist": "Vidyasagar / Madhu Balakrishnan",
                "query": "Vaa Vaa En Devadhaye audio Abhiyum Naanum",
                "start_time": 50,
                "tagline": "Vaa vaa en devadhaye...",
            },
            {
                "title": "Dheivangal Ellaam (Kedi Billa Killadi Ranga)",
                "artist": "Yuvan Shankar Raja",
                "query": "Dheivangal Ellaam Thotraval Diva Kedi Billa Killadi Ranga",
                "start_time": 40,
                "tagline": "Dheivangal ellaam thotraval diva...",
            },
        ],
    },
    "mother_sentiments": {
        "description": "Mother-Son / Mother-Daughter love and sentiment",
        "tracks": [
            {
                "title": "Amma Amma (VIP)",
                "artist": "Anirudh Ravichander / Dhanush",
                "query": "Amma Amma audio VIP Dhanush Anirudh",
                "start_time": 55,
                "tagline": "Amma amma nee enga ammaa...",
            },
            {
                "title": "Aarariraro (Raam)",
                "artist": "Yuvan Shankar Raja / KJ Yesudas",
                "query": "Aarariraro lyrical Raam Yesudas Yuvan",
                "start_time": 45,
                "tagline": "Aarariraro paadida vaa ammaa...",
            },
            {
                "title": "Chinna Chinna Aasai (Roja)",
                "artist": "A.R. Rahman / Minmini",
                "query": "Chinna Chinna Aasai audio Roja AR Rahman",
                "start_time": 30,
                "tagline": "Chinna chinna aasai siragadikum aasai...",
            },
        ],
    },
    "couples_romance": {
        "description": "Romantic melodies & touching lines for couples",
        "tracks": [
            {
                "title": "Munbe Vaa (Sillunu Oru Kaadhal)",
                "artist": "A.R. Rahman / Shreya Ghoshal, Naresh Iyer",
                "query": "Munbe Vaa audio Sillunu Oru Kaadhal AR Rahman",
                "start_time": 70,
                "tagline": "Munbe vaa en anbe vaa...",
            },
            {
                "title": "Megham Karukatha (Thiruchitrambalam)",
                "artist": "Anirudh Ravichander / Dhanush",
                "query": "Megham Karukatha audio Thiruchitrambalam Anirudh",
                "start_time": 35,
                "tagline": "Megham karukatha penne penne...",
            },
            {
                "title": "Maruvaarthai (ENPT)",
                "artist": "Darbuka Siva / Sid Sriram",
                "query": "Maruvaarthai lyrical ENPT Sid Sriram",
                "start_time": 50,
                "tagline": "Maruvaarthai pesaathe madi meedhu nee thoongidu...",
            },
            {
                "title": "Neeyum Naanum Anbe (Imaikkaa Nodigal)",
                "artist": "Hiphop Tamizha / Raghu Dixit",
                "query": "Neeyum Naanum Anbe lyrical Imaikkaa Nodigal",
                "start_time": 40,
                "tagline": "Neeyum naanum anbe kangal korthukondu...",
            },
            {
                "title": "Unakku Thaan (Chithha)",
                "artist": "Santhosh Narayanan / Dhee",
                "query": "Unakku Thaan lyrical Chithha Santhosh Narayanan",
                "start_time": 35,
                "tagline": "Unakku thaan uyir vaazhgiren...",
            },
        ],
    },
    "celebration_party": {
        "description": "Joyful celebrations, friends party, energetic kuthu",
        "tracks": [
            {
                "title": "Vaathi Coming (Master)",
                "artist": "Anirudh Ravichander",
                "query": "Vaathi Coming audio Master Anirudh",
                "start_time": 30,
                "tagline": "Vaathi coming otthe...",
            },
            {
                "title": "Naa Ready (Leo)",
                "artist": "Anirudh Ravichander / Thalapathy Vijay",
                "query": "Naa Ready lyrical Leo Anirudh Vijay",
                "start_time": 45,
                "tagline": "Naa ready thaan varavaa...",
            },
            {
                "title": "Jalabulanjangu (Don)",
                "artist": "Anirudh Ravichander",
                "query": "Jalabulanjangu audio Don Anirudh",
                "start_time": 35,
                "tagline": "Jalabulanjangu kootam serungada...",
            },
            {
                "title": "Arabic Kuthu - Halamithi Habibo (Beast)",
                "artist": "Anirudh Ravichander / Jonita Gandhi",
                "query": "Arabic Kuthu audio Beast Anirudh",
                "start_time": 40,
                "tagline": "Halamithi habibo...",
            },
        ],
    },
    "family_nostalgia": {
        "description": "Heartwarming family moments, Ilaiyaraaja & ARR melodies",
        "tracks": [
            {
                "title": "Idhayathai Thirudathe Flute BGM",
                "artist": "Ilaiyaraaja",
                "query": "Idhayathai Thirudathe BGM Ilaiyaraaja flute original",
                "start_time": 15,
                "tagline": "Soulful Ilaiyaraaja flute melody",
            },
            {
                "title": "Bombay Theme",
                "artist": "A.R. Rahman",
                "query": "Bombay Theme AR Rahman official audio",
                "start_time": 20,
                "tagline": "Epic emotional ARR theme",
            },
            {
                "title": "Kandukondain Kandukondain Title Track",
                "artist": "A.R. Rahman / Hariharan",
                "query": "Kandukondain Kandukondain audio Hariharan",
                "start_time": 45,
                "tagline": "Kandukondain kandukondain kaadhal mugam...",
            },
        ],
    },
    "birthday_celebration": {
        "description": "Happy Birthday celebrations & joyful beats",
        "tracks": [
            {
                "title": "Iniya Pirandha Naal Vaazhthukkal",
                "artist": "Tamil Birthday Melody",
                "query": "Happy Birthday Tamil upbeat song",
                "start_time": 15,
                "tagline": "Iniya pirandha naal vaazhthukkal...",
            },
            {
                "title": "Adada Mazhaida (Paiyaa)",
                "artist": "Yuvan Shankar Raja",
                "query": "Adada Mazhaida audio Paiyaa Yuvan",
                "start_time": 35,
                "tagline": "Joyous celebration rhythm",
            },
        ],
    },
}


class TamilMusicManager:
    """Manages Tamil sentimental audio discovery, downloading, and reel synchronization."""

    def __init__(self, cache_dir: Optional[str] = None):
        self.cache_dir = Path(cache_dir or "audio_cache")
        self.cache_dir.mkdir(parents=True, exist_ok=True)

    def infer_theme(self, event: CuratedEvent) -> str:
        """
        Infers the most appropriate sentimental/thematic Tamil music category
        based on the event occasion, people, and scene context.
        """
        occ = (event.occasion_type or "").lower()
        title_lower = (event.title or "").lower()
        people_lower = [p.lower() for p in event.people]

        # 1. Birthday
        if "birthday" in occ or "birthday" in title_lower or "cake" in title_lower:
            return "birthday_celebration"

        # 2. Mother & Child Sentiment
        if any(m in people_lower for m in ["krishnaveni", "amma", "mom", "mother"]) and any(c in people_lower for c in ["akshara", "hasini", "ishaan"]):
            return "mother_sentiments"

        # 3. Father & Daughter / Son Sentiment
        if any(f in people_lower for f in ["senthil", "appa", "dad", "father"]) and any(c in people_lower for c in ["akshara", "hasini", "ishaan"]):
            return "father_daughter"

        # 4. Couples Romance (2 adults)
        if len(event.people) == 2 and any(p in ["senthil", "krishnaveni"] for p in people_lower):
            return "couples_romance"

        # 5. Party / Friends / Large Gathering
        if len(event.people) >= 3 or "celebration" in occ or "gathering" in occ or "party" in occ:
            return "celebration_party"

        # Default fallback
        return "family_nostalgia"

    def select_track(
        self,
        theme: Optional[str] = None,
        custom_song: Optional[str] = None,
        event: Optional[CuratedEvent] = None,
    ) -> Dict[str, Any]:
        """
        Selects a track definition from the catalog or constructs a custom track query.
        """
        if custom_song:
            return {
                "title": custom_song,
                "artist": "Selected Track",
                "query": f"{custom_song} tamil lyrical audio",
                "start_time": 30,
                "tagline": custom_song,
            }

        resolved_theme = theme
        if not resolved_theme and event:
            resolved_theme = self.infer_theme(event)

        if not resolved_theme or resolved_theme not in TAMIL_THEMES:
            resolved_theme = "celebration_party"

        theme_data = TAMIL_THEMES[resolved_theme]
        chosen = random.choice(theme_data["tracks"])
        chosen_copy = dict(chosen)
        chosen_copy["theme"] = resolved_theme
        chosen_copy["theme_description"] = theme_data["description"]
        return chosen_copy

    def prepare_audio(
        self,
        track_info: Dict[str, Any],
        target_duration: float,
        output_path: Optional[Path] = None,
    ) -> Path:
        """
        Downloads the song snippet using yt-dlp, trims to target_duration,
        and applies smooth audio fade-in & fade-out using ffmpeg.
        """
        safe_name = "".join(c if c.isalnum() else "_" for c in track_info["title"]).strip("_")
        cached_raw = self.cache_dir / f"{safe_name}_raw.mp3"

        # 1. Download snippet with yt-dlp if not already cached
        start_sec = track_info.get("start_time", 30)
        end_sec = start_sec + int(target_duration) + 15  # buffer

        start_str = f"{start_sec // 60:02d}:{start_sec % 60:02d}"
        end_str = f"{end_sec // 60:02d}:{end_sec % 60:02d}"

        if not cached_raw.exists():
            query = track_info.get("query", track_info["title"])
            print(f"[Tamil Music] Searching & downloading audio: '{query}' ({start_str} - {end_str})...")

            cmd = [
                "yt-dlp",
                "--no-update",
                "--download-sections", f"*{start_str}-{end_str}",
                "-x",
                "--audio-format", "mp3",
                "-o", str(cached_raw.with_suffix(".%(ext)s")),
                f"ytsearch1:{query}",
            ]

            proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if proc.returncode != 0:
                # If section download failed, download full audio hook
                print("[Tamil Music] Section download notice, attempting standard query...")
                cmd_fallback = [
                    "yt-dlp",
                    "--no-update",
                    "-x",
                    "--audio-format", "mp3",
                    "-o", str(cached_raw.with_suffix(".%(ext)s")),
                    f"ytsearch1:{query}",
                ]
                subprocess.run(cmd_fallback, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        if not cached_raw.exists():
            raise RuntimeError(f"Failed to fetch audio for '{track_info['title']}'")

        # 2. Trim exact duration and apply smooth fade-in and fade-out with FFmpeg
        final_audio = output_path or (self.cache_dir / f"{safe_name}_{int(target_duration)}s.mp3")

        fade_out_start = max(0.5, target_duration - 2.0)
        filter_str = f"afade=t=in:ss=0:d=1.5,afade=t=out:st={fade_out_start:.2f}:d=2.0"

        ffmpeg_cmd = [
            "ffmpeg",
            "-y",
            "-i", str(cached_raw),
            "-t", str(target_duration),
            "-af", filter_str,
            "-c:a", "libmp3lame",
            "-b:a", "192k",
            str(final_audio),
        ]

        subprocess.run(ffmpeg_cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        print(f"[Tamil Music] Prepared audio track: '{track_info['title']}' ({target_duration:.1f}s)")
        return final_audio
