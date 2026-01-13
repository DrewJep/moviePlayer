use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;

use reqwest::blocking::Client as HttpClient;
use serde_json::Value as JsonValue;
use std::time::{Instant, Duration};
use ratatui::{DefaultTerminal, Frame, 
            widgets::{Block, Borders, List, ListItem, Paragraph, Wrap, Clear}, 
            layout::{Layout, Constraint, Flex, Rect, Position}, 
            style::{Style, Color, Modifier}, 
            text::{Line, Span}};
use crossterm::event::{Event, KeyCode, KeyEventKind, poll};
use rand::Rng;
use rand::seq::SliceRandom;
use std::sync::atomic::{AtomicBool, Ordering};

static AUTO_PLAY_NEXT: AtomicBool = AtomicBool::new(true);
static SHUFFLE_QUEUE: AtomicBool = AtomicBool::new(false);


const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v"];

#[derive(Clone, Debug, Default)]
struct MovieInfo {
    // Fields pulled from the movies DB
    title: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
    director: Option<String>,
    plot: Option<String>,
    runtime: Option<String>,
    rating: Option<f64>,
    watch_count: Option<i32>,
    _imdb_id: Option<String>,

    // Fallback file-level metadata (kept for compatibility)
    file_size: Option<String>,
    codec: Option<String>,
    resolution: Option<String>,
}

#[derive(Clone)]
struct MovieEntry {
    path: PathBuf,
    group_name: String,
}

enum InputMode {
    Normal,
    #[allow(dead_code)]
    Editing,
}

struct AppState {
    movies: Vec<MovieEntry>,
    selected: usize,
    movie_info_cache: HashMap<PathBuf, MovieInfo>,
    scroll_offset: usize,
    show_popup: bool,
    user_input: String,
    #[allow(dead_code)]
    input_mode: InputMode,
    character_index: usize,
}

fn toggle_auto_play_next() {
    AUTO_PLAY_NEXT.fetch_xor(true, Ordering::SeqCst);
}

fn check_auto_play_next() -> bool {
    AUTO_PLAY_NEXT.load(Ordering::SeqCst)
}

fn toggle_shuffle_queue() {
    SHUFFLE_QUEUE.fetch_xor(true, Ordering::SeqCst);
}

fn check_shuffle_queue() -> bool {
    SHUFFLE_QUEUE.load(Ordering::SeqCst)
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn load_movies() -> std::io::Result<(Vec<MovieEntry>, HashMap<PathBuf, MovieInfo>)> {
    let movies_dir = Path::new("../movies");

    // Recursively collect all video files
    let mut movies: Vec<MovieEntry> = Vec::new();
    
    fn collect_movies(dir: &Path, base_dir: &Path, movies: &mut Vec<MovieEntry>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_file() && is_video(&path) {
                    // Skip AppleDouble metadata files that start with "._"
                    if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                        if fname.starts_with("._") {
                            continue;
                        }
                    }
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
    
    // Try to fetch all movies from the FastAPI `/movies/` endpoint and map file keys/paths to metadata.
    let mut info_map: HashMap<PathBuf, MovieInfo> = HashMap::new();
    let api_base = env::var("API_URL").unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let client = HttpClient::new();
    let movies_url = format!("{}/movies/?limit=1000", api_base.trim_end_matches('/'));

    match client.get(&movies_url).send() {
        Ok(resp) => match resp.json::<Vec<JsonValue>>() {
            Ok(api_movies) => {
                // Build a map: file_path_or_key -> movie JSON value
                let mut by_path: HashMap<String, &JsonValue> = HashMap::new();
                for mv in &api_movies {
                    if let Some(fk) = mv.get("file_key").and_then(|v| v.as_str()) {
                        by_path.insert(fk.to_string(), mv);
                    }
                    if let Some(paths) = mv.get("file_paths").and_then(|v| v.as_array()) {
                        for p in paths {
                            if let Some(pstr) = p.as_str() {
                                by_path.insert(pstr.to_string(), mv);
                            }
                        }
                    }
                }

                // For each local file, attempt to find matching metadata
                for movie in &result {
                    let rel = movie.path.strip_prefix(movies_dir)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| movie.path.to_string_lossy().to_string());
                    let candidates = vec![format!("movies/{}", rel), rel.clone(), format!("./movies/{}", rel)];
                    let mut found: Option<&JsonValue> = None;
                    for c in &candidates {
                        if let Some(mv) = by_path.get(c) {
                            found = Some(*mv);
                            break;
                        }
                    }
                    if let Some(mv) = found {
                        let info = MovieInfo {
                            title: mv.get("title").and_then(|v| v.as_str().map(|s| s.to_string())),
                            year: mv.get("year").and_then(|v| v.as_i64().map(|n| n as i32)),
                            genre: mv.get("genre").and_then(|v| v.as_str().map(|s| s.to_string())),
                            director: mv.get("director").and_then(|v| v.as_str().map(|s| s.to_string())),
                            plot: mv.get("plot").and_then(|v| v.as_str().map(|s| s.to_string())),
                            runtime: mv.get("runtime").and_then(|v| v.as_str().map(|s| s.to_string())),
                            rating: mv.get("rating").and_then(|v| v.as_f64()),
                            watch_count: mv.get("watch_count").and_then(|v| v.as_i64().map(|n| n as i32)),
                            _imdb_id: mv.get("imdb_id").and_then(|v| v.as_str().map(|s| s.to_string())),
                            file_size: None,
                            codec: None,
                            resolution: None,
                        };
                        info_map.insert(movie.path.clone(), info);
                    } else {
                        eprintln!("API: no metadata for file; tried keys: {}", candidates.join(" | "));
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to parse /movies/ JSON: {}", e);
            }
        },
        Err(e) => {
            eprintln!("Failed to call API {}: {}", movies_url, e);
        }
    }

    Ok((result, info_map))
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
                title: None,
                year: None,
                genre: None,
                director: None,
                plot: None,
                runtime,
                rating: None,
                watch_count: None,
                file_size,
                codec,
                resolution,
                _imdb_id: None,
            }
        }
        _ => {
            // Fallback: try to get file size at least
            let file_size = fs::metadata(path)
                .ok()
                .map(|m| format_file_size(m.len()));
            
            MovieInfo {
                title: None,
                year: None,
                genre: None,
                director: None,
                plot: None,
                runtime: None,
                rating: None,
                watch_count: None,
                file_size,
                codec: None,
                resolution: None,
                _imdb_id: None,
            }
        }
    }
}

fn play_movies_from_index(movies: &[MovieEntry], start_index: usize, shuffle_order: bool) -> std::io::Result<()> {
    if movies.is_empty() {
        return Ok(());
    }

    // If shuffle_order is true, preserve the selected movie as first and shuffle the rest.
    let movies_to_play: Vec<MovieEntry> = if shuffle_order {
        if start_index >= movies.len() {
            // Fallback: shuffle everything if the start index is out of bounds
            let mut shuffled: Vec<MovieEntry> = movies.to_vec();
            let mut rng = rand::thread_rng();
            shuffled.shuffle(&mut rng);
            shuffled
        } else {
            let mut rng = rand::thread_rng();
            // Collect indices of all movies except the selected one
            let mut other_idxs: Vec<usize> = (0..movies.len()).filter(|&i| i != start_index).collect();
            other_idxs.shuffle(&mut rng);

            // Start with the selected movie, then append the shuffled others
            let mut ordered: Vec<MovieEntry> = Vec::with_capacity(movies.len());
            ordered.push(movies[start_index].clone());
            for i in other_idxs {
                ordered.push(movies[i].clone());
            }
            ordered
        }
    } else {
        // For normal playback, keep original order but rotate to start_index
        let mut rotated = movies[start_index..].to_vec();
        rotated.extend_from_slice(&movies[..start_index]);
        rotated
    };

    // Play movies in order (either shuffled or rotated)
    for movie in movies_to_play {
        println!("Playing {}", movie.path.display());

        // Increment watch count via API if available
        let api_base = env::var("API_URL").unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
        let http = HttpClient::new();
        // compute relative key variants similar to load_movies
        let movies_dir = Path::new("../movies");
        let rel = movie.path.strip_prefix(movies_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| movie.path.to_string_lossy().to_string());
        let candidates = vec![format!("movies/{}", rel), rel.clone(), format!("./movies/{}", rel)];
        // Try incrementing by imdb_id from cached info if present
        if std::env::var("API_URL").is_ok() {
            // prefer imdb_id if the movie_info cache has it
            // (we don't have access to the cache here; attempt by path)
            let endpoint = format!("{}/movies/increment_watch/", api_base.trim_end_matches('/'));
            for c in &candidates {
                let _ = http.post(&endpoint).query(&[("path", c)]).send();
            }
        }

        let status = Command::new("mpv")
            .args([
                "--fullscreen",
                "--no-terminal",
                "--no-sub",
                // "--sub-auto=no",
                // "--sid=-1",
                movie.path.to_str().unwrap(),
            ])
            .status()
            .expect("failed to start mpv");

        let exit_code = status.code().unwrap_or(1);
        
        if exit_code != 0 {
            return Ok(());
        }
        if !check_auto_play_next() {
            return Ok(());
        }
    }
    
    Ok(())
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
/// Gotten from ratatui examples
fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

impl AppState {
    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    fn enter_char(&mut self, new_char: char) {
        self.user_input.insert(self.character_index, new_char);
        self.move_cursor_right();
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.user_input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.user_input.chars().skip(current_index);

            self.user_input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.user_input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn clear_input(&mut self) {
        self.user_input.clear();
        self.reset_cursor();
    }
}



fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    
    let (movies, movie_info_cache) = load_movies()?;
    if movies.is_empty() {
        eprintln!("No movies found in movies/");
        return Ok(());
    }
    
    let selected_index = RefCell::new(None);
    let shuffle_queue = &SHUFFLE_QUEUE;
    let should_exit = RefCell::new(false);

    loop {
        let info_map_ref = &movie_info_cache;
        ratatui::run(|terminal| app(terminal, &movies, info_map_ref, &selected_index, shuffle_queue, &should_exit))?;

        // If the UI signaled to exit (Esc pressed), break the main loop and quit
        if *should_exit.borrow() {
            break;
        }

        let start_index = selected_index.borrow_mut().take();
        let shuffle = shuffle_queue.load(Ordering::SeqCst);

        if let Some(start_index) = start_index {
            play_movies_from_index(&movies, start_index, shuffle)?;
        }
    }
    
    Ok(())
}

fn app(terminal: &mut DefaultTerminal, movies: &[MovieEntry], movie_info_map: &HashMap<PathBuf, MovieInfo>, selected_index: &RefCell<Option<usize>>, shuffle_queue: &AtomicBool, should_exit: &RefCell<bool>) -> std::io::Result<()> {
    let mut state = AppState {
        movies: movies.to_vec(),
        selected: 0,
        movie_info_cache: movie_info_map.clone(),
        scroll_offset: 0,
        show_popup: false,
        user_input: String::new(),
        input_mode: InputMode::Normal,
        character_index: 0,
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
            shuffle_queue.store(true, Ordering::SeqCst);
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

                // Reset the timer on any user input
                last_input_time = Instant::now();

                // Handle text input when popup is open
                if state.show_popup {
                    match key.code {
                        KeyCode::Esc => {
                            // Close the popup without exiting the app
                            state.show_popup = false;
                            state.clear_input();
                        }
                        KeyCode::Char(c) => {
                            state.enter_char(c);
                        }
                        KeyCode::Backspace => {
                            state.delete_char();
                        }
                        KeyCode::Left => {
                            state.move_cursor_left();
                        }
                        KeyCode::Right => {
                            state.move_cursor_right();
                        }
                        KeyCode::Home => {
                            state.reset_cursor();
                        }
                        KeyCode::End => {
                            state.character_index = state.user_input.chars().count();
                        }
                        _ => {}
                    }
                } else {
                    // Handle normal navigation when popup is closed
                    match key.code {
                        KeyCode::Esc => {
                            // Exit the app when popup is not open
                            *should_exit.borrow_mut() = true;
                            return Ok(());
                        }
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
                            } else if SHUFFLE_QUEUE.load(Ordering::SeqCst) {
                                // Selected movie - shuffle order
                                (state.selected, true)
                            } else {
                                // Selected movie - keep original order
                                (state.selected, false)
                            };
                            
                            *selected_index.borrow_mut() = Some(start_index);
                            shuffle_queue.store(should_shuffle, Ordering::SeqCst);
                            return Ok(());
                        }
                        KeyCode::Char('n') => {
                            toggle_auto_play_next();
                        }
                        KeyCode::Char('s') => {
                            toggle_shuffle_queue();
                        }
                        KeyCode::Char(' ') => {
                            state.show_popup = !state.show_popup;
                        }
                        _ => {}
                    }
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
    let taskbar_text = format!("{} | {} | {} | Enter=Play | Esc=Exit | ↑↓=Navigate | Autoplay Next (n)={} | Shuffle (s)={}", 
        time_str, date_str, timer_str, check_auto_play_next().to_string(), check_shuffle_queue().to_string());
    
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
        
        // Get or cache movie info (DB-backed). If not present, fallback to file probe
        let movie_info = state.movie_info_cache.entry(movie.path.clone()).or_insert_with(|| get_movie_info(&movie.path));

        // Prefer DB title if present; otherwise show filename
        let title = movie_info.title.clone().or_else(|| movie.path.file_stem().and_then(|s| s.to_str().map(|s| s.to_string()))).unwrap_or_else(|| "Unknown".to_string());

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Title: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(title, Style::default().fg(Color::White)),
        ]));

        // Year
        if let Some(y) = movie_info.year {
            lines.push(Line::from(vec![
                Span::styled("Year: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(y.to_string(), Style::default().fg(Color::White)),
            ]));
        }

        // Genre
        if let Some(ref g) = movie_info.genre {
            lines.push(Line::from(vec![
                Span::styled("Genre: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(g.clone(), Style::default().fg(Color::White)),
            ]));
        }

        // Director
        if let Some(ref d) = movie_info.director {
            lines.push(Line::from(vec![
                Span::styled("Director: ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(d.clone(), Style::default().fg(Color::White)),
            ]));
        }

        // Runtime (DB runtime preferred, else file probe runtime)
        if let Some(ref rtime) = movie_info.runtime {
            lines.push(Line::from(vec![
                Span::styled("Runtime: ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(rtime.clone(), Style::default().fg(Color::White)),
            ]));
        }

        // Rating
        if let Some(r) = movie_info.rating {
            lines.push(Line::from(vec![
                Span::styled("Rating: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:.1}", r), Style::default().fg(Color::White)),
            ]));
        }

        // Watch count
        if let Some(wc) = movie_info.watch_count {
            lines.push(Line::from(vec![
                Span::styled("Watch Count: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(wc.to_string(), Style::default().fg(Color::White)),
            ]));
        }

        // Plot (wrap as single paragraph line)
        if let Some(ref ptxt) = movie_info.plot {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Plot: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(ptxt.clone(), Style::default().fg(Color::White)),
            ]));
        }

        // File-level metadata fallbacks: file size, codec, resolution
        if let Some(ref fsz) = movie_info.file_size {
            lines.push(Line::from(vec![
                Span::styled("File Size: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(fsz.clone(), Style::default().fg(Color::White)),
            ]));
        }
        if let Some(ref c) = movie_info.codec {
            lines.push(Line::from(vec![
                Span::styled("Codec: ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(c.clone(), Style::default().fg(Color::White)),
            ]));
        }
        if let Some(ref res) = movie_info.resolution {
            lines.push(Line::from(vec![
                Span::styled("Resolution: ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(res.clone(), Style::default().fg(Color::White)),
            ]));
        }

        lines
    } else {
        vec![Line::from(vec![
            Span::styled("Select a movie to see details", Style::default().fg(Color::DarkGray)),
        ])]
    };
    
    let info_paragraph = Paragraph::new(info_lines)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta))
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .title("Movie Info")
        );
    
    frame.render_widget(info_paragraph, info_area);

    // Render Search Bar Popup
    if state.show_popup {
        let area = popup_area(frame.area(), 20, 10);
        frame.render_widget(Clear, area); // Clear the background
        
        // Create the input display with cursor
        let input_display = format!("{}_", state.user_input);
        let cursor_position = state.character_index;
        
        let input_paragraph = Paragraph::new(input_display)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title("Search | Press ESC to exit")
            );
        
        frame.render_widget(input_paragraph, area);
        
        // Set the cursor position for the terminal
        frame.set_cursor_position(Position {
            x: area.x + cursor_position as u16 + 1,
            y: area.y + 1,
        });
    }
}