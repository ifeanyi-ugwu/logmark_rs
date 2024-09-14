use criterion::{criterion_group, criterion_main, Criterion};
use fern;
use log::{info, LevelFilter};

fn bench_fern(c: &mut Criterion) {
    let mut group = c.benchmark_group("fern");
    group.sample_size(10);

    let _ = fern::Dispatch::new()
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
        //.chain(std::io::stdout())
        .chain(fern::Output::call(|_record| {
            // Do nothing, effectively swallowing the output
        }))
        .apply()
        .expect("Failed to initialize fern logger");

    group.bench_function("log message", |b| {
        b.iter(|| {
            info!("A logging message that is reasonably long");
        })
    });
    group.finish();
}

criterion_group!(benches, bench_fern,);
criterion_main!(benches);
