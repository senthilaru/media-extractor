"""
Test EventCurator clustering and quality scoring on Immich library.
Usage:
    python ai-service/studio/test_curator.py [person_name_or_query]
"""

import os
import sys
from pathlib import Path

# Setup paths
current_dir = Path(__file__).resolve().parent
ai_service_dir = current_dir.parent
root_dir = ai_service_dir.parent

# Load .env
dotenv_path = root_dir / ".env"
if dotenv_path.exists():
    with open(dotenv_path, "r") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

if str(ai_service_dir) not in sys.path:
    sys.path.insert(0, str(ai_service_dir))

from studio.curator import EventCurator


def main():
    target = sys.argv[1] if len(sys.argv) > 1 else "Akshara"
    print("=" * 60)
    print(f" Event Curator Analysis: '{target}'")
    print("=" * 60)

    curator = EventCurator()
    print(f"Querying Immich and clustering events for: {target}...")

    # Try as person first, then as smart search query
    try:
        events = curator.fetch_and_cluster(person_name=target)
    except ValueError:
        print(f"'{target}' is not a named person, trying as smart visual query...")
        events = curator.fetch_and_cluster(query=target)

    print(f"\nDiscovered {len(events)} Event Clusters:\n")
    for idx, ev in enumerate(events, 1):
        print(f"[{idx}] {ev.title}")
        print(f"    Date Range:     {ev.start_time.strftime('%Y-%m-%d %H:%M')} -> {ev.end_time.strftime('%H:%M')}")
        print(f"    Raw Photos:     {ev.total_raw_count}")
        print(f"    Curated Assets: {len(ev.assets)}")
        print(f"    People:         {', '.join(ev.people) if ev.people else 'None'}")
        print(f"    Top Highlight Samples:")
        for a in ev.assets[:3]:
            print(f"      • [{a.media_type}] {a.file_path.name} | Sharpness: {a.sharpness:.1f} | Quality Score: {a.quality_score:.1f}")
        print()


if __name__ == "__main__":
    main()
