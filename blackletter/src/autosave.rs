//! Autosave: debounced, atomic, and idle-clean.
//!
//! Documents register a saver closure; a wake lands when a document marks
//! itself dirty; the task sleeps 2 s (the debounce), then runs every saver —
//! each saver decides for itself whether anything was dirty and returns
//! whether it wrote. With nothing dirty the task is parked on the channel:
//! zero wakes, zero frames (shale idle rule).
//!
//! Writes are atomic on both OSes: temp file in the same directory, then
//! `rename` over the target, keeping one `.bak` of the previous version.

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AsyncApp, EntityId, Global, Task};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Debounce window between the first dirty mark and the flush.
pub const DEBOUNCE: Duration = Duration::from_millis(2000);

type Saver = Box<dyn FnMut(&mut App) -> bool>;

pub struct Autosave {
    savers: HashMap<EntityId, Saver>,
    // Read by `mark_dirty`; the whole wake path is consumed by the document
    // entities that arrive in Phase 2.
    #[allow(dead_code)]
    tx: UnboundedSender<()>,
    // The task handle lives here so it is never dropped (a dropped gpui Task
    // silently cancels).
    _task: Task<()>,
}

impl Global for Autosave {}

impl Autosave {
    pub fn init(cx: &mut App) {
        let (tx, mut rx) = unbounded();
        cx.set_global(Self {
            savers: HashMap::new(),
            tx,
            _task: Task::ready(()),
        });
        let task = cx.spawn(async move |cx: &mut AsyncApp| {
            // Parked here until a document goes dirty — no timer, no wake.
            while rx.next().await.is_some() {
                cx.background_executor().timer(DEBOUNCE).await;
                // Coalesce: wakes that landed during the debounce are spent.
                while rx.try_recv().is_ok() {}
                Self::flush(cx);
            }
        });
        cx.global_mut::<Autosave>()._task = task;
    }

    fn flush(cx: &mut AsyncApp) {
        cx.update(|cx| {
            // Move the map out so savers may touch the App (and the global)
            // while running; put it back afterwards.
            let mut savers = std::mem::take(&mut cx.global_mut::<Autosave>().savers);
            for saver in savers.values_mut() {
                saver(cx);
            }
            cx.global_mut::<Autosave>().savers = savers;
        });
    }

    /// Register a document's saver. The closure returns true when it wrote.
    /// (Document entities arrive in Phase 2; quiet until then.)
    #[allow(dead_code)]
    pub fn register(&mut self, id: EntityId, saver: Saver) {
        self.savers.insert(id, saver);
    }

    #[allow(dead_code)]
    pub fn unregister(&mut self, id: EntityId) {
        self.savers.remove(&id);
    }

    /// A document went dirty: wake the flusher.
    #[allow(dead_code)]
    pub fn mark_dirty(&mut self, _id: EntityId) {
        let _ = self.tx.unbounded_send(());
    }
}

/// Write `bytes` to `path` atomically, keeping one `.bak` of the previous file.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "doc".to_string());
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        let bak_name = format!("{}.bak", path.display());
        let bak = Path::new(&bak_name);
        // On Windows a plain rename over an existing file fails; clear it first.
        if std::fs::rename(path, bak).is_err() {
            let _ = std::fs::remove_file(bak);
            std::fs::rename(path, bak)?;
        }
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn atomic_write_rotates_bak() {
        let dir = std::env::temp_dir().join("blackletter_atomic_test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("doc.json");

        atomic_write(&path, b"v1").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"v1");
        assert!(!dir.join("doc.json.bak").exists());

        atomic_write(&path, b"v2").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"v2");
        assert_eq!(std::fs::read(dir.join("doc.json.bak")).unwrap(), b"v1");

        // No temp litter left behind.
        assert!(!dir.join(".doc.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn flush_runs_only_after_a_dirty_wake(cx: &mut TestAppContext) {
        cx.update(Autosave::init);
        let writes = Rc::new(Cell::new(0));
        let w = writes.clone();
        let doc = EntityId::from(1u64);
        cx.update(|cx| {
            cx.global_mut::<Autosave>().register(
                doc,
                Box::new(move |_cx| {
                    w.set(w.get() + 1);
                    true
                }),
            );
        });

        // Nothing dirty: advancing the clock must not flush.
        cx.background_executor.advance_clock(Duration::from_secs(5));
        cx.run_until_parked();
        assert_eq!(writes.get(), 0);

        // One wake: exactly one flush after the debounce.
        cx.update(|cx| cx.global_mut::<Autosave>().mark_dirty(doc));
        cx.background_executor
            .advance_clock(DEBOUNCE + Duration::from_millis(100));
        cx.run_until_parked();
        assert_eq!(writes.get(), 1);

        // Bursts inside the debounce window coalesce into one flush.
        for _ in 0..3 {
            cx.update(|cx| cx.global_mut::<Autosave>().mark_dirty(doc));
        }
        cx.background_executor
            .advance_clock(DEBOUNCE + Duration::from_millis(100));
        cx.run_until_parked();
        assert_eq!(writes.get(), 2);
    }
}
