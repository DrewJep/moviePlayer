import os
import sys
import re
import json
import time
import asyncio
from typing import Optional, List

import requests
from fastapi import FastAPI, HTTPException, Query
from dotenv import load_dotenv

# --- Load environment variables ---
load_dotenv()

PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
DB_DIR = os.path.join(PROJECT_ROOT, 'db')
if DB_DIR not in sys.path:
    sys.path.insert(0, DB_DIR)

import db_access  # your async DB functions

OMDB_APIKEY = os.environ.get('OMDB_APIKEY')

# --- FastAPI app ---
app = FastAPI(title="Movie API")

# --- Video file extensions and regex for year parsing ---
VIDEO_EXTS = {'.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flac', '.webm'}


# -------------------
# Helper functions
# -------------------

def parse_filename(name: str):
    """Parse filename into a cleaned title string.

    This deliberately does NOT attempt to extract a year from the filename.
    Returns (title, None) for backward compatibility with callers that
    expect two values.
    """
    if name.startswith('._'):
        return None, None

    base = os.path.splitext(name)[0]
    s = re.sub(r'[._]+', ' ', base)
    title = re.sub(r'\b(1080p|720p|2160p|x264|x265|h264|bluray|brrip|web[- ]dl|dvdrip)\b', '', s, flags=re.I).strip()
    title = re.sub(r'\s{2,}', ' ', title)
    return title, None


async def fetch_omdb_data(title: str, year: Optional[int] = None):
    """Query OMDB API and return dict or None if not found"""
    if not OMDB_APIKEY:
        return None

    params = {"t": title, "apikey": OMDB_APIKEY}
    if year:
        params["y"] = str(year)
    try:
        r = requests.get("http://www.omdbapi.com/", params=params, timeout=10)
        r.raise_for_status()
        data = r.json()
        if data.get("Response") == "True":
            return data
    except Exception:
        return None
    return None


async def insert_movie_from_file_with_omdb(path: str):
    """Parse local movie file, fetch OMDB data, and insert into DB if not duplicate"""
    name = os.path.basename(path)
    title, year = parse_filename(name)
    if not title:
        return

    # Skip if movie already exists. We prefer matching by `imdb_id` when available,
    # otherwise fall back to case-insensitive title match. We no longer require
    # matching by year because many filenames don't include it.
    existing = await db_access.get_movies_by_title(title)
    if existing:
        # If any existing record has the same title (case-insensitive), skip.
        low_title = title.lower()
        for e in existing:
            if e.get('imdb_id'):
                # If the file yields OMDB data later with same imdb_id we'd skip below.
                continue
            if isinstance(e.get('title'), str) and e.get('title').lower() == low_title:
                return

    # Fetch OMDB metadata
    data = await fetch_omdb_data(title, year)

    if data:
        movie_data = {
            "title": data.get("Title"),
            "year": int(data.get("Year")) if data.get("Year") and data.get("Year").isdigit() else None,
            "imdb_id": data.get("imdbID"),
            "genre": data.get("Genre"),
            "director": data.get("Director"),
            "actors": data.get("Actors"),
            "plot": data.get("Plot"),
            "language": data.get("Language"),
            "country": data.get("Country"),
            "poster_url": data.get("Poster"),
            "runtime": data.get("Runtime"),
            "rating": float(data.get("imdbRating")) if data.get("imdbRating") and data.get("imdbRating") != "N/A" else None,
            "additional_info": json.dumps(data)
        }
    else:
        # Minimal record if OMDB not found
        movie_data = {
            'title': title,
            'year': None,
            'imdb_id': None,
            'genre': None,
            'director': None,
            'actors': None,
            'plot': None,
            'language': None,
            'country': None,
            'poster_url': None,
            'runtime': None,
            'rating': None,
            'additional_info': json.dumps({'source': 'local file', 'filename': name})
        }

    await db_access.insert_movie(movie_data)
    print(f"Inserted: {title} ({name})")


async def scan_and_import_local_with_omdb(movies_dir: str):
    """Scan local folder, fetch OMDB, insert movies"""
    for dirpath, dirs, files in os.walk(movies_dir):
        for f in files:
            if os.path.splitext(f)[1].lower() in VIDEO_EXTS and not f.startswith('._'):
                full_path = os.path.join(dirpath, f)
                await insert_movie_from_file_with_omdb(full_path)
                await asyncio.sleep(0.1)  # modest delay


# -------------------
# FastAPI endpoints
# -------------------

@app.get("/movies/", response_model=List[dict])
async def get_movies(title: Optional[str] = None, limit: int = 50):
    if title:
        movies = await db_access.get_movies_by_title(title)
    else:
        movies = await db_access.get_all_movies(limit=limit)
    return movies


@app.get("/movies/{imdb_id}", response_model=dict)
async def get_movie(imdb_id: str):
    movie = await db_access.get_movie_by_imdb(imdb_id)
    if not movie:
        raise HTTPException(status_code=404, detail="Movie not found")
    return movie


@app.get("/search/")
async def search_omdb(title: str, year: Optional[int] = None, add_to_db: bool = False):
    data = await fetch_omdb_data(title, year)
    if not data:
        raise HTTPException(status_code=404, detail="Movie not found in OMDB")

    movie_data = {
        "title": data.get("Title"),
        "year": int(data.get("Year")) if data.get("Year") and data.get("Year").isdigit() else None,
        "imdb_id": data.get("imdbID"),
        "genre": data.get("Genre"),
        "director": data.get("Director"),
        "actors": data.get("Actors"),
        "plot": data.get("Plot"),
        "language": data.get("Language"),
        "country": data.get("Country"),
        "poster_url": data.get("Poster"),
        "runtime": data.get("Runtime"),
        "rating": float(data.get("imdbRating")) if data.get("imdbRating") and data.get("imdbRating") != "N/A" else None,
        "additional_info": json.dumps(data)
    }

    if add_to_db:
        await db_access.insert_movie(movie_data)

    return movie_data


@app.post("/movies/import_local/")
async def import_local_movies(path: Optional[str] = Query(None, description="Path to local movies folder")):
    """Scan local movies folder, fetch OMDB metadata, insert into DB without duplicates"""
    if path is None:
        path = os.path.abspath(os.path.join(PROJECT_ROOT, 'movies'))

    if not os.path.exists(path):
        raise HTTPException(status_code=404, detail=f"Movies folder not found: {path}")

    try:
        await scan_and_import_local_with_omdb(path)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to import local movies: {e}")

    return {"status": "done", "scanned_path": path}


@app.post("/movies/{imdb_id}/watch")
async def increment_watch_imdb(imdb_id: str):
    """Increment watch_count for a movie identified by imdb_id."""
    updated = await db_access.increment_watch_by_imdb(imdb_id)
    if not updated:
        raise HTTPException(status_code=404, detail="Movie not found")
    return {"status": "ok", "watch_count": updated.get("watch_count")}


@app.post("/movies/increment_watch/")
async def increment_watch_by_path(path: str = Query(..., description="file_key or file path variant")):
    """Increment watch_count for a movie by a file path or file_key.

    This endpoint will match `file_key` or any entry inside `file_paths` JSONB.
    """
    updated = await db_access.increment_watch_by_path(path)
    if not updated:
        raise HTTPException(status_code=404, detail="Movie not found for provided path")
    return {"status": "ok", "watch_count": updated.get("watch_count")}
