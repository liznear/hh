use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hh_widgets::codediff::CodeDiff;
use hh_widgets::markdown::MarkdownBlock;
use hh_widgets::scrollable::{max_offset_for, measure_children, visible_range, ScrollableState};
use hh_widgets::widget::WidgetNode;

static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATED_BYTES.fetch_add(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAlloc = CountingAlloc;

#[derive(Debug, Clone, Copy)]
struct Config {
    children: usize,
    width: u16,
    viewport_height: u16,
    iterations: usize,
    enforce: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            children: 4_000,
            width: 96,
            viewport_height: 30,
            iterations: 120,
            enforce: false,
        }
    }
}

fn main() {
    let config = parse_args(std::env::args().skip(1).collect());
    let children = build_children(config.children);

    let mut duration_samples = Vec::with_capacity(config.iterations);
    let mut allocation_samples = Vec::with_capacity(config.iterations);

    for i in 0..config.iterations {
        let mut state = ScrollableState::default();
        state.offset = ((i * 7) % (config.children.max(1))) as u16;
        state.viewport_height = config.viewport_height;
        state.auto_follow = false;

        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        let started = Instant::now();

        let layout = measure_children(&children, config.width);
        let max_offset = max_offset_for(layout.total_height, state.viewport_height);
        let _ = state.clamp_offset(max_offset);
        let range = visible_range(&layout, &state);
        black_box((range.start, range.end));

        duration_samples.push(started.elapsed());
        allocation_samples.push(ALLOCATED_BYTES.load(Ordering::Relaxed));
    }

    let duration_stats = duration_stats(&duration_samples);
    let allocation_stats = allocation_stats(&allocation_samples);

    println!("scrollable_perf_probe");
    println!(
        "children={} width={} viewport_height={} iterations={}",
        config.children, config.width, config.viewport_height, config.iterations
    );
    println!(
        "measure+slice: mean={:.3}ms median={:.3}ms p95={:.3}ms max={:.3}ms",
        duration_stats.mean_ms,
        duration_stats.median_ms,
        duration_stats.p95_ms,
        duration_stats.max_ms
    );
    println!(
        "allocations: mean={:.1}KB median={:.1}KB p95={:.1}KB max={:.1}KB",
        allocation_stats.mean_kb,
        allocation_stats.median_kb,
        allocation_stats.p95_kb,
        allocation_stats.max_kb
    );
    println!("guardrails: p95_ms<=8.0 max_ms<=20.0 p95_alloc_kb<=6144.0 max_alloc_kb<=7168.0");

    if config.enforce {
        let mut violations = Vec::new();
        if duration_stats.p95_ms > 8.0 {
            violations.push(format!("p95_ms {:.3} > 8.0", duration_stats.p95_ms));
        }
        if duration_stats.max_ms > 20.0 {
            violations.push(format!("max_ms {:.3} > 20.0", duration_stats.max_ms));
        }
        if allocation_stats.p95_kb > 6144.0 {
            violations.push(format!(
                "p95_alloc_kb {:.1} > 6144.0",
                allocation_stats.p95_kb
            ));
        }
        if allocation_stats.max_kb > 7168.0 {
            violations.push(format!(
                "max_alloc_kb {:.1} > 7168.0",
                allocation_stats.max_kb
            ));
        }

        if !violations.is_empty() {
            eprintln!("guardrail violations:");
            for violation in violations {
                eprintln!("- {violation}");
            }
            std::process::exit(1);
        }
    }
}

fn parse_args(args: Vec<String>) -> Config {
    let mut config = Config::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--children" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.children = value;
                }
                i += 1;
            }
            "--width" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<u16>().ok()) {
                    config.width = value;
                }
                i += 1;
            }
            "--height" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<u16>().ok()) {
                    config.viewport_height = value;
                }
                i += 1;
            }
            "--iterations" => {
                if let Some(value) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.iterations = value;
                }
                i += 1;
            }
            "--enforce" => {
                config.enforce = true;
            }
            _ => {}
        }

        i += 1;
    }

    config
}

fn build_children(children: usize) -> Vec<WidgetNode> {
    let mut out = Vec::with_capacity(children);
    for i in 0..children {
        let node = match i % 3 {
            0 => WidgetNode::Markdown(MarkdownBlock::new(format!(
                "### Item {i}\n\n- alpha\n- beta\n- gamma\n"
            ))),
            1 => WidgetNode::CodeDiff(CodeDiff::from_unified_diff(format!(
                "--- a/file_{i}.rs\n+++ b/file_{i}.rs\n@@ -1 +1 @@\n-old\n+new\n"
            ))),
            _ => WidgetNode::Spacer((i % 5 + 1) as u16),
        };
        out.push(node);
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct DurationStats {
    mean_ms: f64,
    median_ms: f64,
    p95_ms: f64,
    max_ms: f64,
}

#[derive(Debug, Clone, Copy)]
struct AllocationStats {
    mean_kb: f64,
    median_kb: f64,
    p95_kb: f64,
    max_kb: f64,
}

fn duration_stats(samples: &[Duration]) -> DurationStats {
    let mut nanos: Vec<u128> = samples.iter().map(Duration::as_nanos).collect();
    nanos.sort_unstable();

    let len = nanos.len();
    let median = nanos[len / 2] as f64 / 1_000_000.0;
    let p95_index = ((len as f64) * 0.95).ceil() as usize;
    let p95 = nanos[p95_index.saturating_sub(1).min(len - 1)] as f64 / 1_000_000.0;
    let max = nanos[len - 1] as f64 / 1_000_000.0;
    let mean = nanos.iter().copied().sum::<u128>() as f64 / len as f64 / 1_000_000.0;

    DurationStats {
        mean_ms: mean,
        median_ms: median,
        p95_ms: p95,
        max_ms: max,
    }
}

fn allocation_stats(samples: &[usize]) -> AllocationStats {
    let mut bytes = samples.to_vec();
    bytes.sort_unstable();

    let len = bytes.len();
    let median = bytes[len / 2] as f64 / 1024.0;
    let p95_index = ((len as f64) * 0.95).ceil() as usize;
    let p95 = bytes[p95_index.saturating_sub(1).min(len - 1)] as f64 / 1024.0;
    let max = bytes[len - 1] as f64 / 1024.0;
    let mean = bytes.iter().copied().sum::<usize>() as f64 / len as f64 / 1024.0;

    AllocationStats {
        mean_kb: mean,
        median_kb: median,
        p95_kb: p95,
        max_kb: max,
    }
}
