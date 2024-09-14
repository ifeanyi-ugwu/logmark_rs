use criterion::{criterion_group, criterion_main, Criterion};
use slog::{Drain, Logger};
use std::fs::File;

fn bench_slog(c: &mut Criterion) {
    let mut group = c.benchmark_group("slog");
    group.sample_size(10);

    group.bench_function("log_message_to_file", |b| {
        let file = File::create("slog_output.log").unwrap();
        let decorator = slog_term::PlainSyncDecorator::new(file);
        let drain = slog_async::Async::new(slog_term::FullFormat::new(decorator).build().fuse())
            .chan_size(3_000_000) //this was through trial and error as some messages was being dropped, it was tricky to get because the more I increased it, the faster it became(at the expense of memory usage) and more message it logged. 3_000_000 was the number i settled for because that was when no messages was dropped
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
