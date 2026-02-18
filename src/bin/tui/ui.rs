use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
};

use crate::app::{App, DownloadStatus, Focus, InputMode};

const ACCENT: Color = Color::Rgb(138, 180, 248);
const DIM: Color = Color::Rgb(128, 128, 128);
const SURFACE: Color = Color::Rgb(30, 30, 30);
const SURFACE_BRIGHT: Color = Color::Rgb(45, 45, 45);
const SUCCESS: Color = Color::Rgb(129, 199, 132);
const WARNING: Color = Color::Rgb(255, 183, 77);
const TEXT: Color = Color::Rgb(230, 230, 230);
const TEXT_DIM: Color = Color::Rgb(160, 160, 160);

pub fn draw(f: &mut Frame, app: &App) {
    f.render_widget(
        Block::default().style(Style::default().bg(SURFACE)),
        f.area(),
    );

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, app, outer[0]);

    let main_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(outer[1])[1];

    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(6)])
        .split(main_area);

    draw_search_bar(f, app, content[0]);

    let has_files = app.current_search_files.is_some() || app.current_user_files.is_some();
    let has_downloads = !app.downloads.is_empty();
    let has_playlist = app.spotify_playlist.is_some();

    if has_playlist {
        if has_downloads {
            let panels = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(content[1]);
            draw_playlist(f, app, panels[0]);
            draw_downloads(f, app, panels[1]);
        } else {
            draw_playlist(f, app, content[1]);
        }
    } else if has_files && has_downloads {
        let panels = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(50),
                Constraint::Percentage(25),
            ])
            .split(content[1]);

        draw_results(f, app, panels[0]);
        draw_files(f, app, panels[1]);
        draw_downloads(f, app, panels[2]);
    } else if has_files {
        let panels = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(content[1]);

        draw_results(f, app, panels[0]);
        draw_files(f, app, panels[1]);
    } else if has_downloads {
        let panels = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(content[1]);

        draw_results(f, app, panels[0]);
        draw_downloads(f, app, panels[1]);
    } else {
        draw_results(f, app, content[1]);
    }

    draw_status_bar(f, app, outer[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let user_display = app
        .logged_in_user
        .as_ref()
        .map(|u| format!(" {} ", u))
        .unwrap_or_else(|| " ··· ".to_string());

    let left = Span::styled(" slsk ", Style::default().fg(ACCENT).bold());
    let right = Span::styled(user_display.clone(), Style::default().fg(TEXT_DIM));

    // Calculate available width for status text to prevent overlap with username
    let prefix_width = 8; // " slsk " + "│" + " "
    let right_width = user_display.len() as u16;
    let available_width = area.width.saturating_sub(prefix_width + right_width + 1);

    let status_display = if app.status.len() as u16 > available_width {
        let truncate_at = available_width.saturating_sub(1) as usize;
        format!(
            "{}…",
            &app.status.chars().take(truncate_at).collect::<String>()
        )
    } else {
        app.status.clone()
    };

    let header = Line::from(vec![
        left,
        Span::styled("│", Style::default().fg(DIM)),
        Span::raw(" "),
        Span::styled(status_display, Style::default().fg(TEXT_DIM)),
    ]);

    let para = Paragraph::new(header).style(Style::default().bg(SURFACE_BRIGHT));
    f.render_widget(para, area);

    let right_para = Paragraph::new(Line::from(right))
        .alignment(Alignment::Right)
        .style(Style::default().bg(SURFACE_BRIGHT));
    f.render_widget(right_para, area);
}

fn draw_search_bar(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Search;
    let is_editing = app.input_mode == InputMode::Editing;

    let border_color = if is_focused { ACCENT } else { DIM };

    let placeholder = if app.search_input.is_empty() && !is_editing {
        "Type / to search..."
    } else {
        ""
    };

    let display_text = if app.search_input.is_empty() {
        placeholder
    } else {
        &app.search_input
    };

    let text_style = if app.search_input.is_empty() {
        Style::default().fg(TEXT_DIM)
    } else {
        Style::default().fg(TEXT)
    };

    let icon = if is_editing { "› " } else { "  " };
    let content = Line::from(vec![
        Span::styled(icon, Style::default().fg(ACCENT)),
        Span::styled(display_text, text_style),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(SURFACE));

    let input = Paragraph::new(content).block(block);
    f.render_widget(input, area);

    if is_editing {
        f.set_cursor_position((area.x + app.cursor_position as u16 + 4, area.y + 1));
    }
}

fn draw_results(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Results;
    let border_color = if is_focused { ACCENT } else { DIM };

    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let is_selected = i == app.selected_result && is_focused;
            let speed_mb = result.avg_speed as f64 / 1_000_000.0;
            let file_count = result.files.len();

            let slot_char = if result.slot_free { "●" } else { "○" };
            let slot_color = if result.slot_free { SUCCESS } else { WARNING };

            let mut spans = vec![
                Span::styled(format!(" {} ", slot_char), Style::default().fg(slot_color)),
                Span::styled(&result.username, Style::default().fg(TEXT).bold()),
                Span::styled(
                    format!("  {} files", file_count),
                    Style::default().fg(TEXT_DIM),
                ),
            ];

            if speed_mb > 0.1 {
                spans.push(Span::styled(
                    format!("  {:.1} MB/s", speed_mb),
                    Style::default().fg(TEXT_DIM),
                ));
            }

            let style = if is_selected {
                Style::default().bg(SURFACE_BRIGHT)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let count_str = if app.search_results.is_empty() {
        String::new()
    } else {
        format!("({})", app.search_results.len())
    };
    let title = format!(" Results {} ", count_str);

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(TEXT_DIM)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(SURFACE));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_files(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Files;
    let border_color = if is_focused { ACCENT } else { DIM };

    let (title, items) = if let Some((username, files)) = &app.current_search_files {
        let title = format!(" {} ({} matches) ", username, files.len());
        let items: Vec<ListItem> = files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let is_selected = i == app.selected_file && is_focused;

                let filename = file
                    .filename
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&file.filename);

                let size_mb = file.size as f64 / 1_048_576.0;
                let size_str = if size_mb >= 1.0 {
                    format!("{:.1} MB", size_mb)
                } else {
                    format!("{:.0} KB", file.size as f64 / 1024.0)
                };

                let path = file
                    .filename
                    .rsplit_once(['/', '\\'])
                    .map(|(p, _)| p)
                    .unwrap_or("");
                let path_display = if path.len() > 40 {
                    format!("...{}", &path[path.len().saturating_sub(37)..])
                } else {
                    path.to_string()
                };

                let spans = vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(filename, Style::default().fg(TEXT)),
                    Span::styled(format!("  {}", size_str), Style::default().fg(TEXT_DIM)),
                ];

                let style = if is_selected {
                    Style::default().bg(SURFACE_BRIGHT)
                } else {
                    Style::default()
                };

                let mut lines = vec![Line::from(spans)];
                if is_selected && !path_display.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(path_display, Style::default().fg(DIM).italic()),
                    ]));
                }

                ListItem::new(lines).style(style)
            })
            .collect();
        (title, items)
    } else if let Some((username, dirs)) = &app.current_user_files {
        let total: usize = dirs.iter().map(|d| d.files.len()).sum();
        let title = format!(" {} ({} files) ", username, total);
        let flat_files = app.get_current_files_flat();
        let items: Vec<ListItem> = flat_files
            .iter()
            .enumerate()
            .map(|(i, (name, file))| {
                let is_selected = i == app.selected_file && is_focused;

                let content: Vec<Span> = if let Some(f) = file {
                    let size_mb = f.size as f64 / 1_048_576.0;
                    vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(name.clone(), Style::default().fg(TEXT)),
                        Span::styled(
                            format!("  {:.1} MB", size_mb),
                            Style::default().fg(TEXT_DIM),
                        ),
                    ]
                } else {
                    vec![
                        Span::styled("  ▸ ", Style::default().fg(ACCENT)),
                        Span::styled(name.clone(), Style::default().fg(ACCENT)),
                    ]
                };

                let style = if is_selected {
                    Style::default().bg(SURFACE_BRIGHT)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(content)).style(style)
            })
            .collect();
        (title, items)
    } else {
        (" Files ".to_string(), vec![])
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(TEXT_DIM)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(SURFACE));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_downloads(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Downloads;
    let border_color = if is_focused { ACCENT } else { DIM };

    let items: Vec<ListItem> = app
        .downloads
        .iter()
        .enumerate()
        .map(|(i, dl)| {
            let is_selected = i == app.selected_download && is_focused;

            let (status_char, status_color) = match &dl.status {
                DownloadStatus::Queued => ("◌", WARNING),
                DownloadStatus::Connecting => ("◐", ACCENT),
                DownloadStatus::Downloading => ("◑", ACCENT),
                DownloadStatus::Completed => ("●", SUCCESS),
                DownloadStatus::Failed(_) => ("✕", Color::Rgb(239, 83, 80)),
            };

            let progress = if dl.size > 0 {
                (dl.downloaded as f64 / dl.size as f64 * 100.0) as u8
            } else {
                0
            };

            let progress_str = match &dl.status {
                DownloadStatus::Completed => "done".to_string(),
                DownloadStatus::Failed(_) => "failed".to_string(),
                DownloadStatus::Downloading => format!("{}%", progress),
                DownloadStatus::Queued => "queued".to_string(),
                DownloadStatus::Connecting => "connecting".to_string(),
            };

            let spans = vec![
                Span::styled(
                    format!(" {} ", status_char),
                    Style::default().fg(status_color),
                ),
                Span::styled(&dl.filename, Style::default().fg(TEXT)),
                Span::styled(format!("  {}", progress_str), Style::default().fg(TEXT_DIM)),
            ];

            let style = if is_selected {
                Style::default().bg(SURFACE_BRIGHT)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let active = app
        .downloads
        .iter()
        .filter(|d| matches!(d.status, DownloadStatus::Downloading))
        .count();
    let title = if active > 0 {
        format!(" Downloads ({} active) ", active)
    } else {
        format!(" Downloads ({}) ", app.downloads.len())
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(TEXT_DIM)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(SURFACE));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_playlist(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Playlist;
    let border_color = if is_focused { ACCENT } else { DIM };

    let Some(playlist) = &app.spotify_playlist else {
        return;
    };

    let items: Vec<ListItem> = playlist
        .tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_selected = i == app.selected_playlist_track && is_focused;
            let is_searching = app.spotify_searching_track == Some(i);

            let (status_char, status_color) = if track.matched_file.is_some() {
                ("●", SUCCESS)
            } else if is_searching {
                ("◐", ACCENT)
            } else {
                ("○", DIM)
            };

            let display = track.spotify_track.display_name();
            let display_truncated = if display.len() > 60 {
                format!("{}...", &display[..57])
            } else {
                display
            };

            let mut spans = vec![
                Span::styled(
                    format!(" {} ", status_char),
                    Style::default().fg(status_color),
                ),
                Span::styled(display_truncated, Style::default().fg(TEXT)),
            ];

            if let Some(matched) = &track.matched_file
                && let Some(bitrate) = matched.bitrate
            {
                spans.push(Span::styled(
                    format!("  {}kbps", bitrate),
                    Style::default().fg(TEXT_DIM),
                ));
            }

            let style = if is_selected {
                Style::default().bg(SURFACE_BRIGHT)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let matched = playlist.matched_count();
    let total = playlist.tracks.len();
    let title = format!(" {} ({}/{} matched) ", playlist.name, matched, total);

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(TEXT_DIM)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(SURFACE));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let bindings = if app.spotify_playlist.is_some() {
        vec![
            ("q", "quit"),
            ("/", "search"),
            ("↑↓", "nav"),
            ("⏎", "search track"),
            ("a", "search all"),
            ("D", "download all"),
            ("esc", "clear"),
        ]
    } else {
        vec![
            ("q", "quit"),
            ("/", "search"),
            ("↑↓", "nav"),
            ("d/⏎", "download"),
            ("esc", "back"),
        ]
    };

    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, (key, desc)) in bindings.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default()));
        }
        spans.push(Span::styled(*key, Style::default().fg(ACCENT)));
        spans.push(Span::styled(
            format!(" {}", desc),
            Style::default().fg(TEXT_DIM),
        ));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE_BRIGHT));
    f.render_widget(bar, area);
}
