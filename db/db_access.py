# db/db_access.py

import asyncpg
import asyncio
from config import DB_CONFIG
from typing import Optional, Dict, Any

# ---------------------------
# Helper function to get a connection
# ---------------------------
async def get_conn():
    return await asyncpg.connect(**DB_CONFIG)


# ---------------------------
# Fetch a movie by title
# ---------------------------
async def get_movie_by_title(title: str) -> Optional[Dict[str, Any]]:
    conn = await get_conn()
    row = await conn.fetchrow(
        "SELECT * FROM movies WHERE title = $1",
        title
    )
    await conn.close()
    return dict(row) if row else None


# ---------------------------
# Fetch a movie by IMDb ID
# ---------------------------
async def get_movie_by_imdb(imdb_id: str) -> Optional[Dict[str, Any]]:
    conn = await get_conn()
    row = await conn.fetchrow(
        "SELECT * FROM movies WHERE imdb_id = $1",
        imdb_id
    )
    await conn.close()
    return dict(row) if row else None


# ---------------------------
# Insert a new movie
# movie_data should be a dict with keys:
# title, year, imdb_id, genre, director, actors,
# plot, language, country, poster_url, runtime,
# rating, additional_info (JSON/dict)
# ---------------------------
async def insert_movie(movie_data: Dict[str, Any]) -> None:
    conn = await get_conn()
    await conn.execute("""
        INSERT INTO movies (
            title, year, imdb_id, genre, director, actors,
            plot, language, country, poster_url, runtime,
            rating, additional_info
        ) VALUES (
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13
        )
        ON CONFLICT (imdb_id) DO NOTHING
    """,
    movie_data.get("title"),
    movie_data.get("year"),
    movie_data.get("imdb_id"),
    movie_data.get("genre"),
    movie_data.get("director"),
    movie_data.get("actors"),
    movie_data.get("plot"),
    movie_data.get("language"),
    movie_data.get("country"),
    movie_data.get("poster_url"),
    movie_data.get("runtime"),
    movie_data.get("rating"),
    movie_data.get("additional_info")
    )
    await conn.close()


# ---------------------------
# Increment watch count
# ---------------------------
async def increment_watch_count(imdb_id: str) -> None:
    conn = await get_conn()
    await conn.execute("""
        UPDATE movies
        SET watch_count = watch_count + 1
        WHERE imdb_id = $1
    """, imdb_id)
    await conn.close()


# ---------------------------
# Example usage for testing
# ---------------------------
if __name__ == "__main__":
    async def test():
        # Example: fetch a movie
        movie = await get_movie_by_title("Inception")
        print("Fetched:", movie)

        # Example: insert a movie
        await insert_movie({
            "title": "Test Movie",
            "year": 2026,
            "imdb_id": "tt1234567",
            "genre": "Action",
            "director": "John Doe",
            "actors": "Actor A, Actor B",
            "plot": "Some plot here",
            "language": "English",
            "country": "USA",
            "poster_url": "",
            "runtime": "120 min",
            "rating": 8.5,
            "additional_info": {"source": "OMDB"}
        })

        # Increment watch count
        await increment_watch_count("tt1234567")

    asyncio.run(test())
