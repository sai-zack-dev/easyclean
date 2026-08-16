from fastapi import APIRouter, Depends
from sqlmodel import Session, select
from database import MediaFile, get_session
from services.hasher import compute_phash, compute_blur
from collections import defaultdict
import hashlib

router = APIRouter()

@router.post("/scan")
def run_detection(db: Session = Depends(get_session)):
    files = db.exec(select(MediaFile).where(MediaFile.phash == None)).all()
    updated = 0
    hash_map: dict[str, list[int]] = defaultdict(list)

    for f in files:
        from pathlib import Path
        p = Path(f.path)
        if not p.exists():
            continue
        phash = compute_phash(p)
        blur_score = compute_blur(p)
        f.phash = phash
        f.blur_score = blur_score
        f.is_blurry = blur_score < 100 if blur_score is not None else False
        if phash:
            hash_map[phash].append(f.id)
        db.add(f)
        updated += 1

    # Mark duplicate groups
    dup_count = 0
    for phash, ids in hash_map.items():
        if len(ids) > 1:
            group_id = hashlib.md5(phash.encode()).hexdigest()[:8]
            for fid in ids:
                file = db.get(MediaFile, fid)
                if file:
                    file.duplicate_group = group_id
                    db.add(file)
                    dup_count += 1

    db.commit()
    return {
        "scanned": updated,
        "blurry": sum(1 for f in db.exec(select(MediaFile)).all() if f.is_blurry),
        "duplicate_pairs": dup_count // 2
    }

@router.get("/summary")
def detection_summary(db: Session = Depends(get_session)):
    all_files = db.exec(select(MediaFile)).all()
    blurry = [f for f in all_files if f.is_blurry and f.status == "pending"]
    dup_ids = set()
    dup_groups: dict[str, list] = defaultdict(list)
    for f in all_files:
        if f.duplicate_group and f.status == "pending":
            dup_groups[f.duplicate_group].append(f.id)
    for g, ids in dup_groups.items():
        if len(ids) > 1:
            dup_ids.update(ids)
    return {
        "blurry_count": len(blurry),
        "duplicate_pairs": len(dup_ids) // 2,
        "duplicate_groups": len(dup_groups)
    }
