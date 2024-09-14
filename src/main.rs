use env_logger;
use log::{info, LevelFilter};
use slog::{o, Drain, Logger};
use slog_term;
use std::time::Instant;
use tracing::{self, event, Level};

use tikv_jemalloc_ctl::{epoch, stats};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

const ITERATIONS: u32 = 10_000;
const MESSAGE: &str = "A logging message that is reasonably long";

struct BenchmarkResult {
    name: String,
    elapsed: f64,
    ops: f64,
    memory_usage: f64,
}

fn bench_env_logger() -> BenchmarkResult {
    std::env::set_var("RUST_LOG", "info");
    env_logger::builder()
        .target(env_logger::Target::Stdout)
        .try_init()
        .ok();
    log::set_max_level(LevelFilter::Info);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        info!(target: "benchmark", "{} {}", MESSAGE,"env_logger");
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "env_logger".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_slog() -> BenchmarkResult {
    let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
    let root = Logger::root(slog_term::FullFormat::new(plain).build().fuse(), o!());

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        slog::info!(root, "{} {}", MESSAGE, "slog");
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "slog".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_slog_async() -> BenchmarkResult {
    let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
    let drain = slog_async::Async::new(slog_term::FullFormat::new(plain).build().fuse())
        //.chan_size(1024 * 9)
        .chan_size(8_991) //8_991 is for 10_000 messages, multiply by ten if the number of messages is multiplied by 10
        .build()
        .fuse();
    let root = Logger::root(drain, o!());

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        slog::info!(root, "{} {}", MESSAGE, "slog_async");
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "slog_async".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_slog_async_block() -> BenchmarkResult {
    let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());

    // Adjusting channel size and blocking strategy to avoid dropped messages
    let drain = slog_async::Async::new(slog_term::FullFormat::new(plain).build().fuse())
        //.chan_size(1024) // Increase channel size
        .overflow_strategy(slog_async::OverflowStrategy::Block) // Block instead of dropping messages
        .build()
        .fuse();

    let root = Logger::root(drain, o!());

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        slog::info!(root, "{} {}", MESSAGE, "slog_async_block");
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "slog_async_block".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_tracing() -> BenchmarkResult {
    let subscriber = tracing_subscriber::fmt().with_test_writer().finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        event!(Level::INFO, "{} {}", MESSAGE, "tracing");
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "tracing".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_winston() -> BenchmarkResult {
    let logger = winston::Logger::builder()
        .add_transport(winston::transports::Console::new(None))
        .format(winston::format::combine(vec![
            winston::format::timestamp(),
            winston::format::printf(|info: &winston::format::LogInfo| {
                format!(
                    "{} - {}: {}",
                    info.get_meta("timestamp").unwrap(),
                    info.level.to_uppercase(),
                    info.message,
                )
            }),
            //winston::format::json(),
        ]))
        .build();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        logger.info(&format!("{} {}", MESSAGE, "winston"));
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "winston".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_winston_global() -> BenchmarkResult {
    let options = winston::LoggerOptions::new()
        .add_transport(winston::transports::Console::new(None))
        .format(winston::format::combine(vec![
            winston::format::timestamp(),
            winston::format::json(),
        ]));

    winston::configure(Some(options));

    /*let mut logger = winston::Logger::default();
    logger.close();*/
    //let logger = winston::Logger::default();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        winston::log_info!("{} {}", MESSAGE, "winston_global");
        //logger.info(&format!("{} {}", MESSAGE, "winston_global"));
    }
    winston::Logger::shutdown();
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "winston_global".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_winston_v2() -> BenchmarkResult {
    winston::logger_v2::transport::initialize_runtime();
    let console_transport = winston::logger_v2::transport::create_console_transport();
    let logger = winston::logger_v2::Logger::new(console_transport);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        logger.log(&format!("{} {}", MESSAGE, "winston_v2"));
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "winston_v2".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn bench_winston_v3() -> BenchmarkResult {
    let console_transport =
        winston::logger_v3::transports::transport::Transport::new(std::io::stdout(), None);

    /*let logger = winston::logger_v3::Logger::builder()
            .level("info")
            .format(winston::format::combine(vec![
                winston::format::timestamp(),
                winston::format::printf(|info: &winston::format::LogInfo| {
                    format!(
                        "{} - {}: {}",
                        info.get_meta("timestamp").unwrap(),
                        info.level.to_uppercase(),
                        info.message,
                    )
                }),
                // winston::format::json(),
            ]))
            .transports(vec![console_transport])
            .build();
    */
    /*let mut logger = winston::Logger::default();
    logger.close();*/
    //let logger = winston::Logger::default();
    let mut logger = winston::logger_v3::Logger::new(None);
    logger.add_transport(console_transport);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        // logger.info(&(MESSAGE.to_owned() + " winston_v3"));
        logger.info(&format!("{} {}", MESSAGE, "winston_v3"));
    }
    let elapsed = start.elapsed();

    // Capture memory usage
    epoch::advance().unwrap();
    let allocated = stats::allocated::read().unwrap();

    BenchmarkResult {
        name: "winston_v3".to_string(),
        elapsed: elapsed.as_secs_f64(),
        ops: ITERATIONS as f64 / elapsed.as_secs_f64(),
        memory_usage: allocated as f64 / 1024.0 / 1024.0, // Convert to MB
    }
}

fn main() {
    let benchmarks = vec![
        //bench_env_logger,
        //bench_slog,
        bench_slog_async,
        // bench_slog_async_block,
        //bench_tracing,
        bench_winston,
        // bench_winston_global,
        //bench_winston_v2,
        bench_winston_v3,
    ];
    //let benchmarks = vec![bench_slog_async_block];
    let mut results = Vec::new();

    for bench in benchmarks {
        let result = bench();
        println!("Finished benchmarking: \"{}\"", result.name);
        println!("  Elapsed: {:.4} seconds", result.elapsed);
        println!("  Ops/sec: {:.4}", result.ops);
        println!("  Memory Usage: {:.4} MB", result.memory_usage);
        results.push(result);
    }

    results.sort_by(|a, b| b.ops.partial_cmp(&a.ops).unwrap());
    println!("\nFastest is {}", results[0].name);

    for i in 0..results.len() {
        for j in i + 1..results.len() {
            let faster = &results[i];
            let slower = &results[j];
            let ratio = faster.ops / slower.ops;
            println!(
                "  {} is {:.4}x faster than {}",
                faster.name, ratio, slower.name
            );
        }
    }

    println!("\nMemory Usage Comparison:");

    // Compare memory allocated
    results.sort_by(|a, b| a.memory_usage.partial_cmp(&b.memory_usage).unwrap());
    println!("\nLowest Memory Allocated is {}", results[0].name);

    for i in 0..results.len() {
        for j in i + 1..results.len() {
            let lower = &results[i];
            let higher = &results[j];
            let ratio = higher.memory_usage / lower.memory_usage;
            println!(
                "  {} uses {:.4}x more memory than {}",
                higher.name, ratio, lower.name
            );
        }
    }
}
