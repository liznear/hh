use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hh_widgets::codediff::{CodeDiffBlock, CodeDiffOptions, render_unified_diff};

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
    files: usize,
    hunks_per_file: usize,
    lines_per_hunk: usize,
    iterations: usize,
    enforce: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            files: 120,
            hunks_per_file: 3,
            lines_per_hunk: 24,
            iterations: 100,
            enforce: false,
        }
    }
}

fn main() {
    let config = parse_args(std::env::args().skip(1).collect());
    let diff = build_large_diff(config.files, config.hunks_per_file, config.lines_per_hunk);
    let block = CodeDiffBlock::new(diff);

    let mut options = CodeDiffOptions::default();
    options.max_rendered_lines = 120;
    options.max_rendered_chars = 8_000;
    options.show_file_headers = true;

    let mut duration_samples = Vec::with_capacity(config.iterations);
    let mut alloc_samples = Vec::with_capacity(config.iterations);

    for _ in 0..config.iterations {
        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        let started = Instant::now();
        let rendered = render_unified_diff(&block, &options);
        black_box(rendered.lines.len());
        black_box(rendered.truncated);
        duration_samples.push(started.elapsed());
        alloc_samples.push(ALLOCATED_BYTES.load(Ordering::Relaxed));
    }

    let duration_stats = duration_stats(&duration_samples);
    let alloc_stats = allocation_stats(&alloc_samples);

    println!("codediff_perf_probe");
    println!(
        "files={} hunks_per_file={} lines_per_hunk={} iterations={}",
        config.files, config.hunks_per_file, config.lines_per_hunk, config.iterations
    );
    println!(
        "render_unified_diff: mean={:.3}ms median={:.3}ms p95={:.3}ms max={:.3}ms",
        duration_stats.mean_ms,
        duration_stats.median_ms,
        duration_stats.p95_ms,
        duration_stats.max_ms
    );
    println!(
        "allocations: mean={:.1}KB median={:.1}KB p95={:.1}KB max={:.1}KB",
        alloc_stats.mean_kb, alloc_stats.median_kb, alloc_stats.p95_kb, alloc_stats.max_kb
    );
    println!("guardrails: p95_ms<=5.0 max_ms<=12.0 p95_alloc_kb<=512.0 max_alloc_kb<=768.0");

    if config.enforce {
        let mut violations = Vec::new();
        if duration_stats.p95_ms > 5.0 {
            violations.push(format!("p95_ms {:.3} > 5.0", duration_stats.p95_ms));
        }
        if duration_stats.max_ms > 12.0 {
            violations.push(format!("max_ms {:.3} > 12.0", duration_stats.max_ms));
        }
        if alloc_stats.p95_kb > 512.0 {
            violations.push(format!("p95_alloc_kb {:.1} > 512.0", alloc_stats.p95_kb));
        }
        if alloc_stats.max_kb > 768.0 {
            violations.push(format!("max_alloc_kb {:.1} > 768.0", alloc_stats.max_kb));
        }

        if !violations.is_empty() {
            eprintln!("guardrail violations:");
            for v in violations {
                eprintln!("- {v}");
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
            "--files" => {
                if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.files = v;
                }
                i += 1;
            }
            "--hunks" => {
                if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.hunks_per_file = v;
                }
                i += 1;
            }
            "--lines" => {
                if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.lines_per_hunk = v;
                }
                i += 1;
            }
            "--iterations" => {
                if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<usize>().ok()) {
                    config.iterations = v;
                }
                i += 1;
            }
            "--enforce" => config.enforce = true,
            _ => {}
        }
        i += 1;
    }

    config
}

fn build_large_diff(files: usize, hunks_per_file: usize, lines_per_hunk: usize) -> String {
    let mut out = String::new();
    for file_idx in 0..files {
        out.push_str(&format!("--- a/src/file_{file_idx}.rs\n"));
        out.push_str(&format!("+++ b/src/file_{file_idx}.rs\n"));
        for hunk_idx in 0..hunks_per_file {
            let start = hunk_idx * lines_per_hunk + 1;
            out.push_str(&format!(
                "@@ -{start},{} +{start},{} @@\n",
                lines_per_hunk, lines_per_hunk
            ));
            for line_idx in 0..lines_per_hunk {
                out.push_str(&format!("-old line {file_idx}:{hunk_idx}:{line_idx}\n"));
                out.push_str(&format!("+new line {file_idx}:{hunk_idx}:{line_idx}\n"));
                out.push_str(&format!(" context line {file_idx}:{hunk_idx}:{line_idx}\n"));
            }
        }
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
