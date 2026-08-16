from fastapi import APIRouter, Depends
from sqlmodel import Session, select
from database import MediaFile, get_session
from datetime import datetime, timedelta

router = APIRouter()

@router.get("/summary")
def stats_summary(db: Session = Depends(get_session)):
    all_files = db.exec(select(MediaFile)).all()
    total = len(all_files)
    pending = sum(1 for f in all_files if f.status == "pending")
    kept = sum(1 for f in all_files if f.status == "keep")
    trashed = sum(1 for f in all_files if f.status == "trashed")
    total_size = sum(f.size_bytes for f in all_files)
    saved_size = sum(f.size_bytes for f in all_files if f.status == "trashed")
    return {
        "total": total,
        "pending": pending,
        "kept": kept,
        "trashed": trashed,
        "total_size_bytes": total_size,
        "saved_bytes": saved_size,
        "swipes": kept + trashed,
    }
