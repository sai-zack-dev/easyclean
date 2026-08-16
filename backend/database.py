from sqlmodel import SQLModel, Field, Session, create_engine, select
from typing import Optional
from datetime import datetime
import os

DATABASE_URL = os.getenv("DATABASE_URL", "sqlite:///./easyclean.db")
engine = create_engine(DATABASE_URL, echo=False)

class MediaFile(SQLModel, table=True):
    id: Optional[int] = Field(default=None, primary_key=True)
    path: str = Field(unique=True)
    filename: str
    media_type: str  # "image" | "video"
    size_bytes: int
    created_at: datetime
    phash: Optional[str] = None
    blur_score: Optional[float] = None
    is_blurry: bool = False
    status: str = "pending"  # pending | keep | deleted | trashed
    trashed_at: Optional[datetime] = None
    duplicate_group: Optional[str] = None
    thumb_path: Optional[str] = None

def init_db():
    SQLModel.metadata.create_all(engine)

def get_session():
    with Session(engine) as session:
        yield session
