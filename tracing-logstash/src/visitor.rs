use std::fmt;

use crate::LogState;
use serde_json::Value;
use tracing_core::field::{Field, Visit};

pub struct FieldVisitor<'a> {
    state: &'a mut LogState,
}

impl<'a> FieldVisitor<'a> {
    pub(crate) fn new(state: &'a mut LogState) -> Self {
        Self { state }
    }

    fn record_field<T: ToString + Into<Value>>(&mut self, field: &Field, value: T) {
        self.state.insert_field(field, value);
    }
}

impl<'a> Visit for FieldVisitor<'a> {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_field(field, value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_field(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_field(field, value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_field(field, value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_field(field, format!("{:#?}", value));
    }
}
