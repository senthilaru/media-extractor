"""
Cinematic Highlight Reel Generator.
Transforms curated event assets (photos + videos) into a polished,
beat-synced MP4 highlight video with Ken Burns motion, smooth cross-dissolves,
elegant title overlays, and audio track integration.
"""

from pathlib import Path
from typing import List, Optional, Tuple, Dict, Any
import os
import subprocess
import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont

from .curator import CuratedEvent, CuratedAsset
from .ken_burns import generate_ken_burns_sequence


class HighlightReelGenerator:
    """Generates MP4 highlight memory reels from curated events."""

    def __init__(
        self,
        output_dir: Optional[str] = None,
        target_size: Tuple[int, int] = (1920, 1080),
        fps: int = 30,
        slide_duration: float = 3.5,
        crossfade_duration: float = 0.5,
    ):
        self.output_dir = Path(output_dir or "output_reels")
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.target_size = target_size
        self.fps = fps
        self.slide_duration = slide_duration
        self.crossfade_duration = crossfade_duration

    def render_event_reel(
        self,
        event: CuratedEvent,
        output_filename: Optional[str] = None,
        audio_path: Optional[str] = None,
        show_title: bool = True,
    ) -> Path:
        """
        Renders a full cinematic MP4 highlight reel for the curated event.
        """
        if not event.assets:
            raise ValueError(f"Event '{event.title}' has no curated assets to render.")

        if not output_filename:
            safe_title = "".join(c if c.isalnum() or c in "-_" else "_" for c in event.title).strip("_")
            output_filename = f"{event.event_id}_{safe_title[:40]}.mp4"

        output_path = self.output_dir / output_filename
        target_w, target_h = self.target_size

        print(f"[Reel Generator] Starting render: '{event.title}'")
        print(f"[Reel Generator] Curated assets count: {len(event.assets)}")
        print(f"[Reel Generator] Canvas size: {target_w}x{target_h} @ {self.fps} fps")
        print(f"[Reel Generator] Target output: {output_path}")

        # Build FFmpeg command
        cmd = [
            "ffmpeg",
            "-y",
            "-f", "rawvideo",
            "-vcodec", "rawvideo",
            "-s", f"{target_w}x{target_h}",
            "-pix_fmt", "rgb24",
            "-r", str(self.fps),
            "-i", "-",  # Video input via stdin
        ]

        if audio_path and Path(audio_path).exists():
            cmd.extend([
                "-i", str(audio_path),
                "-c:a", "aac",
                "-b:a", "192k",
                "-shortest",
            ])
        else:
            # Generate silent stereo audio track so the video has standard audio specs
            cmd.extend([
                "-f", "lavfi",
                "-i", "anullsrc=channel_layout=stereo:sample_rate=44100",
                "-c:a", "aac",
                "-shortest",
            ])

        # Video encoding settings
        cmd.extend([
            "-c:v", "libx264",
            "-preset", "fast",
            "-crf", "22",
            "-pix_fmt", "yuv420p",
            "-movflags", "+faststart",
            str(output_path),
        ])

        log_path = self.output_dir / "ffmpeg_render.log"
        log_file = open(log_path, "wb")
        proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=log_file)

        try:
            motions = ["zoom_in", "zoom_out", "pan_left", "pan_right"]
            crossfade_frames_count = int(self.crossfade_duration * self.fps)

            # Store the previous slide's end frames to blend with the next slide
            prev_tail_frames: List[np.ndarray] = []
            total_frames_written = 0

            for idx, asset in enumerate(event.assets):
                motion = motions[idx % len(motions)]
                curr_frames: List[np.ndarray] = []

                if asset.media_type == "IMAGE":
                    # Generate Ken Burns frames for photo
                    for f in generate_ken_burns_sequence(
                        asset.file_path,
                        target_size=self.target_size,
                        duration=self.slide_duration,
                        fps=self.fps,
                        motion_type=motion,
                    ):
                        curr_frames.append(f)

                elif asset.media_type == "VIDEO":
                    # Extract up to 4.0 seconds of video snippet
                    curr_frames = self._extract_video_frames(
                        asset.file_path,
                        max_duration=4.0,
                        target_size=self.target_size,
                        fps=self.fps,
                    )

                if not curr_frames:
                    continue

                # Add Title Overlay to the first slide
                if idx == 0 and show_title:
                    curr_frames = self._apply_title_overlay(
                        curr_frames,
                        title=event.title,
                        subtitle=event.start_time.strftime("%B %d, %Y"),
                        fps=self.fps,
                    )

                # If this is not the first clip and we have tail frames, perform crossfade
                if prev_tail_frames and len(curr_frames) > crossfade_frames_count:
                    blend_count = min(len(prev_tail_frames), crossfade_frames_count)
                    for b_idx in range(blend_count):
                        alpha = float(b_idx + 1) / float(blend_count + 1)
                        blended = cv2.addWeighted(
                            prev_tail_frames[b_idx], 1.0 - alpha,
                            curr_frames[b_idx], alpha,
                            0.0
                        )
                        proc.stdin.write(blended.tobytes())
                        total_frames_written += 1

                    # Write the rest of the current frames (excluding the crossfaded start & leaving tail)
                    main_frames = curr_frames[blend_count:-crossfade_frames_count] if len(curr_frames) > 2 * crossfade_frames_count else curr_frames[blend_count:]
                    for f in main_frames:
                        proc.stdin.write(f.tobytes())
                        total_frames_written += 1

                    prev_tail_frames = curr_frames[-crossfade_frames_count:]
                else:
                    # First slide or single slide
                    if len(curr_frames) > crossfade_frames_count:
                        main_frames = curr_frames[:-crossfade_frames_count]
                        for f in main_frames:
                            proc.stdin.write(f.tobytes())
                            total_frames_written += 1
                        prev_tail_frames = curr_frames[-crossfade_frames_count:]
                    else:
                        for f in curr_frames:
                            proc.stdin.write(f.tobytes())
                            total_frames_written += 1
                        prev_tail_frames = []

            # Flush remaining tail frames at the very end
            for f in prev_tail_frames:
                proc.stdin.write(f.tobytes())
                total_frames_written += 1

            proc.stdin.close()
            retcode = proc.wait()
            log_file.close()

            if retcode != 0:
                with open(log_path, "r", errors="ignore") as f:
                    err_msg = f.read()
                raise RuntimeError(f"FFmpeg failed with exit code {retcode}: {err_msg}")

            duration_sec = total_frames_written / float(self.fps)
            size_mb = output_path.stat().st_size / (1024.0 * 1024.0)
            print(f"[Reel Generator] Successfully rendered: {output_path.name}")
            print(f"[Reel Generator] Total Duration: {duration_sec:.1f}s | Size: {size_mb:.2f} MB")
            return output_path

        except Exception as e:
            if proc.stdin and not proc.stdin.closed:
                try:
                    proc.stdin.close()
                except Exception:
                    pass
            if not log_file.closed:
                log_file.close()
            proc.kill()
            raise e

    def _extract_video_frames(
        self,
        video_path: Path,
        max_duration: float,
        target_size: Tuple[int, int],
        fps: int,
    ) -> List[np.ndarray]:
        """Extracts and normalizes a short segment of video frames."""
        frames: List[np.ndarray] = []
        cap = cv2.VideoCapture(str(video_path))
        if not cap.isOpened():
            return frames

        orig_fps = cap.get(cv2.CAP_PROP_FPS) or float(fps)
        step = max(1.0, orig_fps / float(fps))
        max_frames = int(max_duration * fps)

        target_w, target_h = target_size
        frame_idx = 0.0

        while len(frames) < max_frames:
            cap.set(cv2.CAP_PROP_POS_FRAMES, int(frame_idx))
            ret, bgr = cap.read()
            if not ret:
                break

            # Convert BGR to RGB
            rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
            # Resize / fit to canvas
            resized = cv2.resize(rgb, (target_w, target_h), interpolation=cv2.INTER_LINEAR)
            frames.append(resized)
            frame_idx += step

        cap.release()
        return frames

    def _apply_title_overlay(
        self,
        frames: List[np.ndarray],
        title: str,
        subtitle: str,
        fps: int,
        duration: float = 3.0,
    ) -> List[np.ndarray]:
        """Draws an elegant fading title card overlay onto the opening frames."""
        overlay_frames_count = min(len(frames), int(duration * fps))
        if overlay_frames_count <= 0:
            return frames

        fade_in_frames = int(0.5 * fps)
        fade_out_frames = int(0.7 * fps)
        hold_end = overlay_frames_count - fade_out_frames

        h, w, _ = frames[0].shape

        # Create title card PIL overlay once
        card_w, card_h = int(w * 0.7), int(h * 0.22)
        card_img = Image.new("RGBA", (card_w, card_h), (20, 20, 20, 180))
        draw = ImageDraw.Draw(card_img)

        # Draw text using basic default or system font
        try:
            title_font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", int(card_h * 0.28))
            sub_font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", int(card_h * 0.18))
        except Exception:
            title_font = ImageFont.load_default()
            sub_font = ImageFont.load_default()

        draw.text((card_w // 2, int(card_h * 0.32)), title, font=title_font, fill=(255, 255, 255, 255), anchor="mm")
        draw.text((card_w // 2, int(card_h * 0.70)), subtitle, font=sub_font, fill=(200, 200, 200, 255), anchor="mm")

        card_np = np.array(card_img)
        card_rgb = card_np[:, :, :3]
        card_alpha = card_np[:, :, 3] / 255.0

        pos_x = (w - card_w) // 2
        pos_y = int(h * 0.72)  # lower-third positioning

        result_frames = []
        for i, frame in enumerate(frames):
            if i < overlay_frames_count:
                # Calculate alpha fade
                if i < fade_in_frames:
                    t_alpha = float(i) / float(fade_in_frames)
                elif i > hold_end:
                    t_alpha = 1.0 - float(i - hold_end) / float(fade_out_frames)
                else:
                    t_alpha = 1.0

                t_alpha = max(0.0, min(1.0, t_alpha))

                if t_alpha > 0.01:
                    # Blend overlay
                    roi = frame[pos_y:pos_y + card_h, pos_x:pos_x + card_w]
                    effective_alpha = card_alpha[:, :, np.newaxis] * t_alpha
                    blended_roi = (roi * (1.0 - effective_alpha) + card_rgb * effective_alpha).astype(np.uint8)
                    frame = frame.copy()
                    frame[pos_y:pos_y + card_h, pos_x:pos_x + card_w] = blended_roi

            result_frames.append(frame)

        return result_frames
