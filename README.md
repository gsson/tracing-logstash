# tracing-logstash

The absolute minimum amount of code I could get away with for writing tracing messages/logs in the https://github.com/logstash/logstash-logback-encoder#standard-fields format.

Currently built for use in a single application with a single log consumer.

## Installation

You can include this library in your Cargo.toml file:

```toml
[dependencies]
tracing-logstash = "0.5.0"
```

## Usage

```rust
fn main() {
    let logger = tracing_logstash::Layer::default()
        .event_format(tracing_logstash::logstash::LogstashFormat::default()
            .with_constants(vec![
                ("service.name", "tracing-logstash".to_owned()),
            ])
        );

    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let collector = Registry::default().with(logger).with(env_filter);

    tracing::subscriber::set_global_default(collector).unwrap();
    
    tracing::info!("Hello, world!");
}
```

## Logstash Format Reference

https://github.com/logstash/logstash-logback-encoder#standard-fields

## Sample output (from example app)

```json
{
  "@version": "1",
  "@timestamp": "2023-09-14T09:34:17.233512Z",
  "thread_name": "tokio-runtime-worker",
  "logger_name": "hyper",
  "level": "INFO",
  "level_value": 5,
  "message": "tracing crate macro",
  "some_tag": 43,
  "service.name": "tracing-logstash"
}
```

## License

This library is distributed under the terms of the MIT License. 
See the [LICENSE](LICENSE) file for details.
