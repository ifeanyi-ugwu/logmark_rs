use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::process::Command;
use std::time::Duration;

// Import the logging libraries
use env_logger;
use log::{info, LevelFilter};
use slog::{debug, o, Drain};
use tracing::{self, event, Level};

fn reset_env_logger() {
    env_logger::builder().is_test(true).try_init().ok();
    log::set_max_level(LevelFilter::Info);
}

fn reset_slog() -> slog::Logger {
    let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
    slog::Logger::root(slog_term::FullFormat::new(plain).build().fuse(), o!())
}

fn reset_tracing() {
    let subscriber = tracing_subscriber::fmt().with_test_writer().finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");
}

fn benchmark_logger(logger: &str, message: &str) -> Duration {
    let output = Command::new(std::env::current_exe().unwrap())
        .arg(logger)
        .arg(message)
        .output()
        .expect("Failed to execute benchmark");

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let nanos: u64 = duration_str.trim().parse().unwrap();
    Duration::from_nanos(nanos)
}

fn run_individual_benchmark(logger: &str, message: &str) {
    let start = std::time::Instant::now();
    match logger {
        "env_logger" => {
            reset_env_logger();
            for _ in 0..1000 {
                info!(target: "benchmark", "{}", message);
            }
        }
        "slog" => {
            let root = reset_slog();
            for _ in 0..1000 {
                debug!(root, "{}", message);
            }
        }
        "tracing" => {
            reset_tracing();
            for _ in 0..1000 {
                event!(Level::INFO, "{}", message);
            }
        }
        _ => panic!("Unknown logger: {}", logger),
    }
    let duration = start.elapsed();
    println!("{}", duration.as_nanos());
}

fn benchmark_loggers(c: &mut Criterion) {
    let message = "A logging message that is reasonably long";

    let mut group = c.benchmark_group("loggers");
    group.sample_size(10); // Reduce sample size as we're running full processes

    for logger in &["env_logger", "slog", "tracing"] {
        group.bench_function(logger.to_string(), |b| {
            b.iter(|| benchmark_logger(black_box(logger), black_box(message)))
        });
    }

    group.finish();
}

criterion_group!(benches, benchmark_loggers);
//criterion_main!(benches);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 2 {
        run_individual_benchmark(&args[1], &args[2]);
    } else {
        benches();
    }
}
