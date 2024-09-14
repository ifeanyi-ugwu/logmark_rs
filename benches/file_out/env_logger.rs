use criterion::{criterion_group, criterion_main, Criterion};
use env_logger;
use log::info;
use std::fs::File;

fn bench_env_logger(c: &mut Criterion) {
    let mut group = c.benchmark_group("env_logger_file");
    group.sample_size(10);

    let file = File::create("env_logger_output.log").unwrap();
    std::env::set_var("RUST_LOG", "info");
    env_logger::builder()
        .target(env_logger::Target::Pipe(Box::new(file)))
        .try_init()
        .expect("Failed to initialize env_logger");

    group.bench_function("log_message_to_file", |b| {
        b.iter(|| {
            info!("A logging message that is reasonably long");
        });
    });

    group.finish();
}

criterion_group!(benches, bench_env_logger);
criterion_main!(benches);
