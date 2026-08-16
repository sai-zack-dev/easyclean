from fastapi import APIRouter, Depends
from sqlmodel import Session, select
from database import MediaFile, get_session
from pathlib import Path
import os

router = APIRouter()

@router.get("/")
def list_trash(db: Session = Depends(get_session)):
    files = db.exec(select(MediaFile).where(MediaFile.status == "trashed")).all()
    return [{"id": f.id, "filename": f.filename, "size_bytes": f.size_bytes,
             "trashed_at": f.trashed_at.isoformat() if f.trashed_at else None,
             "thumb_url": f"/api/media/{f.id}/thumb"} for f in files]

@router.post("/restore/{file_id}")
def restore(file_id: int, db: Session = Depends(get_session)):
    f = db.get(MediaFile, file_id)
    if not f:
        from fastapi import HTTPException
        raise HTTPException(404)
    f.status = "pending"
    f.trashed_at = None
    db.add(f)
    db.commit()
    return {"id": f.id, "status": "pending"}

@router.delete("/purge/{file_id}")
def purge(file_id: int, db: Session = Depends(get_session)):
    f = db.get(MediaFile, file_id)
    if not f:
        from fastapi import HTTPException
        raise HTTPException(404)
    p = Path(f.path)
    if p.exists():
        p.unlink()
    db.delete(f)
    db.commit()
    return {"deleted": file_id}

@router.delete("/purge-all")
def purge_all(db: Session = Depends(get_session)):
    files = db.exec(select(MediaFile).where(MediaFile.status == "trashed")).all()
    count = 0
    for f in files:
        p = Path(f.path)
        if p.exists():
            p.unlink()
        db.delete(f)
        count += 1
    db.commit()
    return {"deleted": count}
