"""
Command-Line Interface for generating Occasion Memory Highlight Reels with Tamil Music & Immich Publishing.
Usage:
    python ai-service/studio/generate_reel.py --person "Akshara" --event-idx 1
    python ai-service/studio/generate_reel.py --person "Akshara" --theme father_daughter
    python ai-service/studio/generate_reel.py --person "Akshara" --song "Aanandha Yaazhai"
    python ai-service/studio/generate_reel.py --query "cake" --theme birthday_celebration
    python ai-service/studio/generate_reel.py --person "Akshara" --orientation portrait --publish
"""

import argparse
import os
import sys
from pathlib import Path

# Setup paths
current_dir = Path(__file__).resolve().parent
ai_service_dir = current_dir.parent
root_dir = ai_service_dir.parent

if str(ai_service_dir) not in sys.path:
    sys.path.insert(0, str(ai_service_dir))

from studio.immich_client import ImmichClient
from studio.curator import EventCurator
from studio.reel_generator import HighlightReelGenerator
from studio.tamil_music import TamilMusicManager, TAMIL_THEMES


def parse_args():
    parser = argparse.ArgumentParser(description="Immich AI Highlight Reel Studio with Tamil Music")
    parser.add_argument("--person", type=str, default=None, help="Name of the person from Immich")
    parser.add_argument("--query", type=str, default=None, help="CLIP smart search query (e.g. birthday, cake, beach)")
    parser.add_argument("--event-idx", type=int, default=1, help="Which event cluster index to render (1-based)")
    parser.add_argument("--output-dir", type=str, default="output_reels", help="Directory to save generated MP4")
    parser.add_argument("--audio", type=str, default=None, help="Optional local background music file path")
    parser.add_argument(
        "--theme",
        type=str,
        default=None,
        choices=list(TAMIL_THEMES.keys()) + ["auto"],
        help="Tamil music mood theme (father_daughter, mother_sentiments, couples_romance, celebration_party, family_nostalgia, birthday_celebration)",
    )
    parser.add_argument("--song", type=str, default=None, help="Specific Tamil song name to search and download")
    parser.add_argument("--no-music", action="store_true", help="Do not attach music to the reel")
    parser.add_argument("--orientation", choices=["landscape", "portrait"], default="landscape", help="Video aspect ratio")
    parser.add_argument("--slide-duration", type=float, default=3.5, help="Duration in seconds per photo")
    parser.add_argument("--fps", type=int, default=30, help="Video framerate")
    parser.add_argument("--publish", action="store_true", default=True, help="Publish rendered reel and album to Immich (default: True)")
    parser.add_argument("--no-publish", dest="publish", action="store_false", help="Skip uploading to Immich")
    return parser.parse_args()


def main():
    args = parse_args()

    # Default to Akshara if neither person nor query is provided
    target_person = args.person
    target_query = args.query
    if not target_person and not target_query:
        target_person = "Akshara"

    print("=" * 65)
    print(" Immich AI Studio: Tamil Sentimental Highlight Reel Generator")
    print("=" * 65)

    client = ImmichClient()
    curator = EventCurator(client)
    music_mgr = TamilMusicManager()

    if target_person:
        print(f"Discovering events for Person: '{target_person}'...")
        events = curator.fetch_and_cluster(person_name=target_person)
    else:
        print(f"Discovering events for Query: '{target_query}'...")
        events = curator.fetch_and_cluster(query=target_query)

    if not events:
        print("No event clusters found matching criteria.")
        sys.exit(1)

    print(f"\nDiscovered {len(events)} Event Clusters:")
    for idx, ev in enumerate(events, 1):
        inferred_t = music_mgr.infer_theme(ev)
        print(f"  [{idx}] {ev.title} ({len(ev.assets)} curated assets) [Mood: {inferred_t}]")

    # Select event
    chosen_idx = max(1, min(len(events), args.event_idx)) - 1
    chosen_event = events[chosen_idx]

    print(f"\nRendering Event [{chosen_idx + 1}]: '{chosen_event.title}'")
    print(f"Curated Shots: {len(chosen_event.assets)} files")

    # Compute expected duration: (slide_duration * N) - (crossfade * (N-1))
    crossfade = 0.5
    num_assets = len(chosen_event.assets)
    estimated_duration = max(3.0, (num_assets * args.slide_duration) - ((num_assets - 1) * crossfade))

    # Resolve Audio Track
    audio_file = None
    song_info = None

    if args.audio and Path(args.audio).exists():
        audio_file = Path(args.audio)
        print(f"[Audio] Using custom local file: {audio_file}")
    elif not args.no_music:
        # Select Tamil Sentimental Track
        theme = None if args.theme == "auto" else args.theme
        song_info = music_mgr.select_track(
            theme=theme,
            custom_song=args.song,
            event=chosen_event,
        )
        print("-" * 65)
        print(f"🎵 Selected Tamil Music Theme: {song_info.get('theme', 'Custom')}")
        print(f"🎵 Song:    {song_info['title']} ({song_info.get('artist', '')})")
        print(f"🎵 Tagline: \"{song_info.get('tagline', '')}\"")
        try:
            audio_file = music_mgr.prepare_audio(
                track_info=song_info,
                target_duration=estimated_duration,
            )
        except Exception as e:
            print(f"[Warning] Failed to fetch Tamil audio track: {e}. Falling back to clean audio.")
            audio_file = None

    # Set canvas resolution
    canvas_size = (1920, 1080) if args.orientation == "landscape" else (1080, 1920)

    generator = HighlightReelGenerator(
        output_dir=args.output_dir,
        target_size=canvas_size,
        fps=args.fps,
        slide_duration=args.slide_duration,
        crossfade_duration=crossfade,
    )

    out_file = generator.render_event_reel(
        event=chosen_event,
        audio_path=str(audio_file) if audio_file else None,
        show_title=True,
    )

    print("=" * 65)
    print(" REEL GENERATION COMPLETE!")
    print(f" Saved video to: {out_file.resolve()}")

    if args.publish and client.is_configured():
        print("-" * 65)
        print("Publishing to Immich Server...")
        try:
            # 1. Upload the generated MP4 reel
            print("  • Uploading highlight reel asset...")
            upload_res = client.upload_asset(out_file, created_at=chosen_event.start_time)
            reel_asset_id = upload_res.get("id")
            print(f"    Uploaded Reel ID: {reel_asset_id}")

            # 2. Collect asset IDs (curated original photos + reel)
            curated_ids = [a.asset_id for a in chosen_event.assets if a.asset_id]
            all_asset_ids = [reel_asset_id] + curated_ids if reel_asset_id else curated_ids

            # 3. Create Immich Album with song details
            album_title = f"{chosen_event.title} (Highlights)"
            song_desc = f" 🎵 Music: {song_info['title']}" if song_info else ""
            print(f"  • Creating Immich Album: '{album_title}'...")
            album_res = client.create_album(
                album_name=album_title,
                description=f"Curated highlight memories generated by Immich AI Studio on {chosen_event.start_time.strftime('%b %d, %Y')}.{song_desc}",
                asset_ids=all_asset_ids,
            )
            album_id = album_res.get("id")
            print(f"    Album Created: {album_id}")
            print(f"    Direct Immich URL: {client.base_url}/albums/{album_id}")
            print("Successfully published to Immich! You can now view it on phone/web.")
        except Exception as e:
            print(f"Warning: Failed to publish to Immich: {e}")

    print("=" * 65)


if __name__ == "__main__":
    main()
