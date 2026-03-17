use env_logger;
use env_logger::fmt::Formatter;
use log::info;
use memory_stats::memory_stats;
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

const ITERATIONS: u32 = 100_000;
const NUM_RUNS: usize = 3;
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
    let before = memory_stats().unwrap();
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        bench_fn();
    }
    let elapsed = start.elapsed();
    let after = memory_stats().unwrap();
    BenchmarkResult {
        name: name.to_string(),
        target,
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: after.physical_mem.saturating_sub(before.physical_mem) as f64
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

    run_benchmark("slog_async", target, move || {
        slog::info!(root, "{} {}", MESSAGE, "slog_async");
    })
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

    let (writer, _guard) = match target {
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

    run_benchmark("tracing_async", target, || {
        event!(Level::INFO, "{} {}", MESSAGE, "tracing_async");
    })
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

    run_benchmark("winston", target, move || {
        winston::log!(logger, info, format!("{} {}", MESSAGE, "winston"));
    })
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

    for run in 1..=runs {
        println!("\n--- Run {} of {} ---", run, runs);
        all_benchmarks.shuffle(&mut rng);

        for &(bench, target) in &all_benchmarks {
            println!("Starting benchmark: {} ({})", bench, target.as_str());

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
                            "  {} took {:.4}s, {:.0} ops/sec, {:.4} MB",
                            name, elapsed, ops, memory
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

fn print_stats_report(stats: &HashMap<String, BenchmarkStats>) {
    println!("\n===== BENCHMARK STATISTICS =====");
    println!("(across {} runs for each benchmark)\n", NUM_RUNS);

    println!(
        "{:<22} | {:<14} | {:<15} | {:<14} | {:<15}",
        "Logger", "Avg Time (s)", "Median Time (s)", "Avg Ops/sec", "Avg Memory (MB)"
    );
    println!(
        "{:-<22} | {:-<14} | {:-<15} | {:-<14} | {:-<15}",
        "", "", "", "", ""
    );

    let mut logger_names: Vec<_> = stats.keys().collect();
    logger_names.sort();

    for name in &logger_names {
        let stat = &stats[*name];
        println!(
            "{:<22} | {:<14.4} | {:<15.4} | {:<14.0} | {:<15.4}",
            name,
            mean(&stat.elapsed_times),
            median(&stat.elapsed_times),
            mean(&stat.ops_rates),
            mean(&stat.memory_usages),
        );
    }

    let mut file = fs::File::create("benchmark_results/detailed_stats.txt").unwrap();
    writeln!(file, "Detailed Benchmark Statistics").unwrap();
    writeln!(file, "==========================\n").unwrap();

    for name in stats.keys() {
        let stat = &stats[name];
        writeln!(file, "Logger: {}", name).unwrap();
        writeln!(file, "  Elapsed Time (s):").unwrap();
        writeln!(file, "    Values: {:?}", stat.elapsed_times).unwrap();
        writeln!(file, "    Mean:   {:.4}", mean(&stat.elapsed_times)).unwrap();
        writeln!(file, "    Median: {:.4}", median(&stat.elapsed_times)).unwrap();
        writeln!(file, "    StdDev: {:.4}", std_dev(&stat.elapsed_times)).unwrap();
        writeln!(file, "  Operations/sec:").unwrap();
        writeln!(file, "    Values: {:?}", stat.ops_rates).unwrap();
        writeln!(file, "    Mean:   {:.4}", mean(&stat.ops_rates)).unwrap();
        writeln!(file, "    Median: {:.4}", median(&stat.ops_rates)).unwrap();
        writeln!(file, "    StdDev: {:.4}", std_dev(&stat.ops_rates)).unwrap();
        writeln!(file, "  Memory Usage (MB):").unwrap();
        writeln!(file, "    Values: {:?}", stat.memory_usages).unwrap();
        writeln!(file, "    Mean:   {:.4}", mean(&stat.memory_usages)).unwrap();
        writeln!(file, "    Median: {:.4}", median(&stat.memory_usages)).unwrap();
        writeln!(file, "    StdDev: {:.4}", std_dev(&stat.memory_usages)).unwrap();
        writeln!(file).unwrap();
    }

    println!("\nDetailed statistics written to benchmark_results/detailed_stats.txt");
    generate_target_summaries(stats);
}

fn generate_target_summaries(stats: &HashMap<String, BenchmarkStats>) {
    let mut target_groups: HashMap<&str, Vec<(&str, &BenchmarkStats)>> = HashMap::new();

    for (name, stat) in stats {
        if let Some((logger, target)) = name.rsplit_once('_') {
            target_groups.entry(target).or_default().push((logger, stat));
        }
    }

    for (target, loggers) in target_groups {
        let mut file =
            fs::File::create(format!("benchmark_results/{}_summary.txt", target)).unwrap();
        writeln!(
            file,
            "Performance Summary for {} Target",
            target.to_uppercase()
        )
        .unwrap();
        writeln!(file, "=======================================\n").unwrap();
        writeln!(
            file,
            "{:<20} | {:<14} | {:<14} | {:<15}",
            "Logger", "Avg Time (s)", "Avg Ops/sec", "Avg Memory (MB)"
        )
        .unwrap();
        writeln!(
            file,
            "{:-<20} | {:-<14} | {:-<14} | {:-<15}",
            "", "", "", ""
        )
        .unwrap();

        let mut sorted_loggers = loggers.clone();
        sorted_loggers.sort_by(|(_, a), (_, b)| {
            mean(&a.elapsed_times)
                .partial_cmp(&mean(&b.elapsed_times))
                .unwrap()
        });

        for (logger, stat) in sorted_loggers {
            writeln!(
                file,
                "{:<20} | {:<14.4} | {:<14.0} | {:<15.4}",
                logger,
                mean(&stat.elapsed_times),
                mean(&stat.ops_rates),
                mean(&stat.memory_usages),
            )
            .unwrap();
        }

        println!(
            "Summary for {} target written to benchmark_results/{}_summary.txt",
            target, target
        );
    }
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

    println!(
        "Running each benchmark with each target {} times in randomized order...",
        NUM_RUNS
    );
    let stats = run_benchmarks_in_processes(&benchmarks, &targets, NUM_RUNS);
    print_stats_report(&stats);
}
