use crate::fields::{
    FieldConfig, FieldSpec,
};
use crate::format::{
    DefaultSpanFormat, FormatEvent, FormatSpan, SerializableSpanList, write_extension_fields,
};
use crate::{DisplayLevelFilter, LoggerName};
use serde::ser::{Error, SerializeMap};
use serde::{Serialize, Serializer};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::Arc;
use tracing_core::field::{Field, Visit};
use tracing_core::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use crate::span_recorder::DefaultSpanRecorder;

pub struct LogstashFormat<SF = DefaultSpanFormat> {
    display_version: bool,
    display_timestamp: bool,
    display_logger_name: Option<LoggerName>,
    display_thread_name: bool,
    display_level: bool,
    display_level_value: bool,
    display_span_list: Option<DisplayLevelFilter>,
    display_stack_trace: Option<DisplayLevelFilter>,
    span_format: SF,
    span_fields: Arc<FieldConfig>,
}

const fn level_value(level: &Level) -> u64 {
    match *level {
        Level::ERROR => 3,
        Level::WARN => 4,
        Level::INFO => 5,
        Level::TRACE => 6,
        Level::DEBUG => 7,
    }
}

impl<SF> LogstashFormat<SF> {
    pub fn with_timestamp(self, display_timestamp: bool) -> Self {
        Self {
            display_timestamp,
            ..self
        }
    }
    pub fn with_version(self, display_version: bool) -> Self {
        Self {
            display_version,
            ..self
        }
    }
    pub fn with_logger_name(self, display_logger_name: Option<LoggerName>) -> Self {
        Self {
            display_logger_name,
            ..self
        }
    }
    pub fn with_thread_name(self, display_thread_name: bool) -> Self {
        Self {
            display_thread_name,
            ..self
        }
    }
    pub fn with_level(self, display_level: bool) -> Self {
        Self {
            display_level,
            ..self
        }
    }
    pub fn with_level_value(self, display_level_value: bool) -> Self {
        Self {
            display_level_value,
            ..self
        }
    }
    pub fn with_span_list(self, display_span_list: Option<DisplayLevelFilter>) -> Self {
        Self {
            display_span_list,
            ..self
        }
    }
    pub fn with_stack_trace(self, display_stack_trace: Option<DisplayLevelFilter>) -> Self {
        Self {
            display_stack_trace,
            ..self
        }
    }
    pub fn with_span_fields(self, span_fields: Vec<FieldSpec>) -> Self {
        Self {
            span_fields: Arc::new(FieldConfig::new(span_fields)),
            ..self
        }
    }
    pub fn span_format<FS2>(self, span_format: FS2) -> LogstashFormat<FS2> {
        LogstashFormat {
            display_version: self.display_version,
            display_timestamp: self.display_timestamp,
            display_logger_name: self.display_logger_name,
            display_thread_name: self.display_thread_name,
            display_level: self.display_level,
            display_stack_trace: self.display_stack_trace,
            display_level_value: self.display_level_value,
            display_span_list: self.display_span_list,
            span_format,
            span_fields: self.span_fields,
        }
    }
}

impl Default for LogstashFormat {
    fn default() -> Self {
        Self {
            display_version: true,
            display_timestamp: true,
            display_logger_name: Some(LoggerName::Event),
            display_thread_name: true,
            display_level: true,
            display_level_value: true,
            display_stack_trace: None,
            display_span_list: None,
            span_format: Default::default(),
            span_fields: Default::default(),
        }
    }
}

fn format_stack_trace<SS>(
    event: &Event<'_>,
    ctx: &Context<'_, SS>,
    filter: DisplayLevelFilter,
) -> String
where
    SS: Subscriber + for<'a> LookupSpan<'a>,
{
    fn append_line(stack_trace: &mut String, metadata: &Metadata<'_>) {
        writeln!(
            stack_trace,
            "  at {}({}:{})",
            metadata.target(),
            metadata.file().unwrap_or("<unknown>"),
            metadata.line().unwrap_or(0)
        )
        .unwrap();
    }
    let mut stack_trace = String::new();
    if let Some(scope) = ctx.event_scope(event) {
        for span in scope.from_root() {
            let span_metadata = span.metadata();
            if filter.is_enabled(event, span_metadata.level()) {
                append_line(&mut stack_trace, span_metadata);
            }
        }
    }
    let event_metadata = event.metadata();
    if filter.is_enabled(event, event_metadata.level()) {
        append_line(&mut stack_trace, event_metadata);
    }
    if !stack_trace.is_empty() {
        stack_trace.truncate(stack_trace.len() - 1);
    }
    stack_trace
}

const RESERVED_NAMES: [&str; 8] = [
    "@version",
    "@timestamp",
    "thread_name",
    "logger_name",
    "level",
    "level_value",
    "stack_trace",
    "spans",
];

struct SerializeSpanName<'c, SS>(&'c Event<'c>, &'c Context<'c, SS>);

impl<'c, SS> Serialize for SerializeSpanName<'c, SS> where SS: Subscriber + for<'a> LookupSpan<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if let Some(span_metadata) = self.1.current_span().metadata() {
            let name = format!("{}::{}", span_metadata.target(), span_metadata.name());
            serializer.serialize_str(&name)
        }
        else {
            serializer.serialize_str(self.0.metadata().target())
        }
    }
}

impl<FS> FormatEvent for LogstashFormat<FS>
where
    FS: FormatSpan,
{
    type R = DefaultSpanRecorder;

    fn span_recorder(&self) -> Self::R {
        DefaultSpanRecorder::from_config(self.span_fields.clone())
    }

    fn format_event<S: Serializer, SS: Subscriber + for<'a> LookupSpan<'a>>(
        &self,
        serializer: S,
        event: &Event<'_>,
        ctx: Context<'_, SS>,
    ) -> Result<S::Ok, S::Error> {
        let event_metadata = event.metadata();
        let event_level = event_metadata.level();

        let mut s = serializer.serialize_map(None)?;
        if self.display_version {
            s.serialize_entry("@version", "1")?;
        }

        if self.display_timestamp {
            s.serialize_entry("@timestamp", &LogTimestamp::default())?;
        }

        if self.display_thread_name {
            let thread = std::thread::current();
            if let Some(name) = thread.name() {
                s.serialize_entry("thread_name", name)?;
            }
        }

        if let Some(l) = self.display_logger_name {
            match l {
                LoggerName::Event => s.serialize_entry("logger_name", event_metadata.target())?,
                LoggerName::Span => s.serialize_entry("logger_name", &SerializeSpanName(event, &ctx))?
            };
        }

        if self.display_level {
            s.serialize_entry("level", event_level.as_str())?;
        }

        if self.display_level_value {
            s.serialize_entry("level_value", &level_value(event_level))?;
        }

        if let Some(filter) = self.display_stack_trace {
            s.serialize_entry("stack_trace", &format_stack_trace(event, &ctx, filter))?;
        }

        if let Some(filter) = self.display_span_list {
            s.serialize_entry(
                "spans",
                &SerializableSpanList(&self.span_format, event, &ctx, filter),
            )?;
        }

        let mut seen = HashSet::from(RESERVED_NAMES);

        let mut field_visitor = SerializingFieldVisitor {
            serializer: &mut s,
            field_name_filter: |name| seen.insert(name),
            status: None,
        };

        event.record(&mut field_visitor);
        if let Some(e) = field_visitor.status {
            return Err(e);
        }

        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(span_fields) = span.extensions().get::<DefaultSpanRecorder>() {
                    write_extension_fields(&mut seen, &mut s, span_fields)?;
                }
            }
        }
        s.end()
    }
}

struct SerializingFieldVisitor<'a, F, S, E> {
    field_name_filter: F,
    serializer: &'a mut S,
    status: Option<E>,
}

impl<'a, S: SerializeMap, F: FnMut(&'static str) -> bool>
    SerializingFieldVisitor<'a, F, S, S::Error>
{
    fn record_field<V: ?Sized + Serialize>(&mut self, field: &Field, value: &V) {
        if self.status.is_none() && (self.field_name_filter)(field.name()) {
            if let Err(e) = self.serializer.serialize_entry(field.name(), &value) {
                self.status = Some(e)
            }
        }
    }
}

impl<'a, F: FnMut(&'static str) -> bool, S: SerializeMap> Visit
    for SerializingFieldVisitor<'a, F, S, S::Error>
{
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_field(field, &value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_field(field, &value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_field(field, &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_field(field, &value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_field(field, value);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.record_field(field, &format!("{}", value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record_field(field, &format!("{:?}", value));
    }
}

struct LogTimestamp(time::OffsetDateTime);

impl Default for LogTimestamp {
    fn default() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }
}

impl Serialize for LogTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self
            .0
            .format(&time::format_description::well_known::Rfc3339)
        {
            Ok(s) => serializer.serialize_str(&s),
            Err(e) => Err(S::Error::custom(e)),
        }
    }
}
