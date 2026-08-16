from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from contextlib import asynccontextmanager
import os

from database import init_db
from routers import media, detection, trash, stats

@asynccontextmanager
async def lifespan(app: FastAPI):
    init_db()
    yield

app = FastAPI(title="EasyClean API", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(media.router,      prefix="/api/media",     tags=["media"])
app.include_router(detection.router,  prefix="/api/detection", tags=["detection"])
app.include_router(trash.router,      prefix="/api/trash",     tags=["trash"])
app.include_router(stats.router,      prefix="/api/stats",     tags=["stats"])

@app.get("/api/health")
def health():
    return {"status": "ok"}
