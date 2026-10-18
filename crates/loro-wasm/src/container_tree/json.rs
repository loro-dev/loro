//! Stream the existing traversal into UTF-8 JSON without constructing JS values.
use super::{err, ContainerID, ContainerType, Event, LoroResult, Sink};

struct Frame {
    array: bool,
    first: bool,
}

#[derive(Default)]
pub(super) struct JsonSink {
    output: Vec<u8>,
    stack: Vec<Frame>,
}

impl JsonSink {
    pub(super) fn finish(self) -> LoroResult<String> {
        String::from_utf8(self.output).map_err(|_| err("Invalid container tree JSON UTF-8"))
    }

    fn separator(output: &mut Vec<u8>, frame: &mut Frame) {
        if !frame.first {
            output.push(b',');
        }
        frame.first = false;
    }

    fn before_value(&mut self) {
        if let Some(frame) = self.stack.last_mut() {
            if frame.array {
                Self::separator(&mut self.output, frame);
            }
        }
    }

    fn write<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> LoroResult<()> {
        serde_json::to_writer(&mut self.output, value)
            .map_err(|_| err("Failed to serialize container tree JSON"))
    }

    fn wrapper(&mut self, kind: &str, id: Option<&ContainerID>) -> LoroResult<()> {
        self.emit(Event::Object(if id.is_some() { 3 } else { 2 }))?;
        self.emit(Event::Key("type"))?;
        self.emit(Event::String(kind))?;
        if let Some(id) = id {
            self.emit(Event::Key("cid"))?;
            self.emit(Event::String(&id.to_string()))?;
        }
        self.emit(Event::Key("value"))
    }
}

impl Sink for JsonSink {
    fn emit(&mut self, event: Event<'_>) -> LoroResult<()> {
        match event {
            Event::Key(key) => {
                let frame = self
                    .stack
                    .last_mut()
                    .ok_or_else(|| err("Missing JSON object frame"))?;
                Self::separator(&mut self.output, frame);
                self.write(key)?;
                self.output.push(b':');
            }
            Event::End => {
                let frame = self
                    .stack
                    .pop()
                    .ok_or_else(|| err("Missing JSON state frame"))?;
                self.output.push(if frame.array { b']' } else { b'}' });
            }
            Event::Object(_) | Event::Array(_) => {
                self.before_value();
                let array = matches!(event, Event::Array(_));
                self.output.push(if array { b'[' } else { b'{' });
                self.stack.push(Frame { array, first: true });
            }
            Event::String(value) => {
                self.before_value();
                self.write(value)?;
            }
            Event::Number(value) => {
                if !value.is_finite() {
                    return Err(err("Container tree JSON cannot preserve non-finite numbers; use toContainerTree"));
                }
                self.before_value();
                self.write(&value)?;
            }
            Event::Bool(value) => {
                self.before_value();
                self.write(&value)?;
            }
            Event::Null => {
                self.before_value();
                self.output.extend_from_slice(b"null");
            }
            Event::Binary(_) => {
                return Err(err(
                    "Container tree JSON cannot preserve binary values; use toContainerTree",
                ));
            }
        }
        Ok(())
    }

    fn container(&mut self, id: &ContainerID) -> LoroResult<()> {
        let kind = match id.container_type() {
            ContainerType::Map => "Map",
            ContainerType::List => "List",
            ContainerType::MovableList => "MovableList",
            ContainerType::Text => "Text",
            ContainerType::Tree => "Tree",
            ContainerType::Counter => "Counter",
            _ => return Err(err("Unsupported container type")),
        };
        self.wrapper(kind, Some(id))
    }

    fn value_start(&mut self) -> LoroResult<()> {
        self.wrapper("Value", None)
    }

    fn value_end(&mut self) -> LoroResult<()> {
        self.emit(Event::End)
    }
}
