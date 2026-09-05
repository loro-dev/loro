//! Build JS state with fixed constructors; no callbacks or document-wide intermediary.
use super::*;
use loro_internal::read_state::{err, Event, Sink};
use std::collections::HashMap;
#[wasm_bindgen(inline_js = "
const kinds = ['Map', 'List', 'MovableList', 'Text', 'Tree', 'Counter'];
export function stateContainer(kind, cid, value) { return {type: kinds[kind], cid, value}; }
export function stateCid(peer,counter,kind) { return 'cid:'+counter+'@'+peer+':'+kinds[kind]; }
export function stateRootCid(name,kind) { return 'cid:root-'+name+':'+kinds[kind]; }
export function stateValue(value) { return {type: 'Value', value}; }
export function stateBinary(bytes) { return bytes.slice(); }
export function stateIndex(array, index, value) { Object.defineProperty(array, index, {value, enumerable: true, writable: true, configurable: true}); }
export function stateSet(object, key, value) { Object.defineProperty(object, key, {value, enumerable: true, writable: true, configurable: true}); }
")]
extern "C" {
    fn stateContainer(kind: u8, cid: &JsValue, value: JsValue) -> JsValue;
    fn stateValue(value: JsValue) -> JsValue;
    fn stateBinary(bytes: &[u8]) -> JsValue;
    #[wasm_bindgen(catch)]
    fn stateIndex(array: &JsValue, index: u32, value: &JsValue) -> Result<(), JsValue>;
    fn stateCid(peer: &JsValue, counter: i32, kind: u8) -> JsValue;
    fn stateRootCid(name: &str, kind: u8) -> JsValue;
    #[wasm_bindgen(catch)]
    fn stateSet(object: &JsValue, key: &JsValue, value: &JsValue) -> Result<(), JsValue>;
}
struct Frame {
    wrapper: u8,
    cid: JsValue,
    v: JsValue,
    array: bool,
    next: u32,
    key: JsValue,
}
#[derive(Default)]
struct JsSink {
    stack: Vec<Frame>,
    root: JsValue,
    keys: HashMap<String, JsValue>,
}
impl JsSink {
    fn add(&mut self, v: JsValue) -> LoroResult<()> {
        if let Some(f) = self.stack.last_mut() {
            if f.wrapper != 0 {
                f.v = v;
                return Ok(());
            }
            if f.array {
                stateIndex(&f.v, f.next, &v).map_err(|_| err("JS array property failed"))?;
                f.next += 1;
            } else {
                stateSet(&f.v, &f.key, &v).map_err(|_| err("JS define property failed"))?;
            }
        } else {
            self.root = v;
        }
        Ok(())
    }
}
impl JsSink {
    fn emit(&mut self, e: Event<'_>) -> LoroResult<()> {
        match e {
            Event::Key(k) => {
                let key = if let Some(v) = self.keys.get(k) {
                    v.clone()
                } else {
                    let v: JsValue = k.into();
                    self.keys.insert(k.to_owned(), v.clone());
                    v
                };
                let f = self
                    .stack
                    .last_mut()
                    .ok_or_else(|| err("Missing object frame"))?;
                f.key = key;
            }
            Event::Object(_) => self.stack.push(Frame {
                wrapper: 0,
                cid: JsValue::NULL,
                v: Object::new().into(),
                array: false,
                next: 0,
                key: JsValue::NULL,
            }),
            Event::Array(n) => self.stack.push(Frame {
                wrapper: 0,
                cid: JsValue::NULL,
                v: Array::new_with_length(n as u32).into(),
                array: true,
                next: 0,
                key: JsValue::NULL,
            }),
            Event::End => {
                let f = self.stack.pop().ok_or_else(|| err("Missing state frame"))?;
                let v = if f.wrapper == 16 {
                    stateValue(f.v)
                } else if f.wrapper != 0 {
                    stateContainer(f.wrapper - 10, &f.cid, f.v)
                } else {
                    f.v
                };
                self.add(v)?;
            }
            Event::String(s) => self.add(s.into())?,
            Event::Number(n) => self.add(n.into())?,
            Event::Bool(b) => self.add(b.into())?,
            Event::Binary(bytes) => self.add(stateBinary(bytes))?,
            Event::Null => self.add(JsValue::NULL)?,
        };
        Ok(())
    }
}
#[derive(Default)]
struct FixedSink(JsSink, HashMap<u64, JsValue>);
impl Sink for FixedSink {
    fn container(&mut self, id: &ContainerID) -> LoroResult<()> {
        let kind = match id.container_type() {
            ContainerType::Map => 0,
            ContainerType::List => 1,
            ContainerType::MovableList => 2,
            ContainerType::Text => 3,
            ContainerType::Tree => 4,
            ContainerType::Counter => 5,
            _ => return Err(err("Unsupported container type")),
        };
        let cid = match id {
            ContainerID::Root { name, .. } => stateRootCid(name, kind),
            ContainerID::Normal { peer, counter, .. } => {
                let p = self
                    .1
                    .entry(*peer)
                    .or_insert_with(|| JsValue::from_str(&peer.to_string()));
                stateCid(p, *counter, kind)
            }
        };
        self.0.stack.push(Frame {
            wrapper: kind + 10,
            cid,
            v: JsValue::NULL,
            array: false,
            next: 0,
            key: JsValue::NULL,
        });
        Ok(())
    }

    fn emit(&mut self, e: Event<'_>) -> LoroResult<()> {
        self.0.emit(e)
    }
    fn container_end(&mut self) -> LoroResult<()> {
        self.emit(Event::End)
    }
    fn value_start(&mut self) -> LoroResult<()> {
        self.0.stack.push(Frame {
            wrapper: 16,
            cid: JsValue::NULL,
            v: JsValue::NULL,
            array: false,
            next: 0,
            key: JsValue::NULL,
        });
        Ok(())
    }
    fn value_end(&mut self) -> LoroResult<()> {
        self.emit(Event::End)
    }
}

#[derive(Default, serde::Deserialize)]
struct Options {
    container: Option<String>,
    #[serde(default)]
    text: TextMode,
    range: Option<Range>,
}
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextMode {
    #[default]
    Plain,
    Delta,
}
#[derive(serde::Deserialize)]
struct Range {
    start: u32,
    end: u32,
}

#[wasm_bindgen]
impl LoroDoc {
    /// Read nested container state with explicit IDs and opaque ordinary values.
    /// Reads current state without committing. Text defaults to plain strings.
    /// A list range is end-exclusive and clamped; inverted ranges are empty.
    /// Throws for invalid options, missing containers, unsupported container types,
    /// or nesting exceeding 256 levels. Returned objects are independent snapshots.
    #[wasm_bindgen(js_name = readState, skip_typescript)]
    pub fn read_state(&self, options: Option<JsReadStateOptions>) -> JsResult<JsReadState> {
        let opts: Options = match options {
            None => Options::default(),
            Some(v) => serde_wasm_bindgen::from_value(v.into())
                .map_err(|e| JsValue::from_str(&format!("Invalid readState options: {e}")))?,
        };
        let cid = opts
            .container
            .as_deref()
            .map(ContainerID::try_from)
            .transpose()
            .map_err(|_| JsValue::from_str("Invalid container ID"))?;
        if cid.as_ref().is_some_and(|id| !self.doc.has_container(id)) {
            return Err(JsValue::from_str("The container does not exist in the doc"));
        }
        let mut sink = FixedSink::default();
        self.doc.app_state().lock().read_state(
            &mut sink,
            cid.as_ref(),
            matches!(opts.text, TextMode::Delta),
            opts.range.map(|r| (r.start as usize, r.end as usize)),
        )?;
        Ok(sink.0.root.unchecked_into())
    }
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "ReadStateOptions")]
    pub type JsReadStateOptions;
    #[wasm_bindgen(typescript_type = "ContainerState | Record<string, ContainerState>")]
    pub type JsReadState;
}
#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
interface LoroDoc {
    /** Read nested container snapshots without committing. Ordinary data stays inside Value nodes.
     * Text defaults to strings; use text: "delta" to preserve formatting.
     * Throws for invalid options, unsupported containers, or nesting over 256 levels.
     */
    readState(options?: ReadStateOptions & { container?: undefined; range?: never }): Record<string, ContainerState>;
    /** Read one container; list ranges are clamped and end-exclusive. */
    readState(options: ReadStateOptions & { container: ContainerID }): ContainerState;
    readState(options: ReadStateOptions): ContainerState | Record<string, ContainerState>;
}
/** Options for reading the whole document, a container, or a list interval. */
export interface ReadStateOptions {
    container?: ContainerID;
    text?: "plain" | "delta";
    range?: { start: number; end: number };
}
/** Ordinary data is opaque: never interpret objects inside value as containers. */
export type StateValue = { type: "Value"; value: Value };
export type StateNode = ContainerState | StateValue;
export type ContainerState =
    | { type: "Map"; cid: ContainerID; value: Record<string, StateNode> }
    | { type: "List" | "MovableList"; cid: ContainerID; value: StateNode[] }
    | { type: "Text"; cid: ContainerID; value: string | Delta<string>[] }
    | { type: "Tree"; cid: ContainerID; value: StateTreeNode[] }
    | { type: "Counter"; cid: ContainerID; value: number };
export interface StateTreeNode {
    id: TreeID;
    parent: TreeID | null;
    index: number;
    fractional_index: string;
    meta: Extract<ContainerState, { type: "Map" }>;
    children: StateTreeNode[];
}
"#;
