import asyncpg
import os
import json
from dotenv import load_dotenv

load_dotenv()

DB_CONFIG = {
    "user": os.environ.get("DB_USER"),
    "password": os.environ.get("DB_PASSWORD"),
    "database": os.environ.get("DB_NAME"),
    "host": os.environ.get("DB_HOST", "127.0.0.1"),
    "port": int(os.environ.get("DB_PORT", 5432))
}

async def insert_movie(movie: dict):
    conn = await asyncpg.connect(**DB_CONFIG)
    await conn.execute("""
        INSERT INTO movies(title, year, imdb_id, genre, director, actors, plot, language, country, poster_url, runtime, rating, additional_info)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        ON CONFLICT (imdb_id) DO NOTHING
    """, movie.get("title"), movie.get("year"), movie.get("imdb_id"), movie.get("genre"),
         movie.get("director"), movie.get("actors"), movie.get("plot"), movie.get("language"),
         movie.get("country"), movie.get("poster_url"), movie.get("runtime"), movie.get("rating"),
         movie.get("additional_info"))
    await conn.close()

async def get_all_movies(limit=50):
    conn = await asyncpg.connect(**DB_CONFIG)
    rows = await conn.fetch(f"SELECT * FROM movies ORDER BY title LIMIT {limit}")
    await conn.close()
    return [dict(row) for row in rows]

async def get_movies_by_title(title: str):
    conn = await asyncpg.connect(**DB_CONFIG)
    rows = await conn.fetch("SELECT * FROM movies WHERE title ILIKE $1 ORDER BY title", f"%{title}%")
    await conn.close()
    return [dict(row) for row in rows]

async def get_movie_by_imdb(imdb_id: str):
    conn = await asyncpg.connect(**DB_CONFIG)
    row = await conn.fetchrow("SELECT * FROM movies WHERE imdb_id=$1", imdb_id)
    await conn.close()
    return dict(row) if row else None
