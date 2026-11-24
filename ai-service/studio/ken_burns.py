"""
Ken Burns Dynamic Motion & Frame Synthesis Engine.
Generates smooth cinematic pan-and-zoom animations with ease-in-out curves
and intelligent blurred-background padding for mismatched aspect ratios.
"""

from pathlib import Path
from typing import Generator, Tuple, Optional
import math
import numpy as np
import cv2
from PIL import Image, ImageOps, ImageFilter


def ease_in_out_sine(x: float) -> float:
    """Smooth sinusoidal easing from 0.0 to 1.0."""
    return -(math.cos(math.pi * x) - 1.0) / 2.0


def prepare_canvas_frame(
    img: Image.Image,
    target_width: int,
    target_height: int,
    zoom_factor: float,
    pan_x: float,  # -1.0 (left) to 1.0 (right)
    pan_y: float,  # -1.0 (top) to 1.0 (bottom)
    cached_bg: Optional[Image.Image] = None,
) -> np.ndarray:
    """
    Renders a single frame with Ken Burns zoom & pan.
    If aspect ratio differs substantially (e.g. portrait photo on landscape canvas),
    uses precomputed blurred background with the centered sharp image.
    """
    canvas_ratio = target_width / target_height
    img_w, img_h = img.size
    img_ratio = img_w / img_h

    # If aspect ratios are compatible (within 35%)
    aspect_diff = abs(img_ratio - canvas_ratio) / canvas_ratio

    if aspect_diff <= 0.35:
        # Direct crop & zoom
        if img_ratio > canvas_ratio:
            base_h = img_h
            base_w = int(img_h * canvas_ratio)
        else:
            base_w = img_w
            base_h = int(img_w / canvas_ratio)

        # Apply zoom factor (zoom > 1.0 means crop smaller area)
        crop_w = max(10, int(base_w / zoom_factor))
        crop_h = max(10, int(base_h / zoom_factor))

        max_pan_x = (img_w - crop_w) / 2.0
        max_pan_y = (img_h - crop_h) / 2.0

        center_x = img_w / 2.0 + pan_x * max_pan_x
        center_y = img_h / 2.0 + pan_y * max_pan_y

        left = int(max(0, min(img_w - crop_w, center_x - crop_w / 2.0)))
        top = int(max(0, min(img_h - crop_h, center_y - crop_h / 2.0)))
        right = left + crop_w
        bottom = top + crop_h

        cropped = img.crop((left, top, right, bottom))
        resized = cropped.resize((target_width, target_height), Image.Resampling.BILINEAR)
        return np.array(resized)

    else:
        # Significant aspect ratio mismatch (e.g. portrait photo on 16:9 canvas)
        if cached_bg is not None:
            bg = cached_bg.copy()
        else:
            small_bg = img.resize((target_width // 4, target_height // 4), Image.Resampling.BILINEAR)
            small_bg = small_bg.filter(ImageFilter.GaussianBlur(radius=6))
            bg = small_bg.resize((target_width, target_height), Image.Resampling.BILINEAR)

        # Foreground: Fitted in center with Ken Burns zoom applied
        fg_scale_base = min(target_width / img_w, target_height / img_h) * 0.95
        fg_scale = fg_scale_base * zoom_factor
        fg_w = int(img_w * fg_scale)
        fg_h = int(img_h * fg_scale)
        fg = img.resize((fg_w, fg_h), Image.Resampling.BILINEAR)

        paste_x = (target_width - fg_w) // 2 + int(pan_x * 20)
        paste_y = (target_height - fg_h) // 2 + int(pan_y * 20)
        bg.paste(fg, (paste_x, paste_y))

        return np.array(bg)


def generate_ken_burns_sequence(
    image_path: Path,
    target_size: Tuple[int, int] = (1920, 1080),
    duration: float = 3.5,
    fps: int = 30,
    motion_type: str = "zoom_in",
) -> Generator[np.ndarray, None, None]:
    """
    Yields RGB frames for an image over the specified duration with smooth motion.
    motion_type: 'zoom_in', 'zoom_out', 'pan_left', 'pan_right'
    """
    total_frames = max(1, int(duration * fps))

    # Open and orient image
    with Image.open(image_path) as raw_img:
        img = ImageOps.exif_transpose(raw_img)
        if img.mode != "RGB":
            img = img.convert("RGB")

        # Downsample extremely massive images (> 4K) to avoid memory overhead
        if max(img.size) > 3840:
            scale = 3840.0 / max(img.size)
            img = img.resize((int(img.width * scale), int(img.height * scale)), Image.Resampling.BILINEAR)

        target_w, target_h = target_size
        img_w, img_h = img.size
        canvas_ratio = target_w / target_h
        img_ratio = img_w / img_h
        aspect_diff = abs(img_ratio - canvas_ratio) / canvas_ratio

        # Precompute blurred background once if aspect ratio differs substantially
        cached_bg = None
        if aspect_diff > 0.35:
            small_bg = img.resize((target_w // 4, target_h // 4), Image.Resampling.BILINEAR)
            small_bg = small_bg.filter(ImageFilter.GaussianBlur(radius=6))
            cached_bg = small_bg.resize((target_w, target_h), Image.Resampling.BILINEAR)

        for f in range(total_frames):
            progress = ease_in_out_sine(f / float(total_frames))

            if motion_type == "zoom_in":
                zoom = 1.0 + 0.12 * progress
                pan_x = 0.05 * progress
                pan_y = -0.05 * progress
            elif motion_type == "zoom_out":
                zoom = 1.12 - 0.12 * progress
                pan_x = -0.05 * progress
                pan_y = 0.05 * progress
            elif motion_type == "pan_left":
                zoom = 1.06
                pan_x = 0.15 * (1.0 - 2.0 * progress)
                pan_y = 0.0
            else:  # pan_right
                zoom = 1.06
                pan_x = -0.15 * (1.0 - 2.0 * progress)
                pan_y = 0.0

            frame = prepare_canvas_frame(
                img,
                target_width=target_w,
                target_height=target_h,
                zoom_factor=zoom,
                pan_x=pan_x,
                pan_y=pan_y,
                cached_bg=cached_bg,
            )
            yield frame
