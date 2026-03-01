use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hh_cli::cli::tui::{build_message_lines, ChatApp, ChatMessage};

const WRAP_WIDTH: usize = 96;

fn bench_build_message_lines(c: &mut Criterion) {
    let mut group = c.benchmark_group("tui/build_message_lines");

    for message_pairs in [100usize, 300, 600] {
        let app = build_app_with_history(message_pairs);
        group.throughput(Throughput::Elements(message_pairs as u64 * 2));
        group.bench_with_input(
            BenchmarkId::from_parameter(message_pairs),
            &app,
            |b, app| {
                b.iter(|| {
                    let lines = build_message_lines(app, WRAP_WIDTH);
                    black_box(lines.len());
                });
            },
        );
    }

    group.finish();
}

fn bench_context_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("tui/context_usage");

    for message_pairs in [100usize, 300, 600] {
        let app = build_app_with_history(message_pairs);
        group.throughput(Throughput::Elements(message_pairs as u64 * 2));
        group.bench_with_input(
            BenchmarkId::from_parameter(message_pairs),
            &app,
            |b, app| {
                b.iter(|| {
                    let usage = app.context_usage();
                    black_box(usage);
                });
            },
        );
    }

    group.finish();
}

fn bench_get_lines_rebuild(c: &mut Criterion) {
    let mut group = c.benchmark_group("tui/get_lines_rebuild");

    for message_pairs in [100usize, 300, 600] {
        let app = build_app_with_history(message_pairs);
        group.throughput(Throughput::Elements(message_pairs as u64 * 2));
        group.bench_with_input(
            BenchmarkId::from_parameter(message_pairs),
            &app,
            |b, app| {
                b.iter(|| {
                    app.mark_dirty();
                    let lines = app.get_lines(WRAP_WIDTH);
                    black_box(lines.len());
                });
            },
        );
    }

    group.finish();
}

fn build_app_with_history(message_pairs: usize) -> ChatApp {
    let mut app = ChatApp::new("benchmark-session".to_string(), Path::new("."));
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

criterion_group!(
    benches,
    bench_build_message_lines,
    bench_context_usage,
    bench_get_lines_rebuild
);
criterion_main!(benches);
