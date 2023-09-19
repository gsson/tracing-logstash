use std::{
    io::{self, Read, Write},
    sync::{Arc, RwLock},
};
use time::format_description::well_known::Rfc3339;
use tracing_subscriber::{
    fmt::writer::BoxMakeWriter, prelude::__tracing_subscriber_SubscriberExt, Registry,
};

#[derive(Default, Clone)]
struct Buffer {
    inner: Arc<RwLock<Vec<u8>>>,
}

impl Buffer {
    fn new(inner: Arc<RwLock<Vec<u8>>>) -> Self {
        Self { inner }
    }
}

impl Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.write().unwrap();
        inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.write().unwrap();
        inner.flush()
    }
}

impl Read for Buffer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let inner = self.inner.read().unwrap();
        buf.copy_from_slice(&inner);
        Ok(inner.len())
    }
}

#[test]
fn simple_log_format() {
    let shared = Arc::new(RwLock::new(Vec::new()));
    let cloned = shared.clone();
    let writer = BoxMakeWriter::new(move || Buffer::new(cloned.clone()));

    let logger = tracing_logstash::Layer::default()
        .event_format(
            tracing_logstash::logstash::LogstashFormat::default().with_constants(vec![
                ("service.name", "tracing-logstash".to_owned()),
                ("service.environment", "testing".to_owned()),
            ]),
        )
        .with_writer(writer);

    let collector = Registry::default().with(logger);

    tracing::subscriber::set_global_default(collector).unwrap();

    tracing::info!("test");

    let output = String::from_utf8(shared.read().unwrap().to_vec()).unwrap();
    let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();

    let expected_json = serde_json::json!({
        "@version": "1",
        "@timestamp": output_json["@timestamp"],
        "thread_name": "simple_log_format",
        "logger_name": "output",
        "level": "INFO",
        "level_value": 5,
        "service.name": "tracing-logstash",
        "service.environment": "testing",
        "message": "test",
    });

    assert_eq!(output_json, expected_json);

    // assert that output_json["@timestamp"] is a valid timestamp
    time::OffsetDateTime::parse(output_json["@timestamp"].as_str().unwrap(), &Rfc3339).unwrap();
}
