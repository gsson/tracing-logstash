mod event_recorder;
mod fields;
pub mod format;
pub mod logstash;
mod span_recorder;

use crate::logstash::LogstashFormat;
use span_recorder::SpanRecorder;
use std::io::Write;
use std::marker::PhantomData;
use tracing_core::span::{Attributes, Id, Record};
use tracing_core::{Event, Level, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

pub struct Layer<S, E = LogstashFormat, W = fn() -> std::io::StdoutLock<'static>> {
    record_separator: Vec<u8>,
    make_writer: W,
    event_format: E,
    _inner: PhantomData<S>,
}

impl<S> Default for Layer<S> {
    fn default() -> Self {
        Self {
            record_separator: vec![b'\n'],
            make_writer: || std::io::stdout().lock(),
            event_format: Default::default(),
            _inner: Default::default(),
        }
    }
}

impl<S, E, W> Layer<S, E, W>
where
    E: format::FormatEvent + 'static,
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    pub fn record_separator(self, separator: impl Into<Vec<u8>>) -> Layer<S, E, W> {
        Layer {
            record_separator: separator.into(),
            ..self
        }
    }

    pub fn event_format<E2>(self, event_format: E2) -> Layer<S, E2, W>
    where
        E2: format::FormatEvent + 'static,
    {
        Layer {
            event_format,
            record_separator: self.record_separator,
            make_writer: self.make_writer,
            _inner: self._inner,
        }
    }

    pub fn with_writer<W2>(self, make_writer: W2) -> Layer<S, E, W2>
    where
        W2: for<'writer> MakeWriter<'writer> + 'static,
    {
        Layer {
            make_writer,
            event_format: self.event_format,
            record_separator: self.record_separator,
            _inner: self._inner,
        }
    }

    fn write_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut serializer = serde_json::Serializer::new(self.make_writer.make_writer());
        self.event_format
            .format_event(&mut serializer, event, ctx)
            .unwrap();
        let mut inner = serializer.into_inner();
        inner.write_all(&self.record_separator).unwrap();
    }
}

impl<S, E, W> tracing_subscriber::Layer<S> for Layer<S, E, W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    E: format::FormatEvent + 'static,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");

        let mut extensions = span.extensions_mut();

        if extensions.get_mut::<E::R>().is_none() {
            let mut recorder = self.event_format.span_recorder();
            recorder.record_span(attrs);

            extensions.insert(recorder);
        }
    }

    fn on_record(&self, id: &Id, record: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");
        let mut extensions = span.extensions_mut();

        if let Some(fields) = extensions.get_mut::<E::R>() {
            fields.merge(record);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        self.write_event(event, ctx);
    }
}

#[derive(Copy, Clone)]
pub enum LoggerName {
    Event,
    Span,
}

#[derive(Copy, Clone)]
pub enum DisplayLevelFilter {
    Off,
    All,
    Level(Level),
    Event,
}

impl DisplayLevelFilter {
    pub const ERROR: DisplayLevelFilter = Self::from_level(Level::ERROR);
    pub const WARN: DisplayLevelFilter = Self::from_level(Level::WARN);
    pub const INFO: DisplayLevelFilter = Self::from_level(Level::INFO);
    pub const DEBUG: DisplayLevelFilter = Self::from_level(Level::DEBUG);
    pub const TRACE: DisplayLevelFilter = Self::from_level(Level::TRACE);

    #[inline]
    const fn from_level(level: Level) -> DisplayLevelFilter {
        DisplayLevelFilter::Level(level)
    }

    #[inline]
    pub fn is_enabled(&self, event: &Event, span_level: &Level) -> bool {
        let filter_level = match self {
            DisplayLevelFilter::Level(level) => level,
            DisplayLevelFilter::Event => event.metadata().level(),
            DisplayLevelFilter::All => return true,
            DisplayLevelFilter::Off => return false,
        };
        filter_level >= span_level
    }
}
