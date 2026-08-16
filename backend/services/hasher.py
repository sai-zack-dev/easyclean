from pathlib import Path
from typing import Optional

def compute_phash(path: Path) -> Optional[str]:
    try:
        import imagehash
        from PIL import Image
        img = Image.open(path).convert("RGB")
        return str(imagehash.phash(img))
    except Exception:
        return None

def compute_blur(path: Path) -> Optional[float]:
    try:
        import cv2
        import numpy as np
        img = cv2.imread(str(path))
        if img is None:
            return None
        gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
        return float(cv2.Laplacian(gray, cv2.CV_64F).var())
    except Exception:
        return None
