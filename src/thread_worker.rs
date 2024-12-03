//! Small utility to correctly spawn crossbeam-channel based worker threads.
//! Original source: https://github.com/rust-analyzer/rust-analyzer/blob/c7ceea82a5ab8aabab2f98e7c1e1ec94e82087c2/crates/thread_worker/src/lib.rs

use std::panic;
use std::thread;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use crate::editor_transport::send_command_to_editor;
use crate::editor_transport::send_command_to_editor_here;
use crate::editor_transport::ToEditorSender;
use crate::EditorResponse;
use crate::SessionId;

#[derive(Clone)]
pub enum ToEditorDispatcher {
    ThisThread(SessionId),
    OtherThread(ToEditorSender),
}

impl ToEditorDispatcher {
    pub fn send(&self, response: EditorResponse) {
        match self {
            ToEditorDispatcher::ThisThread(session) => {
                send_command_to_editor_here(session, response);
            }
            ToEditorDispatcher::OtherThread(to_editor) => {
                send_command_to_editor(to_editor, response)
            }
        }
    }
}

/// Like `std::thread::JoinHandle<()>`, but joins thread in drop automatically.
pub struct ScopedThread {
    // Option for drop
    inner: Option<thread::JoinHandle<()>>,
    to_editor_dispatcher: ToEditorDispatcher,
}

impl Drop for ScopedThread {
    fn drop(&mut self) {
        let inner = self.inner.take().unwrap();
        let name = inner.thread().name().unwrap().to_string();
        debug!(dispatcher:self.to_editor_dispatcher, "Waiting for {} to finish...", name);
        let res = inner.join();
        debug!(dispatcher:self.to_editor_dispatcher,
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

impl ScopedThread {
    pub fn spawn(
        to_editor: ToEditorSender,
        name: &'static str,
        f: impl FnOnce(ToEditorSender) + Send + 'static,
    ) -> ScopedThread {
        let to_editor_copy = to_editor.clone();
        let inner = thread::Builder::new()
            .name(name.into())
            .spawn(|| f(to_editor_copy))
            .unwrap();
        ScopedThread {
            inner: Some(inner),
            to_editor_dispatcher: ToEditorDispatcher::OtherThread(to_editor),
        }
    }
    pub fn spawn_to_editor_dispatcher(
        session: SessionId,
        name: &'static str,
        f: impl FnOnce() + Send + 'static,
    ) -> ScopedThread {
        let inner = thread::Builder::new().name(name.into()).spawn(f).unwrap();
        ScopedThread {
            inner: Some(inner),
            to_editor_dispatcher: ToEditorDispatcher::ThisThread(session),
        }
    }
}

/// A wrapper around event-processing thread with automatic shutdown semantics.
pub struct Worker<I, O> {
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
    _thread: ScopedThread,
    receiver: Receiver<O>,
}

impl<I, O> Worker<I, O> {
    pub fn spawn<F>(to_editor: ToEditorSender, name: &'static str, buf: usize, f: F) -> Worker<I, O>
    where
        F: FnOnce(ToEditorSender, Receiver<I>, Sender<O>) + Send + 'static,
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
    pub fn spawn_to_editor_dispatcher<F>(
        session: SessionId,
        name: &'static str,
        buf: usize,
        f: F,
    ) -> Worker<I, O>
    where
        F: FnOnce(Receiver<I>, Sender<O>) + Send + 'static,
        I: Send + 'static,
        O: Send + 'static,
    {
        // Set up worker channels in a deadlock-avoiding way. If one sets both input
        // and output buffers to a fixed size, a worker might get stuck.
        let (sender, input_receiver) = bounded::<I>(buf);
        let (output_sender, receiver) = unbounded::<O>();
        let _thread = ScopedThread::spawn_to_editor_dispatcher(session, name, move || {
            f(input_receiver, output_sender)
        });
        Worker {
            sender,
            _thread,
            receiver,
        }
    }
}

impl<I, O> Worker<I, O> {
    pub fn sender(&self) -> &Sender<I> {
        &self.sender
    }
    pub fn receiver(&self) -> &Receiver<O> {
        &self.receiver
    }
}
