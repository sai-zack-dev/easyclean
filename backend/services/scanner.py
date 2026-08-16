from pathlib import Path
from sqlmodel import Session, select
from database import MediaFile
from datetime import datetime

IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".heic", ".webp", ".gif", ".bmp", ".tiff"}
VIDEO_EXTS = {".mp4", ".mov", ".avi", ".mkv", ".m4v", ".3gp"}

def scan_media_folder(root: Path, db: Session) -> int:
    """
    Scans /mnt/easystore/storage/<username>/ for all users.
    root = /mnt/easystore/storage
    """
    if not root.exists():
        return 0

    existing = set(row[0] for row in db.exec(select(MediaFile.path)).all())
    added = 0

    # Walk each user subfolder: /mnt/easystore/storage/zack/, /mnt/easystore/storage/lily/, etc.
    scan_roots = [d for d in root.iterdir() if d.is_dir()]
    if not scan_roots:
        scan_roots = [root]  # fallback: scan root itself

    for user_dir in scan_roots:
        for p in user_dir.rglob("*"):
            if not p.is_file():
                continue
            suffix = p.suffix.lower()
            if suffix not in IMAGE_EXTS and suffix not in VIDEO_EXTS:
                continue
            if str(p) in existing:
                continue

            media_type = "image" if suffix in IMAGE_EXTS else "video"
            stat = p.stat()
            created_at = datetime.fromtimestamp(stat.st_mtime)

            if media_type == "image":
                try:
                    from PIL import Image
                    from PIL.ExifTags import TAGS
                    img = Image.open(p)
                    exif = img._getexif()
                    if exif:
                        for tag_id, value in exif.items():
                            if TAGS.get(tag_id) == "DateTimeOriginal":
                                created_at = datetime.strptime(value, "%Y:%m:%d %H:%M:%S")
                                break
                except Exception:
                    pass

            file = MediaFile(
                path=str(p),
                filename=p.name,
                media_type=media_type,
                size_bytes=stat.st_size,
                created_at=created_at,
            )
            db.add(file)
            added += 1

    db.commit()
    return added
