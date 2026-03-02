use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use hh_cli::cli::tui::{ChatApp, ChatMessage, TuiEvent};

#[derive(Debug, Clone, Copy)]
struct Config {
    history_pairs: usize,
    typing_steps: usize,
    stream_steps: usize,
    wrap_width: usize,
    visible_height: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            history_pairs: 600,
            typing_steps: 200,
            stream_steps: 200,
            wrap_width: 96,
            visible_height: 30,
        }
    }
}

fn main() {
    let config = parse_args(std::env::args().skip(1).collect());
    let mut app = build_app_with_history(config.history_pairs);

    warm_cache(&mut app, config.wrap_width, config.visible_height);

    let typing = measure_typing_path(
        &mut app,
        config.typing_steps,
        config.wrap_width,
        config.visible_height,
    );
    let streaming = measure_stream_path(
        &mut app,
        config.stream_steps,
        config.wrap_width,
        config.visible_height,
    );
    let coalesced = measure_stream_path_coalesced(
        &mut app,
        config.stream_steps,
        config.wrap_width,
        config.visible_height,
        8,
    );

    println!("tui_perf_probe");
    println!(
        "history_pairs={} typing_steps={} stream_steps={} wrap_width={} visible_height={}",
        config.history_pairs,
        config.typing_steps,
        config.stream_steps,
        config.wrap_width,
        config.visible_height
    );
    print_stats("typing_update", &typing);
    print_stats("stream_delta_update", &streaming);
    print_stats("stream_coalesced_flush", &coalesced);
    println!("target: p95 < 16.0ms");
}

fn parse_args(args: Vec<String>) -> Config {
    let mut config = Config::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--history" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.history_pairs = value;
                }
                i += 1;
            }
            "--typing" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.typing_steps = value;
                }
                i += 1;
            }
            "--stream" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.stream_steps = value;
                }
                i += 1;
            }
            "--width" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.wrap_width = value;
                }
                i += 1;
            }
            "--height" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.visible_height = value;
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    config
}

fn warm_cache(app: &mut ChatApp, wrap_width: usize, visible_height: usize) {
    let total = {
        let lines = app.get_lines(wrap_width);
        lines.len()
    };
    let offset = app.message_scroll.effective_offset(total, visible_height);
    let visible = app.get_visible_lines(wrap_width, visible_height, offset);
    black_box(visible.len());
}

fn measure_typing_path(
    app: &mut ChatApp,
    steps: usize,
    wrap_width: usize,
    visible_height: usize,
) -> Vec<Duration> {
    let mut durations = Vec::with_capacity(steps);

    for _ in 0..steps {
        let started = Instant::now();
        app.insert_char('x');

        let total = {
            let lines = app.get_lines(wrap_width);
            lines.len()
        };
        let offset = app.message_scroll.effective_offset(total, visible_height);
        let visible = app.get_visible_lines(wrap_width, visible_height, offset);
        black_box(visible.len());

        durations.push(started.elapsed());
    }

    durations
}

fn measure_stream_path(
    app: &mut ChatApp,
    steps: usize,
    wrap_width: usize,
    visible_height: usize,
) -> Vec<Duration> {
    let mut durations = Vec::with_capacity(steps);

    for i in 0..steps {
        let started = Instant::now();
        app.handle_event(&TuiEvent::AssistantDelta(format!(
            "delta #{i}: synthetic stream payload for render-path timing.\n"
        )));

        let total = {
            let lines = app.get_lines(wrap_width);
            lines.len()
        };
        let offset = app.message_scroll.effective_offset(total, visible_height);
        let visible = app.get_visible_lines(wrap_width, visible_height, offset);
        black_box(visible.len());

        durations.push(started.elapsed());
    }

    durations
}

fn measure_stream_path_coalesced(
    app: &mut ChatApp,
    steps: usize,
    wrap_width: usize,
    visible_height: usize,
    chunk_size: usize,
) -> Vec<Duration> {
    let mut durations = Vec::with_capacity((steps / chunk_size).max(1));
    let mut pending = String::new();

    for i in 0..steps {
        pending.push_str(&format!(
            "delta #{i}: synthetic stream payload for render-path timing.\n"
        ));

        if (i + 1) % chunk_size != 0 {
            continue;
        }

        let started = Instant::now();
        app.handle_event(&TuiEvent::AssistantDelta(std::mem::take(&mut pending)));

        let total = {
            let lines = app.get_lines(wrap_width);
            lines.len()
        };
        let offset = app.message_scroll.effective_offset(total, visible_height);
        let visible = app.get_visible_lines(wrap_width, visible_height, offset);
        black_box(visible.len());

        durations.push(started.elapsed());
    }

    if !pending.is_empty() {
        let started = Instant::now();
        app.handle_event(&TuiEvent::AssistantDelta(std::mem::take(&mut pending)));
        let total = {
            let lines = app.get_lines(wrap_width);
            lines.len()
        };
        let offset = app.message_scroll.effective_offset(total, visible_height);
        let visible = app.get_visible_lines(wrap_width, visible_height, offset);
        black_box(visible.len());
        durations.push(started.elapsed());
    }

    durations
}

fn print_stats(name: &str, samples: &[Duration]) {
    if samples.is_empty() {
        println!("{name}: no samples");
        return;
    }

    let mut nanos: Vec<u128> = samples.iter().map(Duration::as_nanos).collect();
    nanos.sort_unstable();

    let len = nanos.len();
    let median = nanos[len / 2] as f64 / 1_000_000.0;
    let p95_index = ((len as f64) * 0.95).ceil() as usize;
    let p95 = nanos[p95_index.saturating_sub(1).min(len - 1)] as f64 / 1_000_000.0;
    let max = nanos[len - 1] as f64 / 1_000_000.0;
    let mean = nanos.iter().copied().sum::<u128>() as f64 / len as f64 / 1_000_000.0;

    println!("{name}: mean={mean:.3}ms median={median:.3}ms p95={p95:.3}ms max={max:.3}ms");
}

fn build_app_with_history(message_pairs: usize) -> ChatApp {
    let mut app = ChatApp::new("perf-probe".to_string(), Path::new("."));
    app.input = "measure typing latency under long history".to_string();

    for i in 0..message_pairs {
        app.messages.push(ChatMessage::User(format!(
            "User message #{i}: explain why history depth matters for render and input latency."
        )));

        app.messages
            .push(ChatMessage::Assistant(sample_assistant_markdown(i)));

        if i % 20 == 0 {
            app.messages.push(ChatMessage::ToolCall {
                name: "edit".to_string(),
                args: "{\"path\":\"src/cli/tui/ui.rs\"}".to_string(),
                output: Some(
                    "{\"path\":\"src/cli/tui/ui.rs\",\"summary\":{\"added_lines\":12,\"removed_lines\":3}}"
                        .to_string(),
                ),
                is_error: Some(false),
            });
        }
    }

    app
}

fn sample_assistant_markdown(i: usize) -> String {
    format!(
        "Assistant response #{i}\n\n- Keep repaint work proportional to viewport\n- Avoid cloning large line buffers on every frame\n\n```rust file=src/cli/tui/ui.rs\nfn render_messages() {{\n    // synthetic benchmark payload\n}}\n```\n"
    )
}
