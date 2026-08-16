from pathlib import Path
from typing import Optional
import os

THUMB_DIR = Path(os.getenv("THUMB_DIR", "/tmp/easyclean_thumbs"))

def ensure_thumb(path: Path, file_id: int) -> Optional[Path]:
    THUMB_DIR.mkdir(parents=True, exist_ok=True)
    thumb_path = THUMB_DIR / f"{file_id}.jpg"
    if thumb_path.exists():
        return thumb_path
    if not path.exists():
        return None
    try:
        from PIL import Image
        img = Image.open(path)
        img.thumbnail((400, 600))
        img = img.convert("RGB")
        img.save(str(thumb_path), "JPEG", quality=75)
        return thumb_path
    except Exception:
        return None
