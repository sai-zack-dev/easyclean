from fastapi import APIRouter, Depends, HTTPException, Query
from fastapi.responses import FileResponse
from sqlmodel import Session, select
from pydantic import BaseModel
from datetime import datetime
from pathlib import Path
import os, mimetypes

from database import MediaFile, get_session
from services.scanner import scan_media_folder
from services.thumbnailer import ensure_thumb

router = APIRouter()
MEDIA_ROOT = Path(os.getenv("MEDIA_ROOT", "./media"))

class SwipeAction(BaseModel):
    file_id: int
    action: str  # "keep" | "delete"

@router.post("/scan")
def scan(db: Session = Depends(get_session)):
    added = scan_media_folder(MEDIA_ROOT, db)
    return {"added": added, "message": f"Scanned {MEDIA_ROOT}, added {added} files"}

@router.get("/batch")
def get_batch(
    mode: str = Query("timeline"),  # timeline | blurs | duplicates
    month: str = Query(None),       # e.g. "2026-02"
    size: int = Query(50),
    db: Session = Depends(get_session)
):
    query = select(MediaFile).where(MediaFile.status == "pending")

    if mode == "blurs":
        query = select(MediaFile).where(
            MediaFile.status == "pending",
            MediaFile.is_blurry == True
        )
    elif mode == "duplicates":
        query = select(MediaFile).where(
            MediaFile.status == "pending",
            MediaFile.duplicate_group != None
        )
    elif mode == "timeline" and month:
        # Filter by month prefix from created_at
        files = db.exec(query).all()
        files = [f for f in files if f.created_at.strftime("%Y-%m") == month]
        return {"files": [_serialize(f) for f in files[:size]], "total": len(files)}

    files = db.exec(query.limit(size)).all()
    return {"files": [_serialize(f) for f in files], "total": len(files)}

@router.post("/swipe")
def swipe(action: SwipeAction, db: Session = Depends(get_session)):
    file = db.get(MediaFile, action.file_id)
    if not file:
        raise HTTPException(404, "File not found")
    if action.action == "delete":
        file.status = "trashed"
        file.trashed_at = datetime.utcnow()
    elif action.action == "keep":
        file.status = "keep"
    db.add(file)
    db.commit()
    return {"id": file.id, "status": file.status}

@router.get("/timeline")
def get_timeline(db: Session = Depends(get_session)):
    """Returns month buckets with counts for timeline checklist."""
    files = db.exec(select(MediaFile).where(MediaFile.status != "trashed")).all()
    buckets: dict = {}
    for f in files:
        key = f.created_at.strftime("%b %Y")
        if key not in buckets:
            buckets[key] = {"month": key, "total": 0, "reviewed": 0, "sort_key": f.created_at.strftime("%Y-%m")}
        buckets[key]["total"] += 1
        if f.status in ("keep", "deleted"):
            buckets[key]["reviewed"] += 1
    sorted_buckets = sorted(buckets.values(), key=lambda x: x["sort_key"], reverse=True)
    return sorted_buckets

@router.get("/{file_id}/thumb")
def get_thumb(file_id: int, db: Session = Depends(get_session)):
    file = db.get(MediaFile, file_id)
    if not file:
        raise HTTPException(404)
    thumb = ensure_thumb(Path(file.path), file_id)
    if not thumb or not thumb.exists():
        raise HTTPException(404, "Thumbnail not available")
    return FileResponse(str(thumb))

@router.get("/{file_id}/raw")
def get_raw(file_id: int, db: Session = Depends(get_session)):
    file = db.get(MediaFile, file_id)
    if not file:
        raise HTTPException(404)
    p = Path(file.path)
    if not p.exists():
        raise HTTPException(404, "File missing on disk")
    mime = mimetypes.guess_type(str(p))[0] or "application/octet-stream"
    return FileResponse(str(p), media_type=mime)

def _serialize(f: MediaFile):
    return {
        "id": f.id,
        "filename": f.filename,
        "media_type": f.media_type,
        "size_bytes": f.size_bytes,
        "created_at": f.created_at.isoformat(),
        "is_blurry": f.is_blurry,
        "blur_score": f.blur_score,
        "status": f.status,
        "duplicate_group": f.duplicate_group,
        "thumb_url": f"/api/media/{f.id}/thumb",
        "raw_url": f"/api/media/{f.id}/raw",
    }
