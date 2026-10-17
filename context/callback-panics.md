# User Callbacks That Panic

Verified against code 2026-10-16.

Subscribers, pre-commit callbacks, and local-update callbacks are user code. A
panic in one of them reaches the caller of the Loro method that ran it. It must
not break the document. That means it must not poison a doc lock, lose another
subscriber's event or a commit's local update, or hang another thread. This only
applies where panics unwind. Under `panic = "abort"` (WASM) nothing is caught.

## Where each guard is

- `SubscriberSet::retain` (`utils/subscription.rs`) checks an emitter's
  subscribers out and leaves a thread-id marker in their place. It runs every
  callback under `DeferredPanic`, puts the subscribers back, and only then raises
  the first panic again. Before this, the subscribers were dropped with the
  marker left behind. Later emits on the same thread then queued their events
  forever, and emits on other threads slept in the `retain` loop forever.
- `DeferredPanic` runs steps that must all happen even if one panics, and raises
  the first panic at the end. It is used by `SubscriberSetWithQueue::emit`
  (queued payloads), `Observer::emit` (container-level sets, the root set, and
  queued events), `LoroDoc::emit_events`, and the commit's `on_commit`
  (`txn.rs:txn_with_origin`). There the events are already taken out of the
  state, and the local update is what apps send to their peers.
- Pre-commit callbacks run in `Transaction::_commit` while `commit_internal`
  holds the txn lock, the change is applied to the state, and the op log holds
  it as uncommitted. `_commit` catches the panic and finishes the commit as if the
  callback had returned. Then `Transaction::commit` raises it at its end,
  `commit_internal` raises it after it has released the txn guard, and `Drop`
  raises it only if the thread is not already unwinding.
- `LoroDoc::with_barrier` runs its closure under `catch_unwind`, releases the txn
  guard, renews the txn, and raises the panic again. Import events are emitted
  inside the barrier, so a subscriber that panicked during `import` used to
  poison the txn lock.

## Why pre-commit callbacks stay under the txn lock

The txn lock is what keeps other threads from starting a transaction or
committing while a commit is half done: the state is in txn mode and
`OpLog::uncommitted_change` is set. Releasing it around the callback would need
another "commit in progress" guard with the same effect. It would also let the
callback re-enter the doc in the middle of the commit.

## What this does not cover

A panic inside Loro (a bug) while it holds the op log or state lock still
poisons that lock, and `LoroMutex` turns every later lock into a panic.
`with_barrier` keeps the txn lock clean and re-raises the original panic, not the
poisoned-lock panic of its cleanup. Dropping a doc with a poisoned lock still
panics (`LoroDoc::drop` → `implicit_commit_then_stop`).

## Tests

`crates/loro/tests/callback_panic.rs`: a panicking pre-commit callback, root
subscriber, and local-update subscriber, and a subscriber that panics during
`import`. On the previous code the pre-commit and import cases poisoned the txn
lock and aborted the test process when the doc was dropped. The subscriber case
lost events and hung the other thread.
