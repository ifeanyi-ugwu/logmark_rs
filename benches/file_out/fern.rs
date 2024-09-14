use criterion::{criterion_group, criterion_main, Criterion};
use fern;
use log::{info, LevelFilter};
use std::fs::File;

fn bench_fern_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("fern_file");
    group.sample_size(10);

    let file = File::create("fern_output.log").unwrap();
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(LevelFilter::Info)
        .chain(file)
        .apply()
        .expect("Failed to initialize fern logger");

    group.bench_function("log_message_to_file", |b| {
        b.iter(|| {
            info!("A logging message that is reasonably long");
        });
    });

    group.finish();
}

criterion_group!(benches, bench_fern_file);
criterion_main!(benches);
