use criterion::{criterion_group, criterion_main, Criterion};
use winston::{format::LogInfo, /*transports::Transport, */ Logger as WinstonLogger};
use winston_transport::Transport;

struct NullTransport;

impl Transport for NullTransport {
    fn log(&self, _info: LogInfo) {
        // Do nothing
    }

    /*fn log(&self, _info: &str, _level: &str) {
        // Do nothing
    }*/

    fn get_level(&self) -> Option<&String> {
        None
    }

    fn get_format(&self) -> Option<&winston::format::Format> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn bench_winston_rust(c: &mut Criterion) {
    let mut group = c.benchmark_group("winston_rust");
    group.sample_size(10);

    group.bench_function("log message", |b| {
        let logger = WinstonLogger::builder()
            //.add_transport(winston::transports::Console::new(None))
            .add_transport(NullTransport)
            .build();

        b.iter(|| {
            logger.info("A logging message that is reasonably long");
        })
    });
    group.finish();
}

criterion_group!(benches, bench_winston_rust);
criterion_main!(benches);
