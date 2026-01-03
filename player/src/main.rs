use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::cell::RefCell;
use ratatui::{DefaultTerminal, Frame, widgets::{Block, Borders, List, ListItem}};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use rand::Rng;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v"];

struct AppState {
    movies: Vec<PathBuf>,
    selected: usize,
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn load_movies() -> std::io::Result<Vec<PathBuf>> {
    let movies_dir = Path::new("movies");

    let mut movies: Vec<_> = fs::read_dir(movies_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_video(p))
        .collect();

    movies.sort();
    Ok(movies)
}

fn play_movies_from_index(movies: &[PathBuf], start_index: usize) -> std::io::Result<()> {
    if movies.is_empty() {
        return Ok(());
    }

    // Play movies starting from start_index, then wrap around
    let mut current_index = start_index;
    
    loop {
        let movie = &movies[current_index];
        println!("Playing {}", movie.display());

        let status = Command::new("mpv")
            .args([
                "--fullscreen",
                "--no-terminal",
                movie.to_str().unwrap(),
            ])
            .status()
            .expect("failed to start mpv");

        let exit_code = status.code().unwrap_or(1);
        
        if exit_code != 0 {
            return Ok(());
        }
        
        current_index = (current_index + 1) % movies.len();
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    
    let movies = load_movies()?;
    if movies.is_empty() {
        eprintln!("No movies found in movies/");
        return Ok(());
    }
    
    let selected_index = RefCell::new(None);
    ratatui::run(|terminal| app(terminal, &movies, &selected_index))?;
    
    // After terminal is restored, play the selected movie
    if let Some(start_index) = selected_index.into_inner() {
        play_movies_from_index(&movies, start_index)?;
    }
    
    Ok(())
}

fn app(terminal: &mut DefaultTerminal, movies: &[PathBuf], selected_index: &RefCell<Option<usize>>) -> std::io::Result<()> {
    let mut state = AppState {
        movies: movies.to_vec(),
        selected: 0,
    };

    loop {
        terminal.draw(|frame| render(frame, &state))?;
        
        if let Event::Key(key) = crossterm::event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Up => {
                    if state.selected > 0 {
                        state.selected -= 1;
                    } else {
                        state.selected = state.movies.len(); // Wrap to "Random Movie"
                    }
                }
                KeyCode::Down => {
                    if state.selected < state.movies.len() {
                        state.selected += 1;
                    } else {
                        state.selected = 0; // Wrap to first movie
                    }
                }
                KeyCode::Enter => {
                    // Store the selected index and exit to restore terminal
                    let start_index = if state.selected == state.movies.len() {
                        // Random movie selected
                        rand::thread_rng().gen_range(0..state.movies.len())
                    } else {
                        // Selected movie
                        state.selected
                    };
                    
                    *selected_index.borrow_mut() = Some(start_index);
                    return Ok(());
                }
                KeyCode::Esc => {
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

fn render(frame: &mut Frame, state: &AppState) {
    let mut items: Vec<ListItem> = state.movies
        .iter()
        .enumerate()
        .map(|(i, movie)| {
            let name = movie.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            let prefix = if i == state.selected { "> " } else { "  " };
            ListItem::new(format!("{}{}", prefix, name))
        })
        .collect();
    
    // Add "Random Movie" option
    let random_prefix = if state.selected == state.movies.len() { "> " } else { "  " };
    items.push(ListItem::new(format!("{}Random Movie", random_prefix)));

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Select a Movie"));

    frame.render_widget(list, frame.area());
}