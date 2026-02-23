use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};

fn collect_screen_files(dir: &Path) -> anyhow::Result<Vec<std::fs::DirEntry>> {
    let mut files: Vec<_> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("screen-")
                && entry.file_name().to_string_lossy().ends_with(".txt")
        })
        .collect();

    // Sort by frame number
    files.sort_by_key(|entry| {
        let name = entry.file_name().to_string_lossy().to_string();
        // Extract number from "screen-XXX.txt"
        name.trim_start_matches("screen-")
            .trim_end_matches(".txt")
            .parse::<usize>()
            .unwrap_or(0)
    });

    Ok(files)
}

pub fn replay_frames(dir: &Path, delay_ms: u64, loop_replay: bool) -> anyhow::Result<()> {
    let initial_files = collect_screen_files(dir)?;

    if initial_files.is_empty() {
        anyhow::bail!("No screen dump files found in {}", dir.display());
    }

    let is_tty = io::stdin().is_terminal();

    println!(
        "Replaying frames from {} (delay: {}ms, loop: {})",
        dir.display(),
        delay_ms,
        loop_replay
    );

    if is_tty {
        println!("Press 'q' to quit, 'p' to pause/resume\n");
    } else {
        println!();
    }

    let delay = Duration::from_millis(delay_ms);
    let mut paused = false;
    let mut last_shown_frame: usize = 0;

    loop {
        // Re-scan directory to pick up new frames
        let files = collect_screen_files(dir)?;

        for entry in files.iter() {
            // Skip frames we've already shown
            let frame_name = entry.file_name().to_string_lossy().to_string();
            let frame_num = frame_name
                .trim_start_matches("screen-")
                .trim_end_matches(".txt")
                .parse::<usize>()
                .unwrap_or(0);
            if frame_num <= last_shown_frame {
                continue;
            }
            // Check for quit/pause input (only if TTY)
            if is_tty && event::poll(Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => {
                            execute!(std::io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                            return Ok(());
                        }
                        KeyCode::Char('p') => {
                            paused = !paused;
                        }
                        _ => {}
                    }
                }
            }

            if paused {
                // Wait while paused
                loop {
                    if !is_tty {
                        // Non-TTY mode: can't pause, just continue
                        break;
                    }
                    if event::poll(Duration::from_millis(100))? {
                        if let Event::Key(key) = event::read()? {
                            match key.code {
                                KeyCode::Char('q') => {
                                    execute!(
                                        std::io::stdout(),
                                        Clear(ClearType::All),
                                        MoveTo(0, 0)
                                    )?;
                                    return Ok(());
                                }
                                KeyCode::Char('p') => {
                                    paused = false;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Read and display frame
            let content = fs::read_to_string(entry.path())?;

            // Clear screen and move to top (only if TTY)
            if is_tty {
                execute!(std::io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
            } else {
                // Print separator for non-TTY mode
                println!("{}", "─".repeat(80));
            }

            // Print frame content
            if is_tty {
                execute!(std::io::stdout(), Print(&content))?;
            } else {
                print!("{}", content);
            }

            // Show frame info
            if is_tty {
                println!("\n[{} - press 'q' to quit, 'p' to pause]", frame_name);
            } else {
                println!("\n[{}]", frame_name);
            }

            std::io::stdout().flush()?;

            last_shown_frame = frame_num;
            std::thread::sleep(delay);
        }

        if !loop_replay {
            // In non-loop mode, wait for new frames before exiting
            // Re-scan to check if there are new frames
            let files = collect_screen_files(dir)?;
            let max_frame = files
                .iter()
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.trim_start_matches("screen-")
                        .trim_end_matches(".txt")
                        .parse::<usize>()
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0);

            if max_frame <= last_shown_frame {
                break;
            }
            // If there are new frames, continue the loop to display them
        } else {
            // In loop mode, add a small delay before rescanning
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    Ok(())
}
