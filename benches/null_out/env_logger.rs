use criterion::{criterion_group, criterion_main, Criterion};
use env_logger;
use log::info;
use std::io::Write;

struct NullWriter;

impl Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn bench_env_logger(c: &mut Criterion) {
    let mut group = c.benchmark_group("env_logger");
    group.sample_size(10);

    std::env::set_var("RUST_LOG", "info");
    let _ = env_logger::builder()
        //.is_test(true)
        .target(env_logger::Target::Pipe(Box::new(NullWriter)))
        .try_init()
        .expect("Failed to initialize env logger");

    group.bench_function("log message", |b| {
        b.iter(|| {
            info!("A logging message that is reasonably long");
        })
    });
    group.finish();
}

criterion_group!(benches, bench_env_logger,);
criterion_main!(benches);
