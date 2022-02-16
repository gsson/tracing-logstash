mod fields;
mod visitor;

use crate::fields::*;
use serde_json::map::Map;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tracing_core::field::Field;
use tracing_core::{
    span::{Attributes, Id, Record},
    Event, Level, Subscriber,
};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

#[derive(Clone)]
pub(crate) struct LogState {
    field_routes: Arc<FieldRouter>,
    base: Map<String, Value>,
    tags: Map<String, Value>,
}

impl LogState {
    pub fn new(field_routes: Arc<FieldRouter>) -> Self {
        Self {
            base: Map::with_capacity(5),
            tags: Map::new(),
            field_routes,
        }
    }

    fn insert_field<T: ToString + Into<Value>>(&mut self, field: &Field, value: T) {
        match self.field_routes.route(field, value) {
            Some((FieldDestination::Root, name, value)) => self.insert_base(name, value),
            Some((FieldDestination::Tag, name, value)) => self.insert_tag(name, value),
            None => {}
        }
    }

    pub fn insert_tag<V: Into<Value>>(&mut self, field_name: String, value: V) {
        self.tags.insert(field_name, value.into());
    }

    pub fn insert_base<V: Into<Value>>(&mut self, field_name: String, value: V) {
        self.base.insert(field_name, value.into());
    }

    pub fn merge(&mut self, other: &LogState) {
        self.base.extend(other.base.clone());
        self.tags.extend(other.tags.clone());
    }

    pub fn into_value(mut self) -> Value {
        self.base.insert(VERSION.into(), VERSION_1.into());
        self.base.insert(TAGS.into(), self.tags.into());
        self.base
            .entry(MESSAGE)
            .or_insert_with(|| Value::String(String::new()));
        self.base.into()
    }
}

pub struct Logger<W> {
    state: LogState,
    make_writer: W,
}

impl<W> Logger<W>
where
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    pub fn new(make_writer: W, field_routes: FieldRouter) -> Self {
        Self {
            state: LogState::new(Arc::new(field_routes)),
            make_writer,
        }
    }
}

fn str_iter_join<'a, Iter>(iter: &mut Iter, sep: &str) -> String
where
    Iter: Iterator<Item = &'a str>,
{
    match iter.next() {
        None => String::new(),
        Some(first) => {
            let (lower, _) = iter.size_hint();
            let mut result = String::with_capacity(first.len() + sep.len() * lower);
            result.push_str(first);
            for s in iter {
                result.push_str(sep);
                result.push_str(s);
            }
            result
        }
    }
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

impl<W, S> Layer<S> for Logger<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");

        let mut extensions = span.extensions_mut();

        if extensions.get_mut::<LogState>().is_none() {
            let mut state = LogState::new(self.state.field_routes.clone());
            let mut visitor = visitor::FieldVisitor::new(&mut state);
            attrs.record(&mut visitor);
            extensions.insert(state);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");
        let mut extensions = span.extensions_mut();
        if let Some(mut state) = extensions.get_mut::<LogState>() {
            let mut add_field_visitor = visitor::FieldVisitor::new(&mut state);
            values.record(&mut add_field_visitor);
        } else {
            let mut state = LogState::new(self.state.field_routes.clone());
            let mut add_field_visitor = visitor::FieldVisitor::new(&mut state);
            values.record(&mut add_field_visitor);
            extensions.insert(state)
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut state = self.state.clone();

        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(span_state) = span.extensions().get::<LogState>() {
                    state.merge(span_state);
                }
            }
        }

        let logger_name = if let Some(scope) = ctx.event_scope(event) {
            str_iter_join(&mut scope.map(|span| span.name()), ":")
        } else {
            String::new()
        };

        state.insert_base(LOGGER_NAME.into(), logger_name);
        let level = metadata.level();
        state.insert_base(LEVEL.into(), level.to_string());
        state.insert_base(LEVEL_VALUE.into(), level_value(level));

        let mut field_visitor = visitor::FieldVisitor::new(&mut state);
        event.record(&mut field_visitor);

        let mut raw = serde_json::to_vec(&state.into_value()).unwrap();
        raw.push(b'\n');

        let mut writer = self.make_writer.make_writer();
        let _ = writer.write_all(&raw);
    }
}

#[derive(Copy, Clone)]
pub enum FieldDestination {
    Root,
    Tag,
}

#[derive(Copy, Clone)]
pub enum FieldAction {
    Value,
    ToString,
}

impl FieldAction {
    fn apply<T: ToString + Into<Value>>(self, value: T) -> Value {
        match self {
            FieldAction::Value => value.into(),
            FieldAction::ToString => Value::String(value.to_string()),
        }
    }
}

pub struct FieldRouter {
    routes: HashMap<&'static str, (FieldDestination, FieldAction, String)>,
}

impl FieldRouter {
    pub fn route<T: ToString + Into<Value>>(
        &self,
        field: &Field,
        value: T,
    ) -> Option<(FieldDestination, String, Value)> {
        match self.routes.get(field.name()) {
            Some((d, action, name)) => Some((*d, name.clone(), action.apply(value))),
            _ => None,
        }
    }

    pub fn add_tag<S: ToString>(&mut self, from: &'static str, to: S, action: FieldAction) {
        self.routes
            .insert(from, (FieldDestination::Tag, action, to.to_string()));
    }

    pub fn add_root<S: ToString>(&mut self, from: &'static str, to: S, action: FieldAction) {
        self.routes
            .insert(from, (FieldDestination::Root, action, to.to_string()));
    }
}

impl Default for FieldRouter {
    fn default() -> Self {
        let mut router = Self {
            routes: HashMap::new(),
        };
        router.add_root("message", MESSAGE, FieldAction::ToString);
        router.add_root("log.line", LINE_NUMBER, FieldAction::Value);
        router.add_root("log.file", FILE_NAME, FieldAction::Value);
        router
    }
}

pub fn init<W>(writer: W, field_routes: FieldRouter) -> Logger<W>
where
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    Logger::new(writer, field_routes)
}
