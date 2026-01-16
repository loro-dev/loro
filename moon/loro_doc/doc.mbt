///|
pub struct CommitOptions {
  timestamp : Int64?
  message : String?
}

///|
pub struct ExportOptions {
  from : @loro_codec.VersionVector?
}

enum PendingOpContent {
  MapInsert(String, Value)
  MapDelete(String)
  ListInsert(Int, Array[Value])
  ListDelete(Int, Int, @loro_codec.ID)
  TextInsert(Int, String)
  TextDelete(Int, Int, @loro_codec.ID)
}

struct PendingOp {
  container : @loro_codec.ContainerID
  counter : Int
  lamport : UInt
  len : UInt
  content : PendingOpContent
}

struct DocInner {
  peer : UInt64
  vv : @hashmap.HashMap[UInt64, Int]
  frontiers : Array[@loro_codec.ID]
  frontier_lamports : @hashmap.HashMap[String, UInt]
  change_vv_ends : @hashmap.HashMap[String, @hashmap.HashMap[UInt64, Int]]
  txn_deps : Array[@loro_codec.ID]?
  txn_start_counter : Int
  txn_start_lamport : UInt
  txn_next_counter : Int
  txn_next_lamport : UInt
  keys : Array[String]
  key_to_idx : @hashmap.HashMap[String, UInt64]
  oplog_changes : Array[@loro_codec.Change]
  pending_ops : Array[PendingOp]
  roots : @hashmap.HashMap[String, @loro_codec.ContainerID]
  map_states : @hashmap.HashMap[String, MapState]
  list_trackers : @hashmap.HashMap[String, SeqTracker]
  text_trackers : @hashmap.HashMap[String, SeqTracker]
}

///|
pub struct LoroDoc {
  inner : DocInner
}

///|
pub fn LoroDoc::new() -> LoroDoc {
  {
    inner: {
      peer: 0UL,
      vv: @hashmap.new(capacity=4),
      frontiers: [],
      frontier_lamports: @hashmap.new(capacity=4),
      change_vv_ends: @hashmap.new(capacity=16),
      txn_deps: None,
      txn_start_counter: 0,
      txn_start_lamport: 0U,
      txn_next_counter: 0,
      txn_next_lamport: 0U,
      keys: [],
      key_to_idx: @hashmap.new(capacity=16),
      oplog_changes: [],
      pending_ops: [],
      roots: @hashmap.new(capacity=8),
      map_states: @hashmap.new(capacity=8),
      list_trackers: @hashmap.new(capacity=8),
      text_trackers: @hashmap.new(capacity=8),
    },
  }
}

///|
pub fn LoroDoc::peerId(self : LoroDoc) -> UInt64 {
  self.inner.peer
}

///|
pub fn LoroDoc::setPeerId(self : LoroDoc, peer : UInt64) -> Unit raise LoroError {
  if self.inner.oplog_changes.length() != 0 ||
    self.inner.pending_ops.length() != 0 {
    raise LoroError("setPeerId: cannot change peer after edits/imports")
  }
  self.inner.peer = peer
}

///|
pub fn LoroDoc::commit(
  self : LoroDoc,
  options : CommitOptions?,
) -> Unit raise LoroError {
  self.commit_inner(options)
}

///|
pub fn LoroDoc::oplogVersion(self : LoroDoc) -> @loro_codec.VersionVector {
  vv_from_map(self.inner.vv)
}

///|
pub fn LoroDoc::import(self : LoroDoc, bytes : Bytes) -> Unit raise LoroError {
  // Barrier: commit pending local ops first.
  self.commit_inner(None)

  let doc = @loro_codec.parse_document(bytes, true) catch {
    @loro_codec.DecodeError(msg) => raise LoroError("import: decode failed: " + msg)
  }
  if doc.mode() != 4U {
    raise LoroError("import: only FastUpdates(mode=4) is supported")
  }
  let blocks = @loro_codec.parse_fast_updates_body(doc.body_view()) catch {
    @loro_codec.DecodeError(msg) =>
      raise LoroError("import: parse fastupdates failed: " + msg)
  }

  let incoming : Array[@loro_codec.Change] = []
  for b in blocks {
    let decoded = @loro_codec.decode_change_block_full(b) catch {
      @loro_codec.DecodeError(msg) =>
        raise LoroError("import: decode change block failed: " + msg)
    }
    let block_keys = decoded.keys()
    for c in decoded.changes() {
      incoming.push(self.normalize_change(c, block_keys))
    }
  }

  // Deterministic order: (lamport, peer, counter).
  let counter_bias = BigInt::from_int64(-2147483648L)
  incoming.sort_by_key(c => {
    let lamport_key = BigInt::from_uint64(c.lamport().to_uint64()) << 96
    let peer_key = BigInt::from_uint64(c.id().peer()) << 32
    let counter_key = BigInt::from_int(c.id().counter()) - counter_bias
    lamport_key | peer_key | counter_key
  })

  for c in incoming {
    self.apply_incoming_change(c)
  }
}

///|
pub fn LoroDoc::export(
  self : LoroDoc,
  opts : ExportOptions,
) -> Bytes raise LoroError {
  // Barrier: commit pending local ops first.
  self.commit_inner(None)

  let from_map = match opts.from {
    None => @hashmap.new(capacity=0)
    Some(vv) => vv_to_map(vv)
  }

  let by_peer : @hashmap.HashMap[UInt64, Array[@loro_codec.Change]] = @hashmap.new(
    capacity=8,
  )
  for c in self.inner.oplog_changes {
    let peer = c.id().peer()
    let from_counter = vv_get(from_map, peer)
    if c.id().counter() < from_counter {
      continue
    }
    match by_peer.get(peer) {
      None => {
        let arr : Array[@loro_codec.Change] = []
        arr.push(c)
        by_peer.set(peer, arr)
      }
      Some(arr) => arr.push(c)
    }
  }

  let blocks : Array[Bytes] = []
  for pair in by_peer {
    let (peer, changes) = pair
    changes.sort_by_key(c => c.id().counter())
    let block = {
      peers: [peer],
      keys: self.inner.keys,
      cids: [],
      positions: [],
      changes,
    }
    let encoded = @loro_codec.encode_change_block(block) catch {
      @loro_codec.EncodeError(msg) => raise LoroError("export: encode block failed: " + msg)
    }
    blocks.push(encoded)
  }

  let body = @loro_codec.encode_fast_updates_body(blocks)
  @loro_codec.encode_document(4U, body) catch {
    @loro_codec.DecodeError(msg) => raise LoroError("export: encode document failed: " + msg)
  }
}

///|
pub fn LoroDoc::getShallowValue(self : LoroDoc) -> Json {
  let obj = Map::new(capacity=16)
  for pair in self.inner.roots {
    let (name, cid) = pair
    obj[name] = Json::string(LORO_CONTAINER_ID_PREFIX + container_id_string(cid))
  }
  Json::object(obj)
}

///|
pub fn LoroDoc::toJSON(self : LoroDoc) -> Json {
  let obj = Map::new(capacity=16)
  for pair in self.inner.roots {
    let (name, cid) = pair
    obj[name] = self.container_deep_json(cid)
  }
  Json::object(obj)
}

// --- Root container getters ---

///|
pub fn LoroDoc::getMap(self : LoroDoc, name : String) -> LoroMap raise LoroError {
  check_root_container_name(name)
  let cid = @loro_codec.ContainerID::root(name, @loro_codec.ContainerType::map())
  self.register_root(cid)
  self.ensure_map_state(cid)
  { doc: self, cid }
}

///|
pub fn LoroDoc::getList(self : LoroDoc, name : String) -> LoroList raise LoroError {
  check_root_container_name(name)
  let cid = @loro_codec.ContainerID::root(name, @loro_codec.ContainerType::list())
  self.register_root(cid)
  let _ = self.ensure_list_tracker(cid)
  { doc: self, cid }
}

///|
pub fn LoroDoc::getMovableList(
  self : LoroDoc,
  name : String,
) -> LoroMovableList raise LoroError {
  check_root_container_name(name)
  let cid = @loro_codec.ContainerID::root(
    name,
    @loro_codec.ContainerType::movable_list(),
  )
  self.register_root(cid)
  { doc: self, cid }
}

///|
pub fn LoroDoc::getText(self : LoroDoc, name : String) -> LoroText raise LoroError {
  check_root_container_name(name)
  let cid = @loro_codec.ContainerID::root(name, @loro_codec.ContainerType::text())
  self.register_root(cid)
  let _ = self.ensure_text_tracker(cid)
  { doc: self, cid }
}

///|
pub fn LoroDoc::getTree(self : LoroDoc, name : String) -> LoroTree raise LoroError {
  check_root_container_name(name)
  let cid = @loro_codec.ContainerID::root(name, @loro_codec.ContainerType::tree())
  self.register_root(cid)
  { doc: self, cid }
}

// --- Internal helpers ---

fn LoroDoc::register_root(self : LoroDoc, cid : @loro_codec.ContainerID) -> Unit raise LoroError {
  match cid {
    @loro_codec.ContainerID::Root(name, kind) => {
      match self.inner.roots.get(name) {
        None => self.inner.roots.set(name, cid)
        Some(existing) =>
          match existing {
            @loro_codec.ContainerID::Root(_n, k2) =>
              if k2 != kind {
                raise LoroError("root container name already used with a different type")
              }
            @loro_codec.ContainerID::Normal(_, _, _) => ()
          }
      }
    }
    _ => ()
  }
}

fn LoroDoc::start_txn_if_needed(self : LoroDoc) -> Unit raise LoroError {
  if self.inner.pending_ops.length() != 0 {
    return
  }
  let deps : Array[@loro_codec.ID] = []
  for id in self.inner.frontiers {
    deps.push(id)
  }
  let peer = self.inner.peer
  let start_counter = vv_get(self.inner.vv, peer)
  let start_lamport = self.next_lamport_from_frontiers(deps)
  self.inner.txn_deps = Some(deps)
  self.inner.txn_start_counter = start_counter
  self.inner.txn_start_lamport = start_lamport
  self.inner.txn_next_counter = start_counter
  self.inner.txn_next_lamport = start_lamport
}

fn LoroDoc::commit_inner(
  self : LoroDoc,
  options : CommitOptions?,
) -> Unit raise LoroError {
  if self.inner.pending_ops.length() == 0 {
    return
  }

  let peer = self.inner.peer
  let deps = match self.inner.txn_deps {
    None => raise LoroError("commit: missing txn deps")
    Some(ds) => ds
  }
  let start_counter = self.inner.txn_start_counter
  let start_lamport = self.inner.txn_start_lamport
  let timestamp = match options {
    None => 0L
    Some(o) => match o.timestamp {
      None => 0L
      Some(t) => t
    }
  }
  let msg = match options {
    None => None
    Some(o) => o.message
  }

  let mut change = @loro_codec.Change::new(
    @loro_codec.ID::new(peer, start_counter),
    timestamp,
    deps,
    start_lamport,
    msg,
  )

  for p in self.inner.pending_ops {
    match p.content {
      PendingOpContent::MapInsert(key, value) => {
        let lv = encode_value_to_loro_value(value, self.inner.keys, self.inner.key_to_idx)
        change.ops().push(
          @loro_codec.Op::new(
            p.container,
            p.counter,
            p.len,
            @loro_codec.OpContent::Map(@loro_codec.MapOp::Insert(key, lv)),
          ),
        )
      }
      PendingOpContent::MapDelete(key) => {
        change.ops().push(
          @loro_codec.Op::new(
            p.container,
            p.counter,
            p.len,
            @loro_codec.OpContent::Map(@loro_codec.MapOp::Delete(key)),
          ),
        )
      }
      PendingOpContent::ListInsert(pos, items) => {
        let out : Array[@loro_codec.LoroValue] = []
        for it in items {
          out.push(
            encode_value_to_loro_value(it, self.inner.keys, self.inner.key_to_idx),
          )
        }
        change.ops().push(
          @loro_codec.Op::new(
            p.container,
            p.counter,
            p.len,
            @loro_codec.OpContent::List(
              @loro_codec.ListOp::Insert(pos.reinterpret_as_uint(), out),
            ),
          ),
        )
      }
      PendingOpContent::ListDelete(pos, len, start_id) => {
        change.ops().push(
          @loro_codec.Op::new(
            p.container,
            p.counter,
            p.len,
            @loro_codec.OpContent::List(
              @loro_codec.ListOp::Delete(pos, len.to_int64(), start_id),
            ),
          ),
        )
      }
      PendingOpContent::TextInsert(pos, text) => {
        change.ops().push(
          @loro_codec.Op::new(
            p.container,
            p.counter,
            p.len,
            @loro_codec.OpContent::Text(
              @loro_codec.TextOp::Insert(pos.reinterpret_as_uint(), text),
            ),
          ),
        )
      }
      PendingOpContent::TextDelete(pos, len, start_id) => {
        change.ops().push(
          @loro_codec.Op::new(
            p.container,
            p.counter,
            p.len,
            @loro_codec.OpContent::Text(
              @loro_codec.TextOp::Delete(pos, len.to_int64(), start_id),
            ),
          ),
        )
      }
    }
  }

  let end_counter = self.inner.txn_next_counter
  self.inner.pending_ops = []
  self.inner.txn_deps = None
  self.inner.oplog_changes.push(change)
  vv_set_max(self.inner.vv, peer, end_counter)
  self.inner.txn_start_counter = 0
  self.inner.txn_start_lamport = 0U
  self.inner.txn_next_counter = 0
  self.inner.txn_next_lamport = 0U

  let last_id = change_last_id(change)
  let last_lamport = change_last_lamport(change)
  self.inner.frontier_lamports.set(id_string(last_id), last_lamport)
  self.inner.frontiers = [last_id]
  self.inner.change_vv_ends.set(id_string(last_id), vv_clone(self.inner.vv))
}

fn LoroDoc::next_lamport_from_frontiers(
  self : LoroDoc,
  deps : Array[@loro_codec.ID],
) -> UInt raise LoroError {
  if deps.length() == 0 {
    return 0U
  }
  let mut max_lp : UInt? = None
  for id in deps {
    let key = id_string(id)
    match self.inner.frontier_lamports.get(key) {
      None => raise LoroError("missing lamport for frontier " + key)
      Some(lp) =>
        match max_lp {
          None => max_lp = Some(lp)
          Some(m) => if lp > m {
            max_lp = Some(lp)
          }
        }
    }
  }
  match max_lp {
    None => 0U
    Some(lp) => lp + 1U
  }
}

fn LoroDoc::normalize_change(
  self : LoroDoc,
  change : @loro_codec.Change,
  block_keys : Array[String],
) -> @loro_codec.Change raise LoroError {
  let mut out = @loro_codec.Change::new(
    change.id(),
    change.timestamp(),
    change.deps(),
    change.lamport(),
    change.msg(),
  )
  for op in change.ops() {
    out.ops().push(
      @loro_codec.Op::new(
        op.container(),
        op.counter(),
        op.len(),
        self.normalize_op_content(op.content(), block_keys, op.container(), op.counter()),
      ),
    )
  }
  out
}

fn LoroDoc::normalize_op_content(
  self : LoroDoc,
  content : @loro_codec.OpContent,
  block_keys : Array[String],
  _cid : @loro_codec.ContainerID,
  _counter : Int,
) -> @loro_codec.OpContent raise LoroError {
  match content {
    @loro_codec.OpContent::Map(@loro_codec.MapOp::Insert(key, value)) => {
      let v2 = normalize_loro_value_keys(
        value,
        block_keys,
        self.inner.keys,
        self.inner.key_to_idx,
      )
      @loro_codec.OpContent::Map(@loro_codec.MapOp::Insert(key, v2))
    }
    @loro_codec.OpContent::List(@loro_codec.ListOp::Insert(pos, items)) => {
      let out : Array[@loro_codec.LoroValue] = []
      for it in items {
        out.push(
          normalize_loro_value_keys(
            it,
            block_keys,
            self.inner.keys,
            self.inner.key_to_idx,
          ),
        )
      }
      @loro_codec.OpContent::List(@loro_codec.ListOp::Insert(pos, out))
    }
    @loro_codec.OpContent::MovableList(@loro_codec.MovableListOp::Insert(pos, items)) => {
      let out : Array[@loro_codec.LoroValue] = []
      for it in items {
        out.push(
          normalize_loro_value_keys(
            it,
            block_keys,
            self.inner.keys,
            self.inner.key_to_idx,
          ),
        )
      }
      @loro_codec.OpContent::MovableList(@loro_codec.MovableListOp::Insert(pos, out))
    }
    @loro_codec.OpContent::MovableList(@loro_codec.MovableListOp::Set(idlp, value)) => {
      let v2 = normalize_loro_value_keys(
        value,
        block_keys,
        self.inner.keys,
        self.inner.key_to_idx,
      )
      @loro_codec.OpContent::MovableList(@loro_codec.MovableListOp::Set(idlp, v2))
    }
    @loro_codec.OpContent::Text(@loro_codec.TextOp::Mark(start, end, key, value, info)) => {
      let v2 = normalize_loro_value_keys(
        value,
        block_keys,
        self.inner.keys,
        self.inner.key_to_idx,
      )
      @loro_codec.OpContent::Text(@loro_codec.TextOp::Mark(start, end, key, v2, info))
    }
    _ => content
  }
}

fn LoroDoc::ensure_container_state(self : LoroDoc, cid : @loro_codec.ContainerID) -> Unit {
  match cid {
    @loro_codec.ContainerID::Root(_name, _kind) => ()
    @loro_codec.ContainerID::Normal(_peer, _counter, kind) =>
      match kind {
        @loro_codec.ContainerType::Map => {
          let _ = self.ensure_map_state(cid)
        }
        @loro_codec.ContainerType::List => {
          let _ = self.ensure_list_tracker(cid)
        }
        @loro_codec.ContainerType::Text => {
          let _ = self.ensure_text_tracker(cid)
        }
        _ => ()
      }
  }
}

fn LoroDoc::apply_incoming_change(
  self : LoroDoc,
  change : @loro_codec.Change,
) -> Unit raise LoroError {
  let peer = change.id().peer()
  let atom_len = change_atom_len(change)
  let end_excl = change.id().counter() + atom_len
  if vv_get(self.inner.vv, peer) >= end_excl {
    return
  }

  // Build the causal context vv for this change (merge of deps' vv_end).
  let deps_vv : @hashmap.HashMap[UInt64, Int] = @hashmap.new(capacity=8)
  for dep in change.deps() {
    let key = id_string(dep)
    match self.inner.change_vv_ends.get(key) {
      None => raise LoroError("import: deps not satisfied: missing " + key)
      Some(vv_end) =>
        for pair in vv_end {
          let (p, c) = pair
          vv_set_max(deps_vv, p, c)
        }
    }
  }

  // Update history.
  self.inner.oplog_changes.push(change)
  vv_set_max(self.inner.vv, peer, end_excl)
  let last_id = change_last_id(change)
  let last_lamport = change_last_lamport(change)
  self.inner.frontier_lamports.set(id_string(last_id), last_lamport)
  self.inner.change_vv_ends.set(id_string(last_id), vv_clone(self.inner.vv))

  // Update frontiers (best-effort without full DAG).
  let mut next_frontiers : Array[@loro_codec.ID] = []
  for f in self.inner.frontiers {
    let mut is_dep = false
    for d in change.deps() {
      if f == d {
        is_dep = true
        break
      }
    }
    if !is_dep {
      next_frontiers.push(f)
    }
  }
  next_frontiers.push(last_id)
  self.inner.frontiers = next_frontiers

  let merged_vv = self.inner.vv

  // Apply to state.
  for op in change.ops() {
    // Checkout the seq trackers to the causal context before this op.
    let vv_before_op = vv_clone(deps_vv)
    vv_set_max(vv_before_op, peer, op.counter())

    match op.content() {
      @loro_codec.OpContent::Map(m) => {
        self.register_root(op.container())
        let offset = op.counter() - change.id().counter()
        if offset < 0 {
          raise LoroError("import: invalid op counter order")
        }
        let op_lamport = change.lamport() + offset.reinterpret_as_uint()
        let idlp = @loro_codec.IdLp::new(peer, op_lamport)
        match m {
          @loro_codec.MapOp::Insert(key, lv) => {
            let cid_key = container_id_string(op.container())
            let _ = cid_key
            let op_id = @loro_codec.ID::new(peer, op.counter())
            let value = decode_value_from_loro_value(lv, self.inner.keys, op_id)
            self.apply_map_set(op.container(), key, value, idlp)
          }
          @loro_codec.MapOp::Delete(key) =>
            self.apply_map_delete(op.container(), key, idlp)
        }
      }
      @loro_codec.OpContent::List(l) => {
        self.register_root(op.container())
        let t = self.ensure_list_tracker(op.container())
        t.checkout(vv_before_op)
        let offset = op.counter() - change.id().counter()
        if offset < 0 {
          raise LoroError("import: invalid op counter order")
        }
        let op_lamport = change.lamport() + offset.reinterpret_as_uint()
        let op_id = SeqIdFull::new(peer, op.counter(), op_lamport)
        match l {
          @loro_codec.ListOp::Insert(pos, items) => {
            let out : Array[Value] = []
            for i in 0..<items.length() {
              let v = decode_value_from_loro_value(
                items[i],
                self.inner.keys,
                @loro_codec.ID::new(peer, op.counter() + i),
              )
              match v {
                Value::Container(child) => self.ensure_container_state(child)
                _ => ()
              }
              out.push(v)
            }
            t.insert(op_id, pos.to_int(), SeqContent::List(out))
          }
          @loro_codec.ListOp::Delete(pos, signed_len, start_id) => {
            if signed_len == 0L {
              raise LoroError("import: invalid list delete len=0")
            }
            let reversed = signed_len < 0L
            let abs_len = if signed_len < 0L {
              (-signed_len).to_int()
            } else {
              signed_len.to_int()
            }
            let start_pos = if signed_len > 0L {
              pos
            } else {
              pos + 1 + signed_len.to_int()
            }
            t.delete(@loro_codec.ID::new(peer, op.counter()), start_id, start_pos, abs_len, reversed)
          }
        }
        // Restore to the merged current version so future spans become visible again.
        t.checkout(merged_vv)
      }
      @loro_codec.OpContent::Text(t2) => {
        self.register_root(op.container())
        let t = self.ensure_text_tracker(op.container())
        t.checkout(vv_before_op)
        let offset = op.counter() - change.id().counter()
        if offset < 0 {
          raise LoroError("import: invalid op counter order")
        }
        let op_lamport = change.lamport() + offset.reinterpret_as_uint()
        let op_id = SeqIdFull::new(peer, op.counter(), op_lamport)
        match t2 {
          @loro_codec.TextOp::Insert(pos, text) => {
            let n = utf8_count_chars(text)
            t.insert(op_id, pos.to_int(), SeqContent::Text(text, n))
          }
          @loro_codec.TextOp::Delete(pos, signed_len, start_id) => {
            if signed_len == 0L {
              raise LoroError("import: invalid text delete len=0")
            }
            let reversed = signed_len < 0L
            let abs_len = if signed_len < 0L {
              (-signed_len).to_int()
            } else {
              signed_len.to_int()
            }
            let start_pos = if signed_len > 0L {
              pos
            } else {
              pos + 1 + signed_len.to_int()
            }
            t.delete(@loro_codec.ID::new(peer, op.counter()), start_id, start_pos, abs_len, reversed)
          }
          _ => raise LoroError("import: unsupported text op")
        }
        t.checkout(merged_vv)
      }
      _ => raise LoroError("import: unsupported op content")
    }
  }
}

fn LoroDoc::has_container(self : LoroDoc, cid : @loro_codec.ContainerID) -> Bool {
  match cid {
    @loro_codec.ContainerID::Root(_name, _kind) => true
    @loro_codec.ContainerID::Normal(_peer, _counter, kind) =>
      match kind {
        @loro_codec.ContainerType::Map =>
          match self.inner.map_states.get(container_id_string(cid)) {
            None => false
            Some(_) => true
          }
        @loro_codec.ContainerType::List =>
          match self.inner.list_trackers.get(container_id_string(cid)) {
            None => false
            Some(_) => true
          }
        @loro_codec.ContainerType::Text =>
          match self.inner.text_trackers.get(container_id_string(cid)) {
            None => false
            Some(_) => true
          }
        _ => false
      }
  }
}

fn LoroDoc::container_shallow_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  match cid {
    @loro_codec.ContainerID::Root(_name, kind) =>
      match kind {
        @loro_codec.ContainerType::Map => self.map_shallow_json(cid)
        @loro_codec.ContainerType::List => self.list_shallow_json(cid)
        @loro_codec.ContainerType::Text => Json::string(self.text_to_string(cid))
        _ => Json::null()
      }
    @loro_codec.ContainerID::Normal(_peer, _counter, kind) =>
      match kind {
        @loro_codec.ContainerType::Map => self.map_shallow_json(cid)
        @loro_codec.ContainerType::List => self.list_shallow_json(cid)
        @loro_codec.ContainerType::Text => Json::string(self.text_to_string(cid))
        _ => Json::null()
      }
  }
}

fn LoroDoc::container_deep_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  match cid {
    @loro_codec.ContainerID::Root(_name, kind) =>
      match kind {
        @loro_codec.ContainerType::Map => self.map_deep_json(cid)
        @loro_codec.ContainerType::List => self.list_deep_json(cid)
        @loro_codec.ContainerType::MovableList => Json::array([])
        @loro_codec.ContainerType::Text => Json::string(self.text_to_string(cid))
        @loro_codec.ContainerType::Tree => Json::array([])
        _ => Json::null()
      }
    @loro_codec.ContainerID::Normal(_peer, _counter, kind) =>
      match kind {
        @loro_codec.ContainerType::Map => self.map_deep_json(cid)
        @loro_codec.ContainerType::List => self.list_deep_json(cid)
        @loro_codec.ContainerType::MovableList => Json::array([])
        @loro_codec.ContainerType::Text => Json::string(self.text_to_string(cid))
        @loro_codec.ContainerType::Tree => Json::array([])
        _ => Json::null()
      }
  }
}
