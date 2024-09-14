use criterion::{criterion_group, criterion_main, Criterion};
use winston::Logger as WinstonLogger;

fn bench_winston_rust(c: &mut Criterion) {
    let mut group = c.benchmark_group("winston_rust");
    group.sample_size(10);

    group.bench_function("log_message_to_file", |b| {
        let file_transport = winston::transports::File::builder()
            .filename("winston_output.log")
            .build();
        let logger = WinstonLogger::builder()
            .add_transport(file_transport)
            .build();
        b.iter(|| {
            logger.info("A logging message that is reasonably long");
        })
    });
    group.finish();
}

criterion_group!(benches, bench_winston_rust);
criterion_main!(benches);
