use crate::fields::{FieldConfig, FieldSpec};
use crate::format::{
    write_extension_fields, DefaultSpanFormat, FormatEvent, FormatSpan, SerializableSpanList,
};
use crate::span_recorder::DefaultSpanRecorder;
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

/// Display options for the logstash output format
///
/// # Example
/// ```
/// # use tracing_subscriber::prelude::*;
/// #
/// let logger = tracing_logstash::Layer::default().event_format(
///    tracing_logstash::logstash::LogstashFormat::default()
///         .with_timestamp(false),
/// );
/// #
/// # let collector = tracing_subscriber::Registry::default().with(logger);
/// ```
pub struct LogstashFormat<FC = (), SF = DefaultSpanFormat> {
    display_version: bool,
    display_timestamp: bool,
    display_logger_name: Option<LoggerName>,
    display_thread_name: bool,
    display_level: bool,
    display_level_value: bool,
    display_span_list: Option<DisplayLevelFilter>,
    display_stack_trace: Option<(DisplayLevelFilter, DisplayLevelFilter)>,
    span_format: SF,
    span_fields: Arc<FieldConfig>,
    constants: Vec<(&'static str, String)>,
    field_contributor: FC,
}

/// Converts a `Level` to a numeric value.
const fn level_value(level: &Level) -> u64 {
    match *level {
        Level::ERROR => 3,
        Level::WARN => 4,
        Level::INFO => 5,
        Level::TRACE => 6,
        Level::DEBUG => 7,
    }
}

impl<FC, SF> LogstashFormat<FC, SF> {
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
    pub fn with_stack_trace(
        self,
        display_stack_trace: Option<(DisplayLevelFilter, DisplayLevelFilter)>,
    ) -> Self {
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

    /// Add dynamically generated fields to every event
    ///
    /// # Example
    /// ```
    /// # use tracing_subscriber::prelude::*;
    /// # use tracing_logstash::logstash::{LogFieldReceiver, LogFieldContributor};
    /// #
    /// struct DynamicFields;
    /// impl LogFieldContributor for DynamicFields {
    ///     fn add_fields<F>(&self, serializer: &mut F)
    ///     where
    ///         F: LogFieldReceiver,
    ///     {
    ///         serializer.add_field("string_field", "fnord");
    ///         serializer.add_field("number_field", &42);
    ///    }
    /// }
    ///
    /// let logger = tracing_logstash::Layer::default().event_format(
    ///     tracing_logstash::logstash::LogstashFormat::default()
    ///         .with_field_contributor(DynamicFields),
    /// );
    /// #
    /// # let collector = tracing_subscriber::Registry::default().with(logger);
    /// ```
    pub fn with_field_contributor<FC2>(self, field_contributor: FC2) -> LogstashFormat<FC2, SF> {
        LogstashFormat {
            display_version: self.display_version,
            display_timestamp: self.display_timestamp,
            display_logger_name: self.display_logger_name,
            display_thread_name: self.display_thread_name,
            display_level: self.display_level,
            display_stack_trace: self.display_stack_trace,
            display_level_value: self.display_level_value,
            display_span_list: self.display_span_list,
            span_format: self.span_format,
            span_fields: self.span_fields,
            constants: self.constants,
            field_contributor,
        }
    }

    /// Add a constant field to every event.
    ///
    /// # Example
    /// ```
    /// # use tracing_subscriber::prelude::*;
    /// #
    /// let logger = tracing_logstash::Layer::default().event_format(
    ///     tracing_logstash::logstash::LogstashFormat::default().with_constants(vec![
    ///         ("service.name", "tracing-logstash".to_owned()),
    ///     ]),
    /// );
    /// #
    /// # let collector = tracing_subscriber::Registry::default().with(logger);
    /// ```
    pub fn with_constants(self, constants: Vec<(&'static str, String)>) -> Self {
        Self { constants, ..self }
    }

    pub fn span_format<FS2>(self, span_format: FS2) -> LogstashFormat<FC, FS2> {
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
            constants: self.constants,
            field_contributor: self.field_contributor,
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
            constants: Default::default(),
            field_contributor: (),
        }
    }
}

fn format_stack_trace<SS>(
    event: &Event<'_>,
    ctx: &Context<'_, SS>,
    event_filter: DisplayLevelFilter,
    span_filter: DisplayLevelFilter,
) -> Option<String>
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

    let event_metadata = event.metadata();
    if !event_filter.is_enabled(event, event_metadata.level()) {
        return None;
    }

    let mut stack_trace = String::new();
    if let Some(scope) = ctx.event_scope(event) {
        for span in scope.from_root() {
            let span_metadata = span.metadata();
            if span_filter.is_enabled(event, span_metadata.level()) {
                append_line(&mut stack_trace, span_metadata);
            }
        }
    }

    append_line(&mut stack_trace, event_metadata);
    if !stack_trace.is_empty() {
        stack_trace.truncate(stack_trace.len() - 1);
    }

    Some(stack_trace)
}

struct SerializeSpanName<'c, SS>(&'c Event<'c>, &'c Context<'c, SS>);

impl<'c, SS> Serialize for SerializeSpanName<'c, SS>
where
    SS: Subscriber + for<'a> LookupSpan<'a>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(span_metadata) = self.1.current_span().metadata() {
            let name = format!("{}::{}", span_metadata.target(), span_metadata.name());
            serializer.serialize_str(&name)
        } else {
            serializer.serialize_str(self.0.metadata().target())
        }
    }
}

pub trait LogFieldContributor {
    fn add_fields<F>(&self, serializer: &mut F)
    where
        F: LogFieldReceiver;
}

impl LogFieldContributor for () {
    #[inline(always)]
    fn add_fields<F>(&self, _serializer: &mut F)
    where
        F: LogFieldReceiver,
    {
    }
}

impl<DFN, FS> FormatEvent for LogstashFormat<DFN, FS>
where
    FS: FormatSpan,
    DFN: LogFieldContributor,
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

        let mut seen = HashSet::new();

        let mut field_visitor = SerializingFieldVisitor {
            serializer: &mut s,
            field_name_filter: |name| seen.insert(name),
            status: None,
        };

        if self.display_version {
            field_visitor.add_field("@version", "1");
        }

        if self.display_timestamp {
            field_visitor.add_field("@timestamp", &LogTimestamp::default());
        }

        if self.display_thread_name {
            let thread = std::thread::current();
            if let Some(name) = thread.name() {
                field_visitor.add_field("thread_name", name);
            }
        }

        if let Some(l) = self.display_logger_name {
            match l {
                LoggerName::Event => {
                    field_visitor.add_field("logger_name", event_metadata.target())
                }
                LoggerName::Span => {
                    field_visitor.add_field("logger_name", &SerializeSpanName(event, &ctx))
                }
            };
        }

        if self.display_level {
            field_visitor.add_field("level", event_level.as_str());
        }

        if self.display_level_value {
            field_visitor.add_field("level_value", &level_value(event_level));
        }

        if let Some((event_filter, span_filter)) = self.display_stack_trace {
            if let Some(stack_trace) = format_stack_trace(event, &ctx, event_filter, span_filter) {
                field_visitor.add_field("stack_trace", &stack_trace);
            }
        }

        for (key, value) in &self.constants {
            field_visitor.add_field(key, value);
        }

        self.field_contributor.add_fields(&mut field_visitor);

        if let Some(filter) = self.display_span_list {
            field_visitor.add_field(
                "spans",
                &SerializableSpanList(&self.span_format, event, &ctx, filter),
            );
        }

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

pub trait LogFieldReceiver {
    fn add_field<V: ?Sized + Serialize>(&mut self, field: &'static str, value: &V);
}

pub struct SerializingFieldVisitor<'a, F, S, E> {
    field_name_filter: F,
    serializer: &'a mut S,
    status: Option<E>,
}

impl<'a, S: SerializeMap, F: FnMut(&'static str) -> bool>
    SerializingFieldVisitor<'a, F, S, S::Error>
{
    #[inline]
    fn record_field<V: ?Sized + Serialize>(&mut self, field: &Field, value: &V) {
        self.add_field(field.name(), value)
    }
}

impl<'a, S: SerializeMap, F: FnMut(&'static str) -> bool> LogFieldReceiver
    for SerializingFieldVisitor<'a, F, S, S::Error>
{
    fn add_field<V: ?Sized + Serialize>(&mut self, field: &'static str, value: &V) {
        if self.status.is_none() && (self.field_name_filter)(field) {
            if let Err(e) = self.serializer.serialize_entry(field, &value) {
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

#[cfg(test)]
mod test {
    use time::macros::datetime;

    #[test]
    fn test_serialize_log_timestamp() {
        let timestamp = super::LogTimestamp(datetime!(2020-01-01 00:00:00 +00:00));
        let serialized = serde_json::to_string(&timestamp).unwrap();
        assert_eq!(serialized, "\"2020-01-01T00:00:00Z\"");
    }
}
