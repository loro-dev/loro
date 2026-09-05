//! Build JS state with fixed constructors; no callbacks or document-wide intermediary.
use super::*;
use loro_internal::read_state::{err, Event, Sink};
use std::collections::HashMap;
#[wasm_bindgen(inline_js = "
const kinds = ['Map', 'List', 'MovableList', 'Text', 'Tree', 'Counter'];
export function stateOptions(options, document) {
  if (typeof options !== 'object' || options === null || Array.isArray(options)) throw new Error('Invalid toContainerTree options');
  for (const key of Object.keys(options)) {
    if (key !== 'text' && !(document && key === 'roots')) throw new Error('Unknown toContainerTree option: ' + key);
  }
  if (document && options.roots !== undefined && !Array.isArray(options.roots)) throw new Error('roots must be an array');
  return options;
}
export function stateSlice(node, start, totalLength) { return {cid:node.cid, start, totalLength, items:node.value}; }
export function stateContainer(kind, cid, value) { return {type: kinds[kind], cid, value}; }
export function stateCid(peer,counter,kind) { return 'cid:'+counter+'@'+peer+':'+kinds[kind]; }
export function stateRootCid(name,kind) { return 'cid:root-'+name+':'+kinds[kind]; }
export function stateValue(value) { return {type: 'Value', value}; }
export function stateBinary(bytes) { return bytes.slice(); }
export function stateIndex(array, index, value) { Object.defineProperty(array, index, {value, enumerable: true, writable: true, configurable: true}); }
export function stateSet(object, key, value) { Object.defineProperty(object, key, {value, enumerable: true, writable: true, configurable: true}); }
")]
extern "C" {
    #[wasm_bindgen(catch)]
    fn stateOptions(options: JsValue, document: bool) -> Result<JsValue, JsValue>;
    fn stateSlice(node: JsValue, start: u32, total_length: u32) -> JsValue;
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
#[serde(deny_unknown_fields)]
struct Options {
    #[serde(default)]
    text: TextMode,
}
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentOptions {
    #[serde(default)]
    text: TextMode,
    roots: Option<Vec<String>>,
}
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextMode {
    #[default]
    Plain,
    Delta,
}
fn options<T: Default + serde::de::DeserializeOwned>(
    value: Option<JsValue>,
    document: bool,
) -> JsResult<T> {
    match value {
        None => Ok(T::default()),
        Some(value) => serde_wasm_bindgen::from_value(stateOptions(value, document)?)
            .map_err(|e| JsValue::from_str(&format!("Invalid toContainerTree options: {e}"))),
    }
}
fn container_tree<H: HandlerTrait>(
    handler: &H,
    opts: Option<JsValue>,
) -> JsResult<JsContainerNode> {
    let opts: Options = options(opts, false)?;
    let doc = handler
        .doc()
        .ok_or_else(|| JsValue::from_str("toContainerTree requires an attached container"))?;
    let mut sink = FixedSink::default();
    doc.app_state().lock().read_state(
        &mut sink,
        Some(&handler.id()),
        matches!(opts.text, TextMode::Delta),
        None,
    )?;
    Ok(sink.0.root.unchecked_into())
}
fn list_slice<H: HandlerTrait>(
    handler: &H,
    start: f64,
    end: f64,
    opts: Option<JsValue>,
) -> JsResult<JsContainerTreeSlice> {
    if [start, end]
        .iter()
        .any(|n| !n.is_finite() || *n < 0.0 || *n > u32::MAX as f64 || n.fract() != 0.0)
    {
        return Err(JsValue::from_str(
            "Slice bounds must be nonnegative u32 integers",
        ));
    }
    let opts: Options = options(opts, false)?;
    let doc = handler
        .doc()
        .ok_or_else(|| JsValue::from_str("toContainerTreeSlice requires an attached container"))?;
    let mut sink = FixedSink::default();
    let (start, total) = doc.app_state().lock().read_state_slice(
        &mut sink,
        &handler.id(),
        matches!(opts.text, TextMode::Delta),
        start as usize,
        end as usize,
    )?;
    Ok(stateSlice(sink.0.root, start as u32, total as u32).unchecked_into())
}
#[wasm_bindgen]
impl LoroDoc {
    /// Convert visible roots to independent container trees without committing.
    /// The text format applies recursively, including Tree metadata. Unknown or
    /// hidden roots are omitted; roots: [] returns an empty object. Reads never
    /// create roots. Binary values are owned Uint8Arrays. Not a CRDT export.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsDocumentContainerTreeOptions>,
    ) -> JsResult<JsDocumentContainerTree> {
        let opts: DocumentOptions = options(opts.map(Into::into), true)?;
        let mut sink = FixedSink::default();
        self.doc.app_state().lock().read_state(
            &mut sink,
            None,
            matches!(opts.text, TextMode::Delta),
            opts.roots.as_deref(),
        )?;
        Ok(sink.0.root.unchecked_into())
    }
}
#[wasm_bindgen]
impl LoroMap {
    /// Convert this attached container and all descendants to an independent tree.
    /// Text format applies recursively. Does not commit; throws for detached
    /// containers, unsupported types, invalid options, or nesting above 256.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerNode> {
        container_tree(&self.handler, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroList {
    /// Convert this attached container and all descendants to an independent tree.
    /// Text format applies recursively. Does not commit; throws for detached
    /// containers, unsupported types, invalid options, or nesting above 256.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerNode> {
        container_tree(&self.handler, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroList {
    /// Read [start,end), clamped to list length. Inverted bounds return no items.
    /// Returns actual start, totalLength and items; this is not a complete container.
    /// Only selected descendants are traversed; parent shallow list access is O(N).
    #[wasm_bindgen(js_name = toContainerTreeSlice, skip_typescript)]
    pub fn to_container_tree_slice(
        &self,
        start: f64,
        end: f64,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerTreeSlice> {
        list_slice(&self.handler, start, end, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroMovableList {
    /// Convert this attached container and all descendants to an independent tree.
    /// Text format applies recursively. Does not commit; throws for detached
    /// containers, unsupported types, invalid options, or nesting above 256.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerNode> {
        container_tree(&self.handler, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroMovableList {
    /// Read [start,end), clamped to list length. Inverted bounds return no items.
    /// Returns actual start, totalLength and items; this is not a complete container.
    /// Only selected descendants are traversed; parent shallow list access is O(N).
    #[wasm_bindgen(js_name = toContainerTreeSlice, skip_typescript)]
    pub fn to_container_tree_slice(
        &self,
        start: f64,
        end: f64,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerTreeSlice> {
        list_slice(&self.handler, start, end, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroText {
    /// Convert this attached container and all descendants to an independent tree.
    /// Text format applies recursively. Does not commit; throws for detached
    /// containers, unsupported types, invalid options, or nesting above 256.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerNode> {
        container_tree(&self.handler, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroTree {
    /// Convert this attached container and all descendants to an independent tree.
    /// Text format applies recursively. Does not commit; throws for detached
    /// containers, unsupported types, invalid options, or nesting above 256.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerNode> {
        container_tree(&self.handler, opts.map(Into::into))
    }
}
#[wasm_bindgen]
impl LoroCounter {
    /// Convert this attached container and all descendants to an independent tree.
    /// Text format applies recursively. Does not commit; throws for detached
    /// containers, unsupported types, invalid options, or nesting above 256.
    #[wasm_bindgen(js_name = toContainerTree, skip_typescript)]
    pub fn to_container_tree(
        &self,
        opts: Option<JsContainerTreeOptions>,
    ) -> JsResult<JsContainerNode> {
        container_tree(&self.handler, opts.map(Into::into))
    }
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "ContainerTreeOptions")]
    pub type JsContainerTreeOptions;
    #[wasm_bindgen(typescript_type = "DocumentContainerTreeOptions")]
    pub type JsDocumentContainerTreeOptions;
    #[wasm_bindgen(typescript_type = "ContainerNode")]
    pub type JsContainerNode;
    #[wasm_bindgen(typescript_type = "Record<string, ContainerNode>")]
    pub type JsDocumentContainerTree;
    #[wasm_bindgen(typescript_type = "ContainerTreeSlice")]
    pub type JsContainerTreeSlice;
}
#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
export type ContainerTreeTextFormat = "plain" | "delta";
/** Text format applies to every descendant Text, including Tree metadata. */
export interface ContainerTreeOptions<T extends ContainerTreeTextFormat = ContainerTreeTextFormat> { text?: T }
export interface DocumentContainerTreeOptions<T extends ContainerTreeTextFormat = ContainerTreeTextFormat> extends ContainerTreeOptions<T> {
    /** Select visible roots by name. Missing roots are omitted, never created. [] selects none. */
    roots?: readonly string[];
}
/** Opaque ordinary data: never interpret objects inside value as container nodes. */
export type ValueNode = { type: "Value"; value: Value };
export type ContainerTreeNode<T extends ContainerTreeTextFormat = ContainerTreeTextFormat> = ContainerNode<T> | ValueNode;
export type ContainerNode<T extends ContainerTreeTextFormat = ContainerTreeTextFormat> =
    | { type: "Map"; cid: ContainerID; value: Record<string, ContainerTreeNode<T>> }
    | { type: "List"; cid: ContainerID; value: ContainerTreeNode<T>[] }
    | { type: "MovableList"; cid: ContainerID; value: ContainerTreeNode<T>[] }
    | { type: "Text"; cid: ContainerID; value: T extends "delta" ? Delta<string>[] : string }
    | { type: "Tree"; cid: ContainerID; value: TreeNodeSnapshot<T>[] }
    | { type: "Counter"; cid: ContainerID; value: number };
export interface TreeNodeSnapshot<T extends ContainerTreeTextFormat = ContainerTreeTextFormat> {
    id: TreeID; parent: TreeID | null; index: number; fractional_index: string;
    meta: Extract<ContainerNode<T>, {type:"Map"}>;
    children: TreeNodeSnapshot<T>[];
}
/** A partial list, not a complete ContainerNode. start is clamped to totalLength. */
export interface ContainerTreeSlice<T extends ContainerTreeTextFormat = ContainerTreeTextFormat> {
    cid: ContainerID; start: number; totalLength: number; items: ContainerTreeNode<T>[];
}
interface LoroDoc {
    /** Convert visible roots to independent container trees. No commit, live handles or CRDT history.
     * Text defaults to plain strings. Throws for invalid options, unsupported types or nesting >256.
     */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: DocumentContainerTreeOptions<T>): Record<string, ContainerNode<T>>;
}
interface LoroMap {
    /** Read this attached container and descendants; text format applies recursively. Throws if detached. */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: ContainerTreeOptions<T>): Extract<ContainerNode<T>, {type:"Map"}>;
}
interface LoroList {
    /** Read this attached container and descendants; text format applies recursively. Throws if detached. */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: ContainerTreeOptions<T>): Extract<ContainerNode<T>, {type:"List"}>;
    /** Read [start,end), clamped, with source coordinates; parent shallow list access remains O(N). */
    toContainerTreeSlice<T extends ContainerTreeTextFormat = "plain">(start:number,end:number,options?:ContainerTreeOptions<T>): ContainerTreeSlice<T>;
}
interface LoroMovableList {
    /** Read this attached container and descendants; text format applies recursively. Throws if detached. */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: ContainerTreeOptions<T>): Extract<ContainerNode<T>, {type:"MovableList"}>;
    /** Read [start,end), clamped, with source coordinates; parent shallow list access remains O(N). */
    toContainerTreeSlice<T extends ContainerTreeTextFormat = "plain">(start:number,end:number,options?:ContainerTreeOptions<T>): ContainerTreeSlice<T>;
}
interface LoroText {
    /** Read this attached container and descendants; text format applies recursively. Throws if detached. */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: ContainerTreeOptions<T>): Extract<ContainerNode<T>, {type:"Text"}>;
}
interface LoroTree {
    /** Read this attached container and descendants; text format applies recursively. Throws if detached. */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: ContainerTreeOptions<T>): Extract<ContainerNode<T>, {type:"Tree"}>;
}
interface LoroCounter {
    /** Read this attached container and descendants; text format applies recursively. Throws if detached. */
    toContainerTree<T extends ContainerTreeTextFormat = "plain">(options?: ContainerTreeOptions<T>): Extract<ContainerNode<T>, {type:"Counter"}>;
}
"#;
