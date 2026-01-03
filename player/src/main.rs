use std::fs;
use std::path::Path;
use std::process::Command;
use ratatui::{DefaultTerminal, Frame};

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v"];

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn play_movies() -> std::io::Result<()> {
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

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(app)?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    loop {
        terminal.draw(render)?;
        if crossterm::event::read()?.is_key_press() {
            // break Ok(());
            play_movies();
        }
    }
}

fn render(frame: &mut Frame) {
    frame.render_widget("hello world", frame.area());
}