# Immich AI Creative Studio & Memory Reel Generator

A companion studio built into `media-extractor` that connects to your local Immich server, discovers and clusters family events, curates top-quality photos, applies cinematic Ken Burns motion, and publishes highlight memory videos and albums directly back into Immich.

---

## Architecture Overview

```text
┌────────────────────────────────────────────────────────┐
│                   LOCAL IMMICH SERVER                  │
│ • Holds photos/videos synced from phones & cameras     │
│ • Tagged people (e.g., Akshara, Krishnaveni)           │
│ • CLIP semantic vector search & EXIF metadata          │
└───────────────────────────┬────────────────────────────┘
                            │
               1. Query Assets via REST API
                            │
┌───────────────────────────▼────────────────────────────┐
│                  EVENT CURATION ENGINE                 │
│ • Groups photos into real events by temporal window    │
│ • Auto-detects attending people from Immich tags       │
│ • Eliminates burst duplicates and blurry shots         │
└───────────────────────────┬────────────────────────────┘
                            │
               2. Top Curated Assets
                            │
┌───────────────────────────▼────────────────────────────┐
│            CINEMATIC REEL GENERATOR (FFmpeg)           │
│ • Smooth Ken Burns pan & zoom with ease-in-out         │
│ • Elegant blurred background for portrait shots        │
│ • Cross-dissolve transitions between photos            │
│ • Opening title card with date and people names        │
└───────────────────────────┬────────────────────────────┘
                            │
               3. Publish to Immich Loopback
                            │
┌───────────────────────────▼────────────────────────────┐
│  • Uploads generated MP4 highlight reel as new asset   │
│  • Creates an Immich Album: "Event Title (Highlights)" │
│  • Directly viewable in Immich iOS/Android/Web app!    │
└────────────────────────────────────────────────────────┘
```

---

## Quick Usage

From the project root:

```bash
# Set PYTHONPATH to ai-service and run with virtualenv python:
export PYTHONPATH="ai-service"
PYTHON="ai-service/.venv/bin/python3"
```

### 1. Auto-Select Tamil Music Based on Scene & Sentiments
Automatically infers the event mood (Father-Daughter, Mother-Child, Couples Romance, Party, Nostalgia), downloads the iconic song hook, applies smooth audio fades, and publishes to Immich:

```bash
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --event-idx 1 --publish
```

### 2. Pick a Specific Tamil Sentimental Theme
Available themes: `father_daughter`, `mother_sentiments`, `couples_romance`, `celebration_party`, `family_nostalgia`, `birthday_celebration`:

```bash
# Father-Daughter sentiment ("Aanandha Yaazhai", "Kannaana Kanney", "Vaa Vaa En Devadhaye")
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --theme father_daughter --publish

# Mother sentiment ("Amma Amma", "Aarariraro", "Chinna Chinna Aasai")
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --theme mother_sentiments --publish

# Couples romance ("Munbe Vaa", "Megham Karukatha", "Maruvaarthai", "Neeyum Naanum Anbe")
$PYTHON ai-service/studio/generate_reel.py --query "wedding" --theme couples_romance --publish

# High energy celebration / friends ("Vaathi Coming", "Naa Ready", "Jalabulanjangu", "Arabic Kuthu")
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --theme celebration_party --publish
```

### 3. Attach Any Specific Tamil Song by Name
Specify any song name or movie directly:

```bash
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --song "Aanandha Yaazhai" --publish
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --song "Munbe Vaa" --publish
```

### 4. Create Vertical Reels for Mobile / Instagram / Stories
Render in 9:16 vertical orientation (1080x1920):

```bash
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --orientation portrait --publish
```

### 5. Local-Only Render (Do Not Upload to Immich)
If you only want the local MP4 file saved in `output_reels/`:

```bash
$PYTHON ai-service/studio/generate_reel.py --person "Akshara" --no-publish
```

---

## Python API Reference

```python
from studio.immich_client import ImmichClient
from studio.curator import EventCurator
from studio.reel_generator import HighlightReelGenerator

# 1. Initialize Client & Curator
client = ImmichClient()
curator = EventCurator(client)

# 2. Discover Events
events = curator.fetch_and_cluster(person_name="Akshara")
chosen_event = events[0]

# 3. Render Reel
generator = HighlightReelGenerator(target_size=(1920, 1080), fps=30)
out_mp4 = generator.render_event_reel(chosen_event, show_title=True)

# 4. Upload & Create Immich Album
reel_asset = client.upload_asset(out_mp4, created_at=chosen_event.start_time)
curated_ids = [a.asset_id for a in chosen_event.assets]
album = client.create_album(
    album_name=f"{chosen_event.title} (Highlights)",
    asset_ids=[reel_asset["id"]] + curated_ids
)
print("Album link:", f"{client.base_url}/albums/{album['id']}")
```
