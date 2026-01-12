# db/config.py
import os
from pathlib import Path


def _load_dotenv(dotenv_path: Path | str | None = None) -> None:
    """Load a simple .env file into environment variables.

    This does not require external dependencies. It sets variables only
    if they are not already present in the environment.
    """
    if dotenv_path is None:
        dotenv_path = Path(__file__).resolve().parents[1] / '.env'
    dotenv_path = Path(dotenv_path)
    if not dotenv_path.exists():
        return
    try:
        with open(dotenv_path, 'r') as fh:
            for raw in fh:
                line = raw.strip()
                if not line or line.startswith('#'):
                    continue
                if '=' not in line:
                    continue
                key, val = line.split('=', 1)
                key = key.strip()
                val = val.strip().strip('"').strip("'")
                os.environ.setdefault(key, val)
    except Exception:
        # Silently ignore errors reading .env to keep startup robust
        pass


# Load `.env` from project root (one level up from db/)
_load_dotenv()


DB_CONFIG = {
    "user": os.environ.get("DB_USER", "drewj"),
    "password": os.environ.get("DB_PASSWORD", ""),
    "database": os.environ.get("DB_NAME", "movies_db"),
    "host": os.environ.get("DB_HOST", "127.0.0.1"),
    "port": int(os.environ.get("DB_PORT", 5432)),
}
