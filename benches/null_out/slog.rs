use criterion::{criterion_group, criterion_main, Criterion};
use slog::{Drain, Logger};
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

fn bench_slog(c: &mut Criterion) {
    let mut group = c.benchmark_group("slog");
    group.sample_size(10);

    group.bench_function("log message", |b| {
        // let decorator = slog_term::PlainSyncDecorator::new(std::io::sink());
        let decorator = slog_term::PlainSyncDecorator::new(NullWriter);
        let drain = slog_async::Async::new(slog_term::FullFormat::new(decorator).build().fuse())
            .build()
            .fuse();
        let logger = Logger::root(drain, slog::o!());

        b.iter(|| {
            slog::info!(logger, "A logging message that is reasonably long");
        })
    });
    group.finish();
}

criterion_group!(benches, bench_slog,);
criterion_main!(benches);
