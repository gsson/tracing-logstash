use serde::{Serialize, Serializer};
use std::collections::HashMap;
use tracing_core::field::{Field, Visit};

enum FieldSourceFilter {
    SpanOrEvent,
    Event,
}

enum FieldSource {
    Copy(FieldSourceFilter, &'static str),
    Translate(
        FieldSourceFilter,
        &'static str,
        Box<dyn Fn(RecordedValue) -> RecordedValue>,
    ),
    Static(RecordedValue),
    Dynamic(Box<dyn Fn() -> RecordedValue>),
}

impl FieldSource {
    fn records_span(&self) -> bool {
        matches!(
            self,
            FieldSource::Copy(FieldSourceFilter::SpanOrEvent, _)
                | FieldSource::Translate(FieldSourceFilter::SpanOrEvent, _, _)
        )
    }
    fn records_event(&self) -> bool {
        matches!(
            self,
            FieldSource::Copy(FieldSourceFilter::Event, _)
                | FieldSource::Translate(FieldSourceFilter::Event, _, _)
                | FieldSource::Copy(FieldSourceFilter::SpanOrEvent, _)
                | FieldSource::Translate(FieldSourceFilter::SpanOrEvent, _, _)
        )
    }
}

pub struct FieldSpec(&'static str, FieldSource);

impl From<&'static str> for FieldSpec {
    fn from(name: &'static str) -> Self {
        FieldSpec(
            name,
            FieldSource::Copy(FieldSourceFilter::SpanOrEvent, name),
        )
    }
}

impl From<(&'static str, &'static str)> for FieldSpec {
    fn from((to, from): (&'static str, &'static str)) -> Self {
        FieldSpec(to, FieldSource::Copy(FieldSourceFilter::SpanOrEvent, from))
    }
}

pub struct FieldConfig {
    pub span_field_index: HashMap<&'static str, usize>,
    pub span_field_names: Vec<&'static str>,
    pub event_field_index: HashMap<&'static str, usize>,
    pub event_field_names: Vec<&'static str>,
}

impl Default for FieldConfig {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl FieldConfig {
    pub fn new(fields: Vec<FieldSpec>) -> Self {
        let span_field_index = fields
            .iter()
            .filter(|f| f.1.records_span())
            .enumerate()
            .map(|(i, f)| (f.0, i))
            .collect::<HashMap<&'static str, usize>>();
        let mut span_field_names = vec![""; span_field_index.len()];
        for (name, i) in &span_field_index {
            span_field_names[*i] = name;
        }

        let event_field_index = fields
            .iter()
            .filter(|f| f.1.records_event())
            .enumerate()
            .map(|(i, f)| (f.0, i))
            .collect::<HashMap<&'static str, usize>>();
        let mut event_field_names = vec![""; event_field_index.len()];
        for (name, i) in &event_field_index {
            event_field_names[*i] = name;
        }

        Self {
            span_field_index,
            span_field_names,
            event_field_index,
            event_field_names,
        }
    }

    pub fn field_index(&self, field: &Field) -> Option<usize> {
        self.span_field_index.get(field.name()).copied()
    }

    pub fn event_field_index(&self, field: &Field) -> Option<usize> {
        self.event_field_index.get(field.name()).copied()
    }
}

pub trait TryForEachField {
    fn try_for_each<E, F: FnMut(&'static str, &RecordedValue) -> Result<(), E>>(
        &self,
        f: F,
    ) -> Result<(), E>;
}

pub trait FieldRecorder {
    fn record_field(&mut self, field: &Field, value: impl Into<RecordedValue>);
}

pub struct FieldVisitor<'a, R> {
    recorder: &'a mut R,
}

impl<'a, R> FieldVisitor<'a, R> {
    pub fn new(fields: &'a mut R) -> Self {
        Self { recorder: fields }
    }
}

impl<'a, R: FieldRecorder> Visit for FieldVisitor<'a, R> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.recorder.record_field(field, value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.recorder.record_field(field, value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.recorder.record_field(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.recorder.record_field(field, value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.recorder.record_field(field, value);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.recorder.record_field(field, format!("{}", value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.recorder.record_field(field, format!("{:#?}", value));
    }
}

#[derive(Clone, Debug)]
pub enum RecordedValue {
    Unset,
    None,
    F64(f64),
    I64(i64),
    U64(u64),
    Bool(bool),
    String(String),
}

impl RecordedValue {
    pub fn is_unset(&self) -> bool {
        matches!(self, RecordedValue::Unset)
    }
}

impl Serialize for RecordedValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            RecordedValue::None | RecordedValue::Unset => serializer.serialize_none(),
            RecordedValue::F64(v) => serializer.serialize_f64(*v),
            RecordedValue::I64(v) => serializer.serialize_i64(*v),
            RecordedValue::U64(v) => serializer.serialize_u64(*v),
            RecordedValue::Bool(v) => serializer.serialize_bool(*v),
            RecordedValue::String(v) => serializer.serialize_str(v),
        }
    }
}

impl From<f64> for RecordedValue {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl From<i64> for RecordedValue {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}

impl From<u64> for RecordedValue {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}

impl From<bool> for RecordedValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<String> for RecordedValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for RecordedValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_owned())
    }
}
