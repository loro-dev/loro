use either::Either;
/*
This file is modified from the original file in the following repo:
https://github.com/zed-industries/zed


Copyright 2022 - 2024 Zed Industries, Inc.

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at


       http://www.apache.org/licenses/LICENSE-2.0


   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.




Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/


   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION


   1. Definitions.


      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.


      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.


      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.


      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.


      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.


      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.


      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).


      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.


      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."


      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.


   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.


   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.


   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:


      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and


      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and


      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and


      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.


      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.


   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.


   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.


   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.


   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.


   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.


   END OF TERMS AND CONDITIONS

*/
use crate::sync::{thread, AtomicBool, Mutex};
use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::{fmt::Debug, mem};
use thread::ThreadId;

#[derive(Debug)]
pub enum SubscriptionError {
    CannotEmitEventDueToRecursiveCall,
}

impl std::fmt::Display for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SubscriptionError")
    }
}

pub struct SubscriberSet<EmitterKey, Callback>(
    Arc<Mutex<SubscriberSetState<EmitterKey, Callback>>>,
);

impl<EmitterKey, Callback> Clone for SubscriberSet<EmitterKey, Callback> {
    fn clone(&self) -> Self {
        SubscriberSet(self.0.clone())
    }
}

struct SubscriberSetState<EmitterKey, Callback> {
    subscribers: BTreeMap<EmitterKey, Either<BTreeMap<usize, Subscriber<Callback>>, ThreadId>>,
    /// Subscribers added while their emitter was mid-emit.
    ///
    /// `retain` swaps an emitter's map out for a `ThreadId` marker
    /// while it invokes callbacks, so during that window there is no
    /// map to insert into. The marker cannot simply be replaced: it is
    /// what makes a concurrent `retain` wait instead of emitting the
    /// same emitter twice, and what lets `is_recursive_calling` spot a
    /// re-entrant emit. Parking the new subscriber here keeps the
    /// marker intact, and `retain` folds these back in when it
    /// restores the map.
    pending_subscribers: BTreeMap<EmitterKey, BTreeMap<usize, Subscriber<Callback>>>,
    dropped_subscribers: BTreeSet<(EmitterKey, usize)>,
    next_subscriber_id: usize,
}

struct Subscriber<Callback> {
    active: Arc<AtomicBool>,
    callback: Callback,
    /// This field is used to drop the subscription when the subscriber is dropped.
    _sub: InnerSubscription,
}

impl<EmitterKey, Callback> SubscriberSet<EmitterKey, Callback>
where
    EmitterKey: 'static + Ord + Clone + Debug + Send + Sync,
    Callback: 'static + Send + Sync,
{
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(SubscriberSetState {
            subscribers: Default::default(),
            pending_subscribers: Default::default(),
            dropped_subscribers: Default::default(),
            next_subscriber_id: 0,
        })))
    }

    /// Inserts a new [`Subscription`] for the given `emitter_key`.
    ///
    /// By default, subscriptions are inert, meaning that they won't be listed when calling
    /// `[SubscriberSet::remove]` or `[SubscriberSet::retain]`. This method returns a tuple
    /// of a [`Subscription`] and an `impl FnOnce`, and you can use the latter to activate
    /// the [`Subscription`].
    pub fn insert(
        &self,
        emitter_key: EmitterKey,
        callback: Callback,
    ) -> (Subscription, impl FnOnce()) {
        let active = Arc::new(AtomicBool::new(false));
        let mut lock = self.0.lock();
        let subscriber_id = post_inc(&mut lock.next_subscriber_id);
        let this = Arc::downgrade(&self.0);
        let emitter_key_1 = emitter_key.clone();
        let inner_sub = InnerSubscription {
            unsubscribe: Arc::new(Mutex::new(Some(Box::new(move || {
                let Some(this) = this.upgrade() else {
                    return;
                };

                let mut lock = this.lock();
                let Some(subscribers) = lock.subscribers.get_mut(&emitter_key) else {
                    // remove was called with this emitter_key
                    return;
                };

                if let Either::Left(subscribers) = subscribers {
                    subscribers.remove(&subscriber_id);
                    if subscribers.is_empty() {
                        lock.subscribers.remove(&emitter_key);
                    }
                    return;
                }

                // We didn't manage to remove the subscription, which means it was dropped
                // while invoking the callback. Mark it as dropped so that we can remove it
                // later.
                lock.dropped_subscribers
                    .insert((emitter_key, subscriber_id));
            })))),
        };
        let subscription = Subscription {
            unsubscribe: Arc::downgrade(&inner_sub.unsubscribe),
        };

        let subscriber = Subscriber {
            active: active.clone(),
            callback,
            _sub: inner_sub,
        };
        // Subscribing to an emitter that is mid-emit is legal and
        // happens whenever one thread adds a listener while another is
        // delivering events for the same key (or a callback subscribes
        // re-entrantly). The map is checked out by `retain` in that
        // window, so park the subscriber instead of unwrapping a
        // `Left` that is not there.
        if matches!(lock.subscribers.get(&emitter_key_1), Some(Either::Right(_))) {
            lock.pending_subscribers
                .entry(emitter_key_1)
                .or_default()
                .insert(subscriber_id, subscriber);
        } else {
            lock.subscribers
                .entry(emitter_key_1)
                .or_insert_with(|| Either::Left(BTreeMap::new()))
                .as_mut()
                .unwrap_left()
                .insert(subscriber_id, subscriber);
        }
        (subscription, move || active.store(true, Ordering::Relaxed))
    }

    #[allow(unused)]
    pub fn remove(&self, emitter: &EmitterKey) -> impl IntoIterator<Item = Callback> {
        let mut lock = self.0.lock();
        let subscribers = lock.subscribers.remove(emitter);
        // A subscriber parked mid-emit belongs to the emitter being
        // removed; it never became visible, so it goes with it.
        lock.pending_subscribers.remove(emitter);
        subscribers
            .and_then(|x| x.left().map(|s| s.into_values()))
            .into_iter()
            .flatten()
            .filter_map(|subscriber| {
                if subscriber.active.load(Ordering::Relaxed) {
                    Some(subscriber.callback)
                } else {
                    None
                }
            })
    }

    pub fn is_recursive_calling(&self, emitter: &EmitterKey) -> bool {
        if let Some(Either::Right(thread_id)) = self.0.lock().subscribers.get(emitter) {
            *thread_id == thread::current().id()
        } else {
            false
        }
    }

    /// Call the given callback for each subscriber to the given emitter.
    /// If the callback returns false, the subscriber is removed.
    pub fn retain(
        &self,
        emitter: &EmitterKey,
        f: &mut dyn FnMut(&mut Callback) -> bool,
    ) -> Result<(), SubscriptionError> {
        let mut subscribers = {
            let inner = loop {
                let mut subscriber_set_state = self.0.lock();
                let Some(set) = subscriber_set_state.subscribers.get_mut(emitter) else {
                    return Ok(());
                };
                match set {
                    Either::Left(_) => {
                        break std::mem::replace(set, Either::Right(thread::current().id()))
                            .unwrap_left();
                    }
                    Either::Right(lock_thread) => {
                        if thread::current().id() == *lock_thread {
                            return Err(SubscriptionError::CannotEmitEventDueToRecursiveCall);
                        } else {
                            // return Ok(());
                            drop(subscriber_set_state);
                            #[cfg(loom)]
                            loom::thread::yield_now();
                            #[cfg(not(loom))]
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                }
            };
            inner
        };

        subscribers.retain(|_, subscriber| {
            if subscriber.active.load(Ordering::Relaxed) {
                f(&mut subscriber.callback)
            } else {
                true
            }
        });

        let mut lock = self.0.lock();

        // Add any new subscribers that were added while invoking the callback.
        if let Some(Either::Left(new_subscribers)) = lock.subscribers.remove(emitter) {
            subscribers.extend(new_subscribers);
        }
        // …including those parked because this emitter was checked
        // out. Folded in BEFORE the dropped sweep below, so one that
        // was unsubscribed again mid-emit is still dropped.
        if let Some(parked) = lock.pending_subscribers.remove(emitter) {
            subscribers.extend(parked);
        }

        // Remove any dropped subscriptions that were dropped while invoking the callback.
        for (dropped_emitter, dropped_subscription_id) in mem::take(&mut lock.dropped_subscribers) {
            if *emitter == dropped_emitter {
                subscribers.remove(&dropped_subscription_id);
            } else {
                lock.dropped_subscribers
                    .insert((dropped_emitter, dropped_subscription_id));
            }
        }

        lock.subscribers
            .insert(emitter.clone(), Either::Left(subscribers));
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().subscribers.is_empty()
    }

    pub fn may_include(&self, emitter: &EmitterKey) -> bool {
        self.0.lock().subscribers.contains_key(emitter)
    }
}

impl<EmitterKey, Callback> Default for SubscriberSet<EmitterKey, Callback>
where
    EmitterKey: 'static + Ord + Clone + Debug + Send + Sync,
    Callback: 'static + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<EmitterKey, Callback> std::fmt::Debug for SubscriberSet<EmitterKey, Callback>
where
    EmitterKey: 'static + Ord + Clone + Debug + Send + Sync,
    Callback: 'static + Send + Sync,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lock = self.0.lock();
        f.debug_struct("SubscriberSet")
            .field("subscriber_count", &lock.subscribers.len())
            .field("dropped_subscribers_count", &lock.dropped_subscribers.len())
            .field("next_subscriber_id", &lock.next_subscriber_id)
            .finish()
    }
}

fn post_inc(next_subscriber_id: &mut usize) -> usize {
    let ans = *next_subscriber_id;
    *next_subscriber_id += 1;
    ans
}
type Callback = Box<dyn FnOnce() + 'static + Send + Sync>;

/// A handle to a subscription created by GPUI. When dropped, the subscription
/// is cancelled and the callback will no longer be invoked.
#[must_use]
pub struct Subscription {
    unsubscribe: Weak<Mutex<Option<Callback>>>,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription").finish()
    }
}

impl Subscription {
    /// Detaches the subscription from this handle. The callback will
    /// continue to be invoked until the doc has been subscribed to
    /// are dropped
    pub fn detach(self) {
        if let Some(unsubscribe) = self.unsubscribe.upgrade() {
            unsubscribe.lock().take();
        }
    }

    /// Unsubscribes the subscription.
    #[inline]
    pub fn unsubscribe(self) {
        drop(self)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.upgrade() {
            let unsubscribe = unsubscribe.lock().take();
            if let Some(unsubscribe) = unsubscribe {
                unsubscribe();
            }
        }
    }
}

struct InnerSubscription {
    unsubscribe: Arc<Mutex<Option<Callback>>>,
}

impl Drop for InnerSubscription {
    fn drop(&mut self) {
        self.unsubscribe.lock().take();
    }
}

/// A wrapper around `SubscriberSet` that automatically handles recursive event emission.
///
/// This struct differs from `SubscriberSet` in the following ways:
/// 1. It automatically handles the `CannotEmitEventDueToRecursiveCall` error that can occur in `SubscriberSet`.
/// 2. When a recursive event emission is detected, it queues the event instead of throwing an error.
/// 3. After the current event processing is complete, it automatically processes the queued events.
///
/// This behavior ensures that all events are processed in the order they were emitted, even in cases
/// where recursive event emission would normally cause an error.
#[derive(Clone)]
pub struct SubscriberSetWithQueue<EmitterKey, Callback, Payload> {
    subscriber_set: SubscriberSet<EmitterKey, Callback>,
    queue: Arc<Mutex<BTreeMap<EmitterKey, Vec<Payload>>>>,
}

impl<EmitterKey, Callback, Payload> Debug
    for SubscriberSetWithQueue<EmitterKey, Callback, Payload>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscriberSetWithQueue").finish()
    }
}

impl<EmitterKey, Callback, Payload> Default
    for SubscriberSetWithQueue<EmitterKey, Callback, Payload>
where
    EmitterKey: 'static + Ord + Clone + Debug + Send + Sync,
    Callback: 'static + Send + Sync + for<'a> FnMut(&'a Payload) -> bool,
    Payload: Send + Sync + Debug,
{
    fn default() -> Self {
        Self::new()
    }
}

pub struct WeakSubscriberSetWithQueue<EmitterKey, Callback, Payload> {
    subscriber_set: Weak<Mutex<SubscriberSetState<EmitterKey, Callback>>>,
    queue: Weak<Mutex<BTreeMap<EmitterKey, Vec<Payload>>>>,
}

impl<EmitterKey, Callback, Payload> Debug
    for WeakSubscriberSetWithQueue<EmitterKey, Callback, Payload>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeakSubscriberSetWithQueue").finish()
    }
}

impl<EmitterKey, Callback, Payload> Clone
    for WeakSubscriberSetWithQueue<EmitterKey, Callback, Payload>
{
    fn clone(&self) -> Self {
        Self {
            subscriber_set: self.subscriber_set.clone(),
            queue: self.queue.clone(),
        }
    }
}

impl<EmitterKey, Callback, Payload> WeakSubscriberSetWithQueue<EmitterKey, Callback, Payload> {
    pub fn upgrade(self) -> Option<SubscriberSetWithQueue<EmitterKey, Callback, Payload>> {
        Some(SubscriberSetWithQueue {
            subscriber_set: SubscriberSet(self.subscriber_set.upgrade()?),
            queue: self.queue.upgrade()?,
        })
    }
}

impl<EmitterKey, Callback, Payload> SubscriberSetWithQueue<EmitterKey, Callback, Payload>
where
    EmitterKey: 'static + Ord + Clone + Debug + Send + Sync,
    Callback: 'static + Send + Sync + for<'a> FnMut(&'a Payload) -> bool,
    Payload: Send + Sync + Debug,
{
    pub fn new() -> Self {
        Self {
            subscriber_set: SubscriberSet::new(),
            queue: Arc::new(Mutex::new(Default::default())),
        }
    }

    pub fn downgrade(&self) -> WeakSubscriberSetWithQueue<EmitterKey, Callback, Payload> {
        WeakSubscriberSetWithQueue {
            subscriber_set: Arc::downgrade(&self.subscriber_set.0),
            queue: Arc::downgrade(&self.queue),
        }
    }

    pub fn inner(&self) -> &SubscriberSet<EmitterKey, Callback> {
        &self.subscriber_set
    }

    pub fn emit(&self, key: &EmitterKey, payload: Payload) {
        let mut pending_events: SmallVec<[Payload; 1]> = SmallVec::new();
        pending_events.push(payload);
        while let Some(payload) = pending_events.pop() {
            let result = self
                .subscriber_set
                .retain(key, &mut |callback| (callback)(&payload));
            match result {
                Ok(_) => {
                    let mut queue = self.queue.lock();
                    if let Some(new_pending_events) = queue.remove(key) {
                        pending_events.extend(new_pending_events);
                    }
                }
                Err(SubscriptionError::CannotEmitEventDueToRecursiveCall) => {
                    let mut queue = self.queue.lock();
                    queue.entry(key.clone()).or_default().push(payload);
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_inner_subscription_drop() {
        let subscriber_set = SubscriberSet::<i32, Box<dyn Fn(&i32) -> bool + Send + Sync>>::new();
        let (subscription, activate) = subscriber_set.insert(1, Box::new(move |_: &i32| true));
        activate();
        drop(subscriber_set);
        assert!(subscription.unsubscribe.upgrade().is_none());
    }

    /// Subscribing while the same emitter is mid-emit must not panic.
    /// `retain` checks the emitter's map out and leaves a `ThreadId`
    /// marker in its place, so an `insert` landing in that window used
    /// to `unwrap_left()` the marker and take the process down with
    /// `called Either::unwrap_left() on a Right value`. The new
    /// subscriber is parked and folded in when `retain` restores the
    /// map — so it is registered, but does not fire for the event
    /// already being delivered.
    #[test]
    fn insert_during_emit_does_not_panic() {
        let set = SubscriberSet::<i32, Box<dyn Fn(&i32) -> bool + Send + Sync>>::new();
        let late_fired = Arc::new(AtomicBool::new(false));

        // The first subscriber subscribes AGAIN from inside its own
        // callback: the emitter is checked out at that moment, which
        // is exactly the state that used to panic.
        let set_inner = set.clone();
        let flag = late_fired.clone();
        let (_sub, activate) = set.insert(
            1,
            Box::new(move |_: &i32| {
                let flag = flag.clone();
                let (sub, activate) = set_inner.insert(
                    1,
                    Box::new(move |_: &i32| {
                        flag.store(true, Ordering::Relaxed);
                        true
                    }),
                );
                activate();
                std::mem::forget(sub);
                true
            }),
        );
        activate();

        set.retain(&1, &mut |callback| callback(&1)).unwrap();
        assert!(
            !late_fired.load(Ordering::Relaxed),
            "a subscriber added mid-emit must not receive the event being delivered"
        );

        // The parked subscriber was folded back in, so the NEXT emit
        // reaches it.
        set.retain(&1, &mut |callback| callback(&1)).unwrap();
        assert!(
            late_fired.load(Ordering::Relaxed),
            "a subscriber added mid-emit must be registered for later events"
        );
    }

    /// The cross-thread flavour: one thread emits while another
    /// subscribes to the same emitter.
    #[test]
    fn concurrent_insert_and_emit_do_not_panic() {
        use std::sync::{atomic::AtomicUsize, mpsc};
        let set = SubscriberSet::<i32, Box<dyn Fn(&i32) -> bool + Send + Sync>>::new();

        // The first emit parks inside the callback until released, which
        // keeps the emitter checked out for as long as the test needs.
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let entered_tx = std::sync::Mutex::new(Some(entered_tx));
        let release_rx = std::sync::Mutex::new(release_rx);
        let (_sub, activate) = set.insert(
            1,
            Box::new(move |_: &i32| {
                if let Some(tx) = entered_tx.lock().unwrap().take() {
                    tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                }
                true
            }),
        );
        activate();

        let emitter = set.clone();
        let t = std::thread::spawn(move || {
            emitter.retain(&1, &mut |callback| callback(&1)).unwrap();
        });

        // Only subscribe once the emitting thread is inside the callback,
        // so this insert is guaranteed to land while the emitter is
        // checked out.
        entered_rx.recv().unwrap();
        let late_calls = Arc::new(AtomicUsize::new(0));
        let late_calls_clone = late_calls.clone();
        let (sub, activate) = set.insert(
            1,
            Box::new(move |_: &i32| {
                late_calls_clone.fetch_add(1, Ordering::SeqCst);
                true
            }),
        );
        activate();
        release_tx.send(()).unwrap();
        t.join().unwrap();
        assert_eq!(late_calls.load(Ordering::SeqCst), 0);

        set.retain(&1, &mut |callback| callback(&1)).unwrap();
        assert_eq!(late_calls.load(Ordering::SeqCst), 1);
        drop(sub);
    }

    #[test]
    fn test_inner_subscription_drop_2() {
        let subscriber_set = SubscriberSet::<i32, Box<dyn Fn(&i32) -> bool + Send + Sync>>::new();
        let (subscription, activate) = subscriber_set.insert(1, Box::new(move |_: &i32| false));
        activate();
        subscriber_set
            .retain(&1, &mut |callback| callback(&1))
            .unwrap();
        assert!(subscription.unsubscribe.upgrade().is_none());
    }
}
