use std::sync::Arc;
use tracing_core::Event;
use tracing_core::field::Field;
use crate::fields::{FieldConfig, FieldRecorder, FieldVisitor, RecordedValue, TryForEachField};

pub trait EventRecorder {
    fn record_event(&mut self, event: &Event<'_>);
}

pub struct DefaultEventRecorder {
    config: Arc<FieldConfig>,
    fields: Vec<RecordedValue>,
}

impl TryForEachField for DefaultEventRecorder {
    fn try_for_each<E, F: FnMut(&'static str, &RecordedValue) -> Result<(), E>>(
        &self,
        mut f: F,
    ) -> Result<(), E> {
        for (name, value) in self.config.event_field_names.iter().zip(self.fields.iter()) {
            f(*name, value)?;
        }
        Ok(())
    }
}

impl EventRecorder for DefaultEventRecorder {
    fn record_event(&mut self, event: &Event<'_>) {
        event.record(&mut FieldVisitor::new(self))
    }
}

impl DefaultEventRecorder {
    pub fn from_config(config: Arc<FieldConfig>) -> Self {
        let n = config.event_field_index.len();
        Self {
            config,
            fields: vec![RecordedValue::Unset; n],
        }
    }
}

impl FieldRecorder for DefaultEventRecorder {
    fn record_field(&mut self, field: &Field, value: impl Into<RecordedValue>) {
        if let Some(i) = self.config.event_field_index(field) {
            self.fields[i] = value.into();
        }
    }
}
