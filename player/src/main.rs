use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use ratatui::{DefaultTerminal, Frame, widgets::{Block, Borders, List, ListItem, Paragraph}, layout::{Layout, Constraint}, style::{Style, Color, Modifier}, text::{Line, Span}};
use crossterm::event::{Event, KeyCode, KeyEventKind, poll};
use rand::Rng;
use rand::seq::SliceRandom;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v"];

#[derive(Clone, Debug)]
struct MovieInfo {
    runtime: Option<String>,
    file_size: Option<String>,
    codec: Option<String>,
    resolution: Option<String>,
}

#[derive(Clone)]
struct MovieEntry {
    path: PathBuf,
    group_name: String,
}

struct AppState {
    movies: Vec<MovieEntry>,
    selected: usize,
    movie_info_cache: HashMap<PathBuf, MovieInfo>,
    scroll_offset: usize,
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn load_movies() -> std::io::Result<Vec<MovieEntry>> {
    let movies_dir = Path::new("movies");

    // Recursively collect all video files
    let mut movies: Vec<MovieEntry> = Vec::new();
    
    fn collect_movies(dir: &Path, base_dir: &Path, movies: &mut Vec<MovieEntry>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_file() && is_video(&path) {
                    // Get the parent directory name relative to the base movies directory
                    let group_name = if let Some(parent) = path.parent() {
                        if parent == base_dir {
                            "Root".to_string()
                        } else {
                            parent.file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "Root".to_string())
                        }
                    } else {
                        "Root".to_string()
                    };
                    
                    movies.push(MovieEntry {
                        path,
                        group_name,
                    });
                } else if path.is_dir() {
                    // Recursively search subdirectories
                    collect_movies(&path, base_dir, movies)?;
                }
            }
        }
        Ok(())
    }
    
    collect_movies(movies_dir, movies_dir, &mut movies)?;
    
    // Group movies by group_name, then sort within groups
    let mut groups: HashMap<String, Vec<MovieEntry>> = HashMap::new();
    for movie in movies {
        groups.entry(movie.group_name.clone())
            .or_insert_with(Vec::new)
            .push(movie);
    }
    
    // Sort groups (but put "Root" first), then sort movies within each group
    let mut group_names: Vec<String> = groups.keys().cloned().collect();
    group_names.sort();
    if let Some(root_idx) = group_names.iter().position(|n| n == "Root") {
        group_names.remove(root_idx);
        group_names.insert(0, "Root".to_string());
    }
    
    let mut result: Vec<MovieEntry> = Vec::new();
    for group_name in group_names {
        let mut group_movies = groups.remove(&group_name).unwrap();
        group_movies.sort_by(|a, b| {
            a.path.file_name()
                .and_then(|n| n.to_str())
                .cmp(&b.path.file_name().and_then(|n| n.to_str()))
        });
        result.extend(group_movies);
    }
    
    Ok(result)
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

fn play_movies_from_index(movies: &[MovieEntry], start_index: usize, shuffle_order: bool) -> std::io::Result<()> {
    if movies.is_empty() {
        return Ok(());
    }

    // If shuffle_order is true, create a shuffled copy of the movies
    let movies_to_play: Vec<MovieEntry> = if shuffle_order {
        let mut shuffled: Vec<MovieEntry> = movies.to_vec();
        let mut rng = rand::thread_rng();
        shuffled.shuffle(&mut rng);
        shuffled
    } else {
        // For normal playback, keep original order but rotate to start_index
        let mut rotated = movies[start_index..].to_vec();
        rotated.extend_from_slice(&movies[..start_index]);
        rotated
    };

    // Play movies in order (either shuffled or rotated)
    for movie in movies_to_play {
        println!("Playing {}", movie.path.display());

        let status = Command::new("mpv")
            .args([
                "--fullscreen",
                "--no-terminal",
                movie.path.to_str().unwrap(),
            ])
            .status()
            .expect("failed to start mpv");

        let exit_code = status.code().unwrap_or(1);
        
        if exit_code != 0 {
            return Ok(());
        }
    }
    
    Ok(())
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    
    let movies = load_movies()?;
    if movies.is_empty() {
        eprintln!("No movies found in movies/");
        return Ok(());
    }
    
    let selected_index = RefCell::new(None);
    let shuffle_queue = RefCell::new(false);
    ratatui::run(|terminal| app(terminal, &movies, &selected_index, &shuffle_queue))?;
    
    // After terminal is restored, play the selected movie
    if let Some(start_index) = selected_index.into_inner() {
        play_movies_from_index(&movies, start_index, shuffle_queue.into_inner())?;
    }
    
    Ok(())
}

fn app(terminal: &mut DefaultTerminal, movies: &[MovieEntry], selected_index: &RefCell<Option<usize>>, shuffle_queue: &RefCell<bool>) -> std::io::Result<()> {
    let mut state = AppState {
        movies: movies.to_vec(),
        selected: 0,
        movie_info_cache: HashMap::new(),
        scroll_offset: 0,
    };

    let mut last_input_time = Instant::now();
    const TIMEOUT_SECONDS: u64 = 30;

    loop {
        let elapsed = last_input_time.elapsed();
        terminal.draw(|frame| render(frame, &mut state, elapsed, TIMEOUT_SECONDS))?;
        
        // Check if 30 seconds have passed since last input
        if elapsed >= Duration::from_secs(TIMEOUT_SECONDS) {
            // Auto-select random movie and shuffle queue
            let random_index = rand::thread_rng().gen_range(0..state.movies.len());
            *selected_index.borrow_mut() = Some(random_index);
            *shuffle_queue.borrow_mut() = true;
            return Ok(());
        }
        
        // Poll for events with a short timeout (100ms) to allow checking elapsed time
        let remaining_time = Duration::from_secs(TIMEOUT_SECONDS) - elapsed;
        let poll_timeout = remaining_time.min(Duration::from_millis(100));
        
        if poll(poll_timeout)? {
        if let Event::Key(key) = crossterm::event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

                // Handle Esc immediately (exit without resetting timer)
                if key.code == KeyCode::Esc {
                    return Ok(());
                }
                
                // Reset the timer on any other user input
                last_input_time = Instant::now();

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
                        let (start_index, should_shuffle) = if state.selected == state.movies.len() {
                            // Random movie selected - shuffle the queue
                            (rand::thread_rng().gen_range(0..state.movies.len()), true)
                        } else {
                            // Selected movie - keep original order
                            (state.selected, false)
                        };
                        
                        *selected_index.borrow_mut() = Some(start_index);
                        *shuffle_queue.borrow_mut() = should_shuffle;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn render(frame: &mut Frame, state: &mut AppState, elapsed: Duration, timeout_seconds: u64) {
    // Split the frame: top taskbar, then main content area
    let main_chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(frame.area());
    
    let taskbar_area = main_chunks[0];
    let content_area = main_chunks[1];
    
    // Split the content area into two: left for list, right for info
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(content_area);
    
    let list_area = chunks[0];
    let info_area = chunks[1];
    
    // Render the taskbar
    // Get current time and date using chrono
    let now = chrono::Local::now();
    let time_str = now.format("%H:%M:%S").to_string();
    let date_str = now.format("%Y-%m-%d").to_string();
    
    // Calculate remaining time until auto-play
    let remaining = Duration::from_secs(timeout_seconds).saturating_sub(elapsed);
    let remaining_secs = remaining.as_secs();
    let timer_str = format!("Auto-play in: {:02}s", remaining_secs);
    
    // Create taskbar content
    let taskbar_text = format!("{} | {} | {} | Enter=Play | Esc=Exit | ↑↓=Navigate", 
        time_str, date_str, timer_str);
    
    let taskbar = Paragraph::new(taskbar_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
        );
    
    frame.render_widget(taskbar, taskbar_area);
    
    // Build display list with group headers
    let mut items: Vec<ListItem> = Vec::new();
    let mut current_group: Option<&str> = None;
    let mut selected_display_index = 0; // Track where selected item appears in display list
    
    for (movie_idx, movie) in state.movies.iter().enumerate() {
        // Add group header if this is a new group
        if current_group != Some(movie.group_name.as_str()) {
            current_group = Some(movie.group_name.as_str());
            let header_text = format!("┌─ {} ─┐", movie.group_name);
            items.push(ListItem::new(header_text)
                .style(Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)));
        }
        
        // Add movie item
        let name = movie.path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
        let prefix = if movie_idx == state.selected { "> " } else { "  " };
        let item_text = format!("{}{}", prefix, name);
        
        // Style selected items with bright cyan, unselected with gray
        let style = if movie_idx == state.selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Gray)
        };
        
        items.push(ListItem::new(item_text).style(style));
        
        // Track display index for selected movie (after adding to list)
        if movie_idx == state.selected {
            selected_display_index = items.len() - 1;
        }
    }
    
    // Add separator and "Random Movie" option with its own group
    items.push(ListItem::new("┌─ Special ─┐")
        .style(Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)));
    
    let random_movie_idx = state.movies.len();
    if state.selected == random_movie_idx {
        selected_display_index = items.len();
    }
    
    let random_prefix = if state.selected == random_movie_idx { "> " } else { "  " };
    let random_style = if state.selected == random_movie_idx {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Gray)
    };
    items.push(ListItem::new(format!("{}Random Movie", random_prefix)).style(random_style));

    // Calculate visible area (accounting for borders - 2 lines for top/bottom borders)
    let visible_height = list_area.height.saturating_sub(2);
    
    // Update scroll offset to keep selected item visible
    if selected_display_index < state.scroll_offset {
        // Selected item is above visible area, scroll up
        state.scroll_offset = selected_display_index;
    } else if selected_display_index >= state.scroll_offset + visible_height as usize {
        // Selected item is below visible area, scroll down
        state.scroll_offset = selected_display_index.saturating_sub(visible_height as usize - 1);
    }
    
    // Ensure scroll offset doesn't go beyond bounds
    if state.scroll_offset + visible_height as usize > items.len() {
        state.scroll_offset = items.len().saturating_sub(visible_height as usize);
    }
    if state.scroll_offset > items.len() {
        state.scroll_offset = 0;
    }
    
    // Get visible slice of items
    let end_index = (state.scroll_offset + visible_height as usize).min(items.len());
    let visible_items: Vec<ListItem> = items[state.scroll_offset..end_index].to_vec();

    let list = List::new(visible_items)
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
        let movie_info = state.movie_info_cache.entry(movie.path.clone())
            .or_insert_with(|| get_movie_info(&movie.path));
        
        let movie_name = movie.path.file_name()
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