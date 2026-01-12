import os
import re
import sys
import json
import asyncio
import requests
from typing import Optional, Dict, Any
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

# Project paths
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
DB_DIR = os.path.join(PROJECT_ROOT, 'db')
if DB_DIR not in sys.path:
    sys.path.insert(0, DB_DIR)

import db_access

OMDB_APIKEY = os.environ.get('OMDB_APIKEY')
VIDEO_EXTS = {'.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flac', '.webm'}
def parse_filename(name: str) -> tuple[str, Optional[int]]:
    """Return a cleaned title extracted from the filename.

    Year extraction has been removed; function returns (title, None).
    """
    base = os.path.splitext(name)[0]
    s = re.sub(r'[._]+', ' ', base)
    title = re.sub(r'\b(1080p|720p|2160p|x264|x265|h264|bluray|brrip|web[- ]dl|dvdrip)\b', '', s, flags=re.I)
    title = re.sub(r'\s{2,}', ' ', title).strip()
    return title, None

def confirm_existence(title: str, year: Optional[int] = None) -> Optional[Dict[str, Any]]:
    if not OMDB_APIKEY:
        return None
    params = {'t': title, 'apikey': OMDB_APIKEY}
    try:
        r = requests.get('http://www.omdbapi.com/', params=params, timeout=10)
        r.raise_for_status()
        data = r.json()
        return data if data.get('Response') == 'True' else None
    except Exception as e:
        print(f"OMDB request failed for {title}: {e}")
        return None

async def insert_movie_from_file(path: str) -> None:
    name = os.path.basename(path)
    title, year = parse_filename(name)
    print(f"Processing: {name} -> title='{title}', year={year}")

    data = confirm_existence(title, year)
    if data:
        movie_data = {
            'title': data.get('Title') or title,
            'year': int(data.get('Year')) if data.get('Year') and data.get('Year').isdigit() else None,
            'imdb_id': data.get('imdbID'),
            'genre': data.get('Genre'),
            'director': data.get('Director'),
            'actors': data.get('Actors'),
            'plot': data.get('Plot'),
            'language': data.get('Language'),
            'country': data.get('Country'),
            'poster_url': data.get('Poster'),
            'runtime': data.get('Runtime'),
            'rating': float(data.get('imdbRating')) if data.get('imdbRating') and data.get('imdbRating') != 'N/A' else None,
            'additional_info': json.dumps(data)
        }
    else:
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
            'additional_info': json.dumps({'source': 'filename', 'filename': name})
        }

    try:
        await db_access.insert_movie(movie_data)
        print(f"Inserted/Skipped: {movie_data['title']} ({movie_data.get('imdb_id')})")
    except Exception as e:
        print(f"Failed to insert {name}: {e}")

    # Async-safe sleep to respect OMDB rate limits
    await asyncio.sleep(0.2)

async def scan_and_import(root: str, parallelism: int = 3) -> None:
    sem = asyncio.Semaphore(parallelism)
    async def sem_task(path):
        async with sem:
            await insert_movie_from_file(path)

    tasks = []
    for dirpath, dirs, files in os.walk(root):
        for f in files:
            if f.startswith('._') or f.startswith('.') or f.startswith('~'):
                continue
            if os.path.splitext(f)[1].lower() in VIDEO_EXTS:
                full = os.path.join(dirpath, f)
                tasks.append(asyncio.create_task(sem_task(full)))

    await asyncio.gather(*tasks)

def main(movies_dir: Optional[str] = None):
    if movies_dir is None:
        movies_dir = os.path.join(PROJECT_ROOT, 'movies')
    if not os.path.exists(movies_dir):
        print(f"Movies directory not found: {movies_dir}")
        return
    print(f"Scanning movies directory: {movies_dir}")
    try:
        asyncio.run(scan_and_import(movies_dir))
    except KeyboardInterrupt:
        print("Interrupted")

if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser(description='Scan movies folder and insert entries into DB')
    p.add_argument('path', nargs='?', help='Path to movies folder (defaults to ../movies)')
    args = p.parse_args()
    main(args.path)
