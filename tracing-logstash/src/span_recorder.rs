use tracing_core::field::Field;
use tracing_core::span::{Attributes, Record};
use std::sync::Arc;
use crate::fields::{FieldConfig, FieldRecorder, FieldVisitor, RecordedValue, TryForEachField};

pub trait SpanRecorder {
    fn record_span(&mut self, attrs: &Attributes<'_>);
    fn merge(&mut self, record: &Record<'_>);
}

pub struct DefaultSpanRecorder {
    config: Arc<FieldConfig>,
    fields: Vec<RecordedValue>,
}

impl SpanRecorder for DefaultSpanRecorder {
    fn record_span(&mut self, attrs: &Attributes<'_>) {
        attrs.record(&mut FieldVisitor::new(self))
    }

    fn merge(&mut self, record: &Record<'_>) {
        record.record(&mut FieldVisitor::new(self))
    }
}

impl TryForEachField for DefaultSpanRecorder {
    fn try_for_each<E, F: FnMut(&'static str, &RecordedValue) -> Result<(), E>>(
        &self,
        mut f: F,
    ) -> Result<(), E> {
        for (name, value) in self.config.span_field_names.iter().zip(self.fields.iter()) {
            f(*name, value)?;
        }
        Ok(())
    }
}

impl FieldRecorder for DefaultSpanRecorder {
    fn record_field(&mut self, field: &Field, value: impl Into<RecordedValue>) {
        if let Some(i) = self.config.field_index(field) {
            self.fields[i] = value.into();
        }
    }
}

impl DefaultSpanRecorder {
    pub fn from_config(config: Arc<FieldConfig>) -> Self {
        let n = config.span_field_index.len();
        Self {
            config,
            fields: vec![RecordedValue::None; n],
        }
    }
}
