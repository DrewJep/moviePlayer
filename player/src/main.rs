use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::cell::RefCell;
use std::collections::HashMap;
use ratatui::{DefaultTerminal, Frame, widgets::{Block, Borders, List, ListItem, Paragraph}, layout::{Layout, Constraint}, style::{Style, Color, Modifier}, text::{Line, Span}};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use rand::Rng;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v"];

#[derive(Clone, Debug)]
struct MovieInfo {
    runtime: Option<String>,
    file_size: Option<String>,
    codec: Option<String>,
    resolution: Option<String>,
}

struct AppState {
    movies: Vec<PathBuf>,
    selected: usize,
    movie_info_cache: HashMap<PathBuf, MovieInfo>,
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

fn format_duration(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u64;
    let minutes = ((seconds % 3600.0) / 60.0) as u64;
    let secs = (seconds % 60.0) as u64;
    
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

fn get_movie_info(path: &Path) -> MovieInfo {
    // Try to get metadata using ffprobe
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration,size:stream=codec_name,width,height",
            "-of", "json",
            path.to_str().unwrap_or(""),
        ])
        .output();
    
    match output {
        Ok(output) if output.status.success() => {
            let json_str = String::from_utf8_lossy(&output.stdout);
            let mut runtime = None;
            let mut file_size = None;
            let mut codec = None;
            let mut resolution = None;
            
            // Parse JSON to extract information
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                // Get duration from format
                if let Some(format) = json.get("format") {
                    if let Some(duration_str) = format.get("duration")
                        .and_then(|d| d.as_str()) {
                        if let Ok(duration_secs) = duration_str.parse::<f64>() {
                            runtime = Some(format_duration(duration_secs));
                        }
                    }
                    if let Some(size_str) = format.get("size")
                        .and_then(|s| s.as_str()) {
                        if let Ok(size_bytes) = size_str.parse::<u64>() {
                            file_size = Some(format_file_size(size_bytes));
                        }
                    }
                }
                
                // Get codec and resolution from streams (usually first video stream)
                if let Some(streams) = json.get("streams")
                    .and_then(|s| s.as_array()) {
                    for stream in streams {
                        if stream.get("codec_type").and_then(|t| t.as_str()) == Some("video") {
                            if codec.is_none() {
                                if let Some(codec_name) = stream.get("codec_name")
                                    .and_then(|c| c.as_str()) {
                                    codec = Some(codec_name.to_string());
                                }
                            }
                            if resolution.is_none() {
                                if let (Some(w), Some(h)) = (
                                    stream.get("width").and_then(|w| w.as_u64()),
                                    stream.get("height").and_then(|h| h.as_u64()),
                                ) {
                                    resolution = Some(format!("{}x{}", w, h));
                                }
                            }
                            break;
                        }
                    }
                }
            }
            
            MovieInfo {
                runtime,
                file_size,
                codec,
                resolution,
            }
        }
        _ => {
            // Fallback: try to get file size at least
            let file_size = fs::metadata(path)
                .ok()
                .map(|m| format_file_size(m.len()));
            
            MovieInfo {
                runtime: None,
                file_size,
                codec: None,
                resolution: None,
            }
        }
    }
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
        movie_info_cache: HashMap::new(),
    };

    loop {
        terminal.draw(|frame| render(frame, &mut state))?;
        
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

fn render(frame: &mut Frame, state: &mut AppState) {
    // Split the frame into two areas: left for list, right for info
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(frame.area());
    
    let list_area = chunks[0];
    let info_area = chunks[1];
    
    // Render the movie list
    let mut items: Vec<ListItem> = state.movies
        .iter()
        .enumerate()
        .map(|(i, movie)| {
            let name = movie.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            let prefix = if i == state.selected { "> " } else { "  " };
            let item_text = format!("{}{}", prefix, name);
            
            // Style selected items with bright cyan, unselected with gray
            let style = if i == state.selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Gray)
            };
            
            ListItem::new(item_text).style(style)
        })
        .collect();
    
    // Add "Random Movie" option
    let random_prefix = if state.selected == state.movies.len() { "> " } else { "  " };
    let random_style = if state.selected == state.movies.len() {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Gray)
    };
    items.push(ListItem::new(format!("{}Random Movie", random_prefix)).style(random_style));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue))
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .title("Select a Movie")
        );

    frame.render_widget(list, list_area);
    
    // Render the info panel
    let info_lines: Vec<Line> = if state.selected < state.movies.len() {
        let movie = &state.movies[state.selected];
        
        // Get or cache movie info
        let movie_info = state.movie_info_cache.entry(movie.clone())
            .or_insert_with(|| get_movie_info(movie));
        
        let movie_name = movie.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");
        
        let mut lines = vec![
            Line::from(vec![
                Span::styled("File: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(movie_name, Style::default().fg(Color::White)),
            ]),
            Line::from(""),
        ];
        
        if let Some(ref runtime) = movie_info.runtime {
            lines.push(Line::from(vec![
                Span::styled("Runtime: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(runtime, Style::default().fg(Color::White)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Runtime: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("Unknown", Style::default().fg(Color::DarkGray)),
            ]));
        }
        
        if let Some(ref file_size) = movie_info.file_size {
            lines.push(Line::from(vec![
                Span::styled("File Size: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(file_size, Style::default().fg(Color::White)),
            ]));
        }
        
        if let Some(ref codec) = movie_info.codec {
            lines.push(Line::from(vec![
                Span::styled("Codec: ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(codec, Style::default().fg(Color::White)),
            ]));
        }
        
        if let Some(ref resolution) = movie_info.resolution {
            lines.push(Line::from(vec![
                Span::styled("Resolution: ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(resolution, Style::default().fg(Color::White)),
            ]));
        }
        
        lines
    } else {
        vec![Line::from(vec![
            Span::styled("Select a movie to see details", Style::default().fg(Color::DarkGray)),
        ])]
    };
    
    let info_paragraph = Paragraph::new(info_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta))
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .title("Movie Info")
        );
    
    frame.render_widget(info_paragraph, info_area);
}