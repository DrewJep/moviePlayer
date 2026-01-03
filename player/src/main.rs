use std::fs;
use std::path::Path;
use std::process::Command;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v"];

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn main() -> std::io::Result<()> {
    let movies_dir = Path::new("movies");

    let mut movies: Vec<_> = fs::read_dir(movies_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_video(p))
        .collect();

    movies.sort();

    if movies.is_empty() {
        eprintln!("No movies found in movies/");
        return Ok(());
    }

    println!("Found {} movies", movies.len());

    loop {
        for movie in &movies {
            println!("Playing {}", movie.display());

            let status = Command::new("mpv")
                .args([
                    "--fullscreen",
                    "--no-terminal",
                    movie.to_str().unwrap(),
                ])
                .status()
                .expect("failed to start mpv");

            if !status.success() {
                eprintln!("mpv exited with error");
            }
        }
    }
}
