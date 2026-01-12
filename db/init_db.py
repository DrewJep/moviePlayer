# db/init_db.py
import asyncpg
import asyncio
from config import DB_CONFIG

async def init_db():
    conn = await asyncpg.connect(**DB_CONFIG)
    await conn.execute("""
        CREATE TABLE IF NOT EXISTS movies (
            id SERIAL PRIMARY KEY,
            title TEXT NOT NULL,
            year INT,
            imdb_id TEXT UNIQUE,
            genre TEXT,
            director TEXT,
            actors TEXT,
            plot TEXT,
            language TEXT,
            country TEXT,
            poster_url TEXT,
            runtime TEXT,
            rating REAL,
            watch_count INT DEFAULT 0,
            last_scraped TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            additional_info JSONB
        )
    """)
    await conn.close()

if __name__ == "__main__":
    asyncio.run(init_db())
