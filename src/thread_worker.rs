//! Small utility to correctly spawn crossbeam-channel based worker threads.
//! Original source: https://github.com/rust-analyzer/rust-analyzer/blob/c7ceea82a5ab8aabab2f98e7c1e1ec94e82087c2/crates/thread_worker/src/lib.rs

use std::panic;
use std::thread;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use crate::ToEditor;

/// Like `std::thread::JoinHandle<()>`, but joins thread in drop automatically.
pub struct ScopedThread<T: ToEditor> {
    // Option for drop
    inner: Option<thread::JoinHandle<()>>,
    to_editor: T,
}

impl<T: ToEditor> Drop for ScopedThread<T> {
    fn drop(&mut self) {
        let inner = self.inner.take().unwrap();
        let name = inner.thread().name().unwrap().to_string();
        debug!(&self.to_editor, "Waiting for {} to finish...", name);
        let res = inner.join();
        debug!(
            &self.to_editor,
            "... {} terminated with {}",
            name,
            if res.is_ok() { "ok" } else { "err" }
        );

        // escalate panic, but avoid aborting the process
        if let Err(e) = res {
            panic::panic_any(e);
        }
    }
}

impl<T: ToEditor + Clone + Send + 'static> ScopedThread<T> {
    pub fn spawn(
        to_editor: T,
        name: &'static str,
        f: impl FnOnce(T) + Send + 'static,
    ) -> ScopedThread<T> {
        let to_editor_copy = to_editor.clone();
        let inner = thread::Builder::new()
            .name(name.into())
            .spawn(|| f(to_editor_copy))
            .unwrap();
        ScopedThread {
            inner: Some(inner),
            to_editor,
        }
    }
}

/// A wrapper around event-processing thread with automatic shutdown semantics.
pub struct Worker<T: ToEditor, I, O> {
    // XXX: field order is significant here.
    //
    // In Rust, fields are dropped in the declaration order, and we rely on this
    // here. We must close input first, so that the  `thread` (who holds the
    // opposite side of the channel) noticed shutdown. Then, we must join the
    // thread, but we must keep out alive so that the thread does not panic.
    //
    // Note that a potential problem here is that we might drop some messages
    // from receiver on the floor. This is ok for rust-analyzer: we have only a
    // single client, so, if we are shutting down, nobody is interested in the
    // unfinished work anyway! (It's okay for kakoune-lsp too).
    sender: Sender<I>,
    _thread: ScopedThread<T>,
    receiver: Receiver<O>,
}

impl<T: ToEditor + Clone + Send + 'static, I, O> Worker<T, I, O> {
    pub fn spawn<F>(to_editor: T, name: &'static str, buf: usize, f: F) -> Worker<T, I, O>
    where
        T: ToEditor,
        F: FnOnce(T, Receiver<I>, Sender<O>) + Send + 'static,
        I: Send + 'static,
        O: Send + 'static,
    {
        // Set up worker channels in a deadlock-avoiding way. If one sets both input
        // and output buffers to a fixed size, a worker might get stuck.
        let (sender, input_receiver) = bounded::<I>(buf);
        let (output_sender, receiver) = unbounded::<O>();
        let _thread = ScopedThread::spawn(to_editor, name, move |to_editor| {
            f(to_editor, input_receiver, output_sender)
        });
        Worker {
            sender,
            _thread,
            receiver,
        }
    }
}

impl<T: ToEditor, I, O> Worker<T, I, O> {
    pub fn sender(&self) -> &Sender<I> {
        &self.sender
    }
    pub fn receiver(&self) -> &Receiver<O> {
        &self.receiver
    }
}
