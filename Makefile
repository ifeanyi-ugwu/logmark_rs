# Makefile for running Rust benchmarks

# Default target
all: env_logger_null env_logger_file fern_null fern_file slog_null slog_file winston_null winston_file

# Run benchmarks for env_logger with null output
env_logger_null:
	@cargo bench --bench env_logger_null

# Run benchmarks for env_logger with file output
env_logger_file:
	@cargo bench --bench env_logger_file

# Run benchmarks for fern with null output
fern_null:
	@cargo bench --bench fern_null

# Run benchmarks for fern with file output
fern_file:
	@cargo bench --bench fern_file

# Run benchmarks for slog with null output
slog_null:
	@cargo bench --bench slog_null

# Run benchmarks for slog with file output
slog_file:
	@cargo bench --bench slog_file

# Run benchmarks for winston with null output
winston_null:
	@cargo bench --bench winston_null

# Run benchmarks for winston with file output
winston_file:
	@cargo bench --bench winston_file

.PHONY: all env_logger_null env_logger_file fern_null fern_file slog_null slog_file winston_null winston_file
