# MUSICPLAYER MONOREPO

This repository contains a multi-component music and movie player project. It includes:

Rust TUI frontend (player/) – terminal-based user interface.

Python scraper + FastAPI backend (scraper/) – interacts with OMDB API and a local PostgreSQL database.

Database layer (db/) – PostgreSQL tables and Python scripts for database access.

## FOLDER STRUCTURE

musicPlayer/

player/ # Rust TUI

scraper/ # Python backend + FastAPI

venv/ # Python virtual environment

db/ # Database scripts and config

config.py # DB connection settings

init_db.py # Initialize movies table

db_access.py # Helper functions to query/update DB

source venv/bin/activate