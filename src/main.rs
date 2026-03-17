use chrono::Local;
use env_logger;
use env_logger::fmt::Formatter;
use log::info;
use rand::seq::SliceRandom;
use rand::thread_rng;
use slog::{o, Drain, Logger};
use slog_async;
use slog_term;
use std::collections::HashMap;
use std::io::Write;
use std::process::{exit, Command};
use std::time::Instant;
use std::{env, fs, thread};
use tracing::{event, Level};

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// Returns the number of bytes currently allocated on the heap according to
/// jemalloc. Advances the stats epoch first so the read is current.
fn jemalloc_allocated() -> usize {
    tikv_jemalloc_ctl::epoch::mib().unwrap().advance().unwrap();
    tikv_jemalloc_ctl::stats::allocated::mib()
        .unwrap()
        .read()
        .unwrap()
}

const ITERATIONS: u32 = 100_000;
const NUM_RUNS: usize = 3;
const NUM_WARMUP_RUNS: usize = 1;
const MESSAGE: &str = "A logging message that is reasonably long";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OutputTarget {
    Sink,
    Stdout,
    File,
}

impl OutputTarget {
    fn as_str(&self) -> &'static str {
        match self {
            OutputTarget::Sink => "sink",
            OutputTarget::Stdout => "stdout",
            OutputTarget::File => "file",
        }
    }

    fn all() -> Vec<OutputTarget> {
        vec![OutputTarget::Sink, OutputTarget::Stdout, OutputTarget::File]
    }
}

struct BenchmarkResult {
    name: String,
    elapsed: f64,
    ops: f64,
    memory_usage: f64,
    target: OutputTarget,
}

#[derive(Default)]
struct BenchmarkStats {
    elapsed_times: Vec<f64>,
    ops_rates: Vec<f64>,
    memory_usages: Vec<f64>,
}

fn run_benchmark<F: Fn()>(name: &str, target: OutputTarget, bench_fn: F) -> BenchmarkResult {
    let before = jemalloc_allocated();
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        bench_fn();
    }
    let elapsed = start.elapsed();
    let after = jemalloc_allocated();
    BenchmarkResult {
        name: name.to_string(),
        target,
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: after.saturating_sub(before) as f64
            / (1024.0 * 1024.0),
    }
}

fn bench_env_logger(target: OutputTarget) -> BenchmarkResult {
    env::set_var("RUST_LOG", "info");

    let mut builder = env_logger::Builder::from_default_env();
    builder.format(|buf: &mut Formatter, record| {
        writeln!(
            buf,
            "{{\"level\":\"{}\",\"target\":\"{}\",\"message\":\"{}\"}}",
            record.level(),
            record.target(),
            record.args()
        )
    });

    match target {
        OutputTarget::Sink => {
            builder
                .target(env_logger::Target::Pipe(Box::new(std::io::sink())))
                .init();
        }
        OutputTarget::Stdout => {
            builder.target(env_logger::Target::Stdout).init();
        }
        OutputTarget::File => {
            let log_file = std::fs::File::create("logs/env_logger.log").unwrap();
            builder
                .target(env_logger::Target::Pipe(Box::new(log_file)))
                .init();
        }
    }

    run_benchmark("env_logger", target, || {
        info!("{} {}", MESSAGE, "env_logger");
    })
}

fn bench_fern(target: OutputTarget) -> BenchmarkResult {
    let dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                r#"{{"level":"{}","target":"{}","message":"{}"}}"#,
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Info);

    match target {
        OutputTarget::Sink => dispatch
            .chain(Box::new(std::io::sink()) as Box<dyn std::io::Write + Send>)
            .apply()
            .unwrap(),
        OutputTarget::Stdout => dispatch.chain(std::io::stdout()).apply().unwrap(),
        OutputTarget::File => {
            let f = std::fs::File::create("logs/fern.log").unwrap();
            dispatch.chain(f).apply().unwrap()
        }
    }

    run_benchmark("fern", target, || {
        log::info!("{} {}", MESSAGE, "fern");
    })
}

fn bench_slog(target: OutputTarget) -> BenchmarkResult {
    let root = match target {
        OutputTarget::Sink => {
            let decorator = slog_term::PlainSyncDecorator::new(std::io::sink());
            Logger::root(slog_term::FullFormat::new(decorator).build().fuse(), o!())
        }
        OutputTarget::Stdout => {
            let decorator = slog_term::PlainSyncDecorator::new(std::io::stdout());
            Logger::root(slog_term::FullFormat::new(decorator).build().fuse(), o!())
        }
        OutputTarget::File => {
            let log_file = std::fs::File::create("logs/slog.log").unwrap();
            let drain = std::sync::Mutex::new(slog_json::Json::default(log_file)).fuse();
            Logger::root(drain, o!())
        }
    };

    run_benchmark("slog", target, move || {
        slog::info!(root, "{} {}", MESSAGE, "slog");
    })
}

fn bench_slog_async(target: OutputTarget) -> BenchmarkResult {
    const CHANNEL_SIZE: usize = 50_000;

    let drain = match target {
        OutputTarget::Sink => {
            let decorator = slog_term::PlainSyncDecorator::new(std::io::sink());
            let drain = slog_term::FullFormat::new(decorator).build().fuse();
            slog_async::Async::new(drain)
                .chan_size(CHANNEL_SIZE)
                .overflow_strategy(slog_async::OverflowStrategy::Block)
                .build()
                .fuse()
        }
        OutputTarget::Stdout => {
            let decorator = slog_term::PlainSyncDecorator::new(std::io::stdout());
            let drain = slog_term::FullFormat::new(decorator).build().fuse();
            slog_async::Async::new(drain)
                .chan_size(CHANNEL_SIZE)
                .overflow_strategy(slog_async::OverflowStrategy::Block)
                .build()
                .fuse()
        }
        OutputTarget::File => {
            let log_file = std::fs::File::create("logs/slog_async.log").unwrap();
            let drain = slog_json::Json::default(log_file).fuse();
            slog_async::Async::new(drain)
                .chan_size(CHANNEL_SIZE)
                .overflow_strategy(slog_async::OverflowStrategy::Block)
                .build()
                .fuse()
        }
    };

    let root = Logger::root(drain, o!());

    // Drop root inside the timed window so the worker-thread join is included.
    // Without this, the 100K iterations measure enqueue speed only.
    let before = jemalloc_allocated();
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        slog::info!(root, "{} {}", MESSAGE, "slog_async");
    }
    drop(root);
    let elapsed = start.elapsed();
    let after = jemalloc_allocated();

    BenchmarkResult {
        name: "slog_async".to_string(),
        target,
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: after.saturating_sub(before) as f64
            / (1024.0 * 1024.0),
    }
}

fn bench_tracing(target: OutputTarget) -> BenchmarkResult {
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    let writer = match target {
        OutputTarget::Sink => BoxMakeWriter::new(std::io::sink),
        OutputTarget::Stdout => BoxMakeWriter::new(std::io::stdout),
        OutputTarget::File => {
            let file = std::fs::File::create("logs/tracing.log").unwrap();
            BoxMakeWriter::new(move || file.try_clone().unwrap())
        }
    };

    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(writer)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");

    run_benchmark("tracing", target, || {
        event!(Level::INFO, "{} {}", MESSAGE, "tracing");
    })
}

fn bench_tracing_async(target: OutputTarget) -> BenchmarkResult {
    use tracing_appender::non_blocking::NonBlockingBuilder;
    use tracing_subscriber::fmt;

    const CHANNEL_SIZE: usize = 50_000;

    let (writer, guard) = match target {
        OutputTarget::Sink => NonBlockingBuilder::default()
            .buffered_lines_limit(CHANNEL_SIZE)
            .lossy(false)
            .finish(std::io::sink()),
        OutputTarget::Stdout => NonBlockingBuilder::default()
            .buffered_lines_limit(CHANNEL_SIZE)
            .lossy(false)
            .finish(std::io::stdout()),
        OutputTarget::File => {
            let file = std::fs::File::create("logs/tracing_async.log").unwrap();
            NonBlockingBuilder::default()
                .buffered_lines_limit(CHANNEL_SIZE)
                .lossy(false)
                .finish(file)
        }
    };

    let subscriber = fmt().json().with_writer(writer).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");

    // Drop guard inside the timed window — WorkerGuard::drop blocks until the
    // background thread has flushed all buffered messages.
    let before = jemalloc_allocated();
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        event!(Level::INFO, "{} {}", MESSAGE, "tracing_async");
    }
    drop(guard);
    let elapsed = start.elapsed();
    let after = jemalloc_allocated();

    BenchmarkResult {
        name: "tracing_async".to_string(),
        target,
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: after.saturating_sub(before) as f64
            / (1024.0 * 1024.0),
    }
}

fn bench_winston(target: OutputTarget) -> BenchmarkResult {
    let builder = winston::Logger::builder()
        .channel_capacity(50_000)
        .backpressure_strategy(winston::BackpressureStrategy::Block);

    let logger = match target {
        OutputTarget::Sink => builder
            .transport(winston::transports::WriterTransport::new(std::io::sink()))
            .build(),
        OutputTarget::Stdout => builder.transport(winston::transports::stdout()).build(),
        OutputTarget::File => {
            let log_file = std::fs::File::create("logs/winston.log").unwrap();
            builder
                .transport(winston::transports::WriterTransport::new(log_file))
                .build()
        }
    };

    // Drop logger inside the timed window so the internal worker channel is
    // fully drained before elapsed is captured.
    let before = jemalloc_allocated();
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        winston::log!(logger, info, format!("{} {}", MESSAGE, "winston"));
    }
    drop(logger);
    let elapsed = start.elapsed();
    let after = jemalloc_allocated();

    BenchmarkResult {
        name: "winston".to_string(),
        target,
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: after.saturating_sub(before) as f64
            / (1024.0 * 1024.0),
    }
}

fn run_individual_benchmark(benchmark_name: &str, target_name: &str) -> BenchmarkResult {
    let target = match target_name {
        "sink" => OutputTarget::Sink,
        "stdout" => OutputTarget::Stdout,
        "file" => OutputTarget::File,
        _ => panic!("Unknown target: {}", target_name),
    };

    let result = match benchmark_name {
        "env_logger" => bench_env_logger(target),
        "fern" => bench_fern(target),
        "slog" => bench_slog(target),
        "slog_async" => bench_slog_async(target),
        "tracing" => bench_tracing(target),
        "tracing_async" => bench_tracing_async(target),
        "winston" => bench_winston(target),
        _ => panic!("Unknown benchmark: {}", benchmark_name),
    };

    println!(
        "LOGMARK: {} {} {:.4} {:.4} {:.4}",
        result.name,
        result.target.as_str(),
        result.elapsed,
        result.ops,
        result.memory_usage
    );

    result
}

fn run_benchmarks_in_processes(
    benchmarks: &[&str],
    targets: &[OutputTarget],
    runs: usize,
) -> HashMap<String, BenchmarkStats> {
    let mut results: HashMap<String, BenchmarkStats> = HashMap::new();
    let mut rng = thread_rng();

    let _ = fs::create_dir_all("benchmark_results");
    let _ = fs::create_dir_all("logs");

    let mut all_benchmarks: Vec<(&str, OutputTarget)> = benchmarks
        .iter()
        .flat_map(|&bench| targets.iter().map(move |&target| (bench, target)))
        .collect();

    for warmup in 1..=NUM_WARMUP_RUNS {
        println!(
            "\n-- warmup {} of {}  [{}] --",
            warmup,
            NUM_WARMUP_RUNS,
            Local::now().format("%H:%M:%S")
        );
        all_benchmarks.shuffle(&mut rng);
        for &(bench, target) in &all_benchmarks {
            println!("  {} ({})", bench, target.as_str());
            let start = Instant::now();
            let _ = Command::new(env::current_exe().unwrap())
                .arg("--benchmark")
                .arg(bench)
                .arg(target.as_str())
                .output();
            println!("    done  {:.4}s", start.elapsed().as_secs_f64());
        }
    }

    for run in 1..=runs {
        println!(
            "\n-- run {} of {}  [{}] --",
            run,
            runs,
            Local::now().format("%H:%M:%S")
        );
        all_benchmarks.shuffle(&mut rng);

        for &(bench, target) in &all_benchmarks {
            println!("  {} ({})", bench, target.as_str());

            let output = Command::new(env::current_exe().unwrap())
                .arg("--benchmark")
                .arg(bench)
                .arg(target.as_str())
                .output()
                .expect("Failed to run benchmark");

            if output.status.success() {
                let output_str = String::from_utf8(output.stdout).unwrap();

                if let Some(line) = output_str.lines().find(|l| l.starts_with("LOGMARK: ")) {
                    let result_parts: Vec<&str> =
                        line["LOGMARK: ".len()..].split_whitespace().collect();
                    if result_parts.len() >= 5 {
                        let name = format!("{}_{}", result_parts[0], result_parts[1]);
                        let elapsed: f64 = result_parts[2].parse().unwrap();
                        let ops: f64 = result_parts[3].parse().unwrap();
                        let memory: f64 = result_parts[4].parse().unwrap();

                        let stats = results.entry(name.clone()).or_default();
                        stats.elapsed_times.push(elapsed);
                        stats.ops_rates.push(ops);
                        stats.memory_usages.push(memory);

                        println!(
                            "    [{}] done  {:.4}s  {}  {}",
                            Local::now().format("%H:%M:%S"),
                            elapsed,
                            fmt_ops(ops),
                            fmt_mem(memory)
                        );
                    }
                }
            } else {
                eprintln!(
                    "Benchmark {} ({}) failed:\n{}",
                    bench,
                    target.as_str(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    results
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if sorted.len() % 2 == 0 {
        let mid = sorted.len() / 2;
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    }
}

fn std_dev(values: &[f64]) -> f64 {
    if values.len() <= 1 {
        return 0.0;
    }
    let m = mean(values);
    let variance =
        values.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (values.len() as f64 - 1.0);
    variance.sqrt()
}

fn fmt_ops(ops: f64) -> String {
    if ops >= 1_000_000.0 {
        format!("{:.2}M", ops / 1_000_000.0)
    } else if ops >= 1_000.0 {
        format!("{:.1}K", ops / 1_000.0)
    } else {
        format!("{:.0}", ops)
    }
}

fn fmt_mem(mb: f64) -> String {
    if mb >= 1.0 {
        format!("{:.2}MB", mb)
    } else if mb >= 0.001 {
        format!("{:.1}KB", mb * 1024.0)
    } else {
        let bytes = (mb * 1024.0 * 1024.0).round() as u64;
        format!("{}B", bytes)
    }
}

// Coefficient of variation: stddev / mean. Used to flag unreliable runs.
fn cov(values: &[f64]) -> f64 {
    let m = mean(values);
    if m == 0.0 {
        return 0.0;
    }
    std_dev(values) / m
}

fn print_stats_report(
    stats: &HashMap<String, BenchmarkStats>,
    run_timestamp: &str,
    logger_count: usize,
) {
    let sep = "─".repeat(65);
    println!("\n{sep}");
    println!("  logmark  {run_timestamp}");
    println!("  {logger_count} loggers × 3 targets × {NUM_RUNS} runs × {ITERATIONS} iterations");
    println!("{sep}");

    // Write detailed per-run file (all entries, sorted alphabetically).
    let mut detail = fs::File::create("benchmark_results/detailed_stats.txt").unwrap();
    writeln!(detail, "logmark  {run_timestamp}").unwrap();
    writeln!(
        detail,
        "{logger_count} loggers × 3 targets × {NUM_RUNS} runs × {ITERATIONS} iterations\n"
    )
    .unwrap();
    let mut all_names: Vec<&String> = stats.keys().collect();
    all_names.sort();
    for name in &all_names {
        let stat = &stats[*name];
        let c = cov(&stat.elapsed_times);
        let flag = if c > 0.25 { " [!]" } else { "" };
        write!(detail, "{:<26}", name).unwrap();
        for (i, t) in stat.elapsed_times.iter().enumerate() {
            write!(detail, "  run{}={:.4}s", i + 1, t).unwrap();
        }
        writeln!(
            detail,
            "  median={:.4}s  ±{:.0}%{}  heap=+{}",
            median(&stat.elapsed_times),
            c * 100.0,
            flag,
            fmt_mem(median(&stat.memory_usages)),
        )
        .unwrap();
    }

    // Console tables and per-target summary files, grouped by target.
    for target in &["sink", "stdout", "file"] {
        let mut group: Vec<(&str, &BenchmarkStats)> = stats
            .iter()
            .filter_map(|(name, stat)| {
                name.rsplit_once('_')
                    .filter(|(_, t)| t == target)
                    .map(|(logger, _)| (logger, stat))
            })
            .collect();

        if group.is_empty() {
            continue;
        }

        // Fastest first.
        group.sort_by(|(_, a), (_, b)| {
            median(&b.ops_rates)
                .partial_cmp(&median(&a.ops_rates))
                .unwrap()
        });

        let best_ops = median(&group[0].1.ops_rates);

        println!("\n  ── {} ", target.to_uppercase());
        println!(
            "  {:>2}  {:<16}  {:>12}  {:>11}  {:>7}  {}",
            "#", "logger", "median ops/s", "median time", "vs best", "var"
        );
        println!(
            "  {:>2}  {:<16}  {:>12}  {:>11}  {:>7}  {}",
            "", "────────────────", "────────────", "───────────", "───────", "───"
        );

        let mut any_flagged = false;
        for (rank, (logger, stat)) in group.iter().enumerate() {
            let med_ops = median(&stat.ops_rates);
            let med_time = median(&stat.elapsed_times);
            let vs = best_ops / med_ops;
            let c = cov(&stat.elapsed_times);
            let flag = if c > 0.25 { " [!]" } else { "" };
            if c > 0.25 {
                any_flagged = true;
            }
            println!(
                "  {:>2}  {:<16}  {:>12}  {:>11}  {:>7}  ±{:.0}%{}",
                rank + 1,
                logger,
                fmt_ops(med_ops),
                format!("{:.4}s", med_time),
                format!("{:.1}×", vs),
                c * 100.0,
                flag,
            );
        }

        if any_flagged {
            println!("      [!] CoV > 25% — high variance; median is more reliable than mean");
        }

        // Per-target summary file mirrors the console table.
        let path = format!("benchmark_results/{target}_summary.txt");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "logmark  {target} target  {run_timestamp}").unwrap();
        writeln!(
            f,
            "{logger_count} loggers × {NUM_RUNS} runs × {ITERATIONS} iterations\n"
        )
        .unwrap();
        writeln!(
            f,
            "  {:>2}  {:<16}  {:>12}  {:>11}  {:>7}  {}",
            "#", "logger", "median ops/s", "median time", "vs best", "var"
        )
        .unwrap();
        writeln!(
            f,
            "  {:>2}  {:<16}  {:>12}  {:>11}  {:>7}  {}",
            "", "────────────────", "────────────", "───────────", "───────", "───"
        )
        .unwrap();
        for (rank, (logger, stat)) in group.iter().enumerate() {
            let med_ops = median(&stat.ops_rates);
            let med_time = median(&stat.elapsed_times);
            let vs = best_ops / med_ops;
            let c = cov(&stat.elapsed_times);
            let flag = if c > 0.25 { " [!]" } else { "" };
            writeln!(
                f,
                "  {:>2}  {:<16}  {:>12}  {:>11}  {:>7}  ±{:.0}%{}",
                rank + 1,
                logger,
                fmt_ops(med_ops),
                format!("{:.4}s", med_time),
                format!("{:.1}×", vs),
                c * 100.0,
                flag,
            )
            .unwrap();
        }
    }

    println!("\nResults written to benchmark_results/");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 3 && args[1] == "--benchmark" {
        let _ = run_individual_benchmark(&args[2], &args[3]);
        exit(0);
    }

    let benchmarks = vec![
        // Sync — run on the caller's thread
        "env_logger",
        "fern",
        "slog",
        "tracing",
        // Async — internal worker thread
        "slog_async",
        "tracing_async",
        "winston",
    ];

    let targets = OutputTarget::all();
    let run_timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    println!(
        "logmark  {}  {} loggers × {} targets × {} warmup + {} runs",
        run_timestamp,
        benchmarks.len(),
        targets.len(),
        NUM_WARMUP_RUNS,
        NUM_RUNS
    );

    let stats = run_benchmarks_in_processes(&benchmarks, &targets, NUM_RUNS);
    print_stats_report(&stats, &run_timestamp, benchmarks.len());
}
