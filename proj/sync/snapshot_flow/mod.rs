use std::cell::RefCell;

use super::dispatch::SnapshotFlowNotifier;
use crate::snapshot::{
    SnapshotCleanupScope, SnapshotEntryKind, SnapshotError, SnapshotListedPaths, SnapshotStore,
};
use crate::{CopyResult, EntryKind, EntryMeta, PeerId, RelPath, RunConfig, Timestamp};

pub(super) struct SnapshotFlow<'a> {
    stores: Vec<&'a mut SnapshotStore>,
    notifier_errors: Vec<SnapshotFlowError>,
}

impl<'a> SnapshotFlow<'a> {
    pub(super) fn new(stores: Vec<&'a mut SnapshotStore>) -> Self {
        Self {
            stores,
            notifier_errors: Vec::new(),
        }
    }

    pub(super) fn take_notifier_errors(&mut self) -> Vec<SnapshotFlowError> {
        std::mem::take(&mut self.notifier_errors)
    }

    pub(super) fn confirmed_present(
        &mut self,
        peer_id: PeerId,
        path: &RelPath,
        meta: &EntryMeta,
    ) -> SnapshotFlowResult<Timestamp> {
        self.store_mut(peer_id, path, SnapshotFlowEvent::ConfirmedPresent)?
            .upsert_confirmed_present(path, meta)
            .map_err(|source| {
                SnapshotFlowError::store(
                    peer_id,
                    path,
                    SnapshotFlowEvent::ConfirmedPresent,
                    source,
                )
            })
    }

    pub(super) fn intended_copy(
        &mut self,
        peer_id: PeerId,
        path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> SnapshotFlowResult<()> {
        self.store_mut(peer_id, path, SnapshotFlowEvent::IntendedCopy)?
            .upsert_intended_copy(path, winning_meta)
            .map_err(|source| {
                SnapshotFlowError::store(peer_id, path, SnapshotFlowEvent::IntendedCopy, source)
            })
    }

    pub(super) fn copy_completed(
        &mut self,
        result: &CopyResult,
    ) -> SnapshotFlowResult<Option<Timestamp>> {
        if !copy_succeeded(result) {
            return Ok(None);
        }

        let peer_id = result.destination_peer_id;
        let path = &result.destination_path;
        self.store_mut(peer_id, path, SnapshotFlowEvent::CopyCompleted)?
            .mark_copy_complete(path)
            .map(Some)
            .map_err(|source| {
                SnapshotFlowError::store(peer_id, path, SnapshotFlowEvent::CopyCompleted, source)
            })
    }

    pub(super) fn directory_created(
        &mut self,
        peer_id: PeerId,
        path: &RelPath,
        meta: &EntryMeta,
    ) -> SnapshotFlowResult<Timestamp> {
        self.store_mut(peer_id, path, SnapshotFlowEvent::DirectoryCreated)?
            .upsert_confirmed_present(path, meta)
            .map_err(|source| {
                SnapshotFlowError::store(
                    peer_id,
                    path,
                    SnapshotFlowEvent::DirectoryCreated,
                    source,
                )
            })
    }

    pub(super) fn confirmed_absent(
        &mut self,
        peer_id: PeerId,
        path: &RelPath,
    ) -> SnapshotFlowResult<()> {
        self.store_mut(peer_id, path, SnapshotFlowEvent::ConfirmedAbsent)?
            .mark_absent(path)
            .map_err(|source| {
                SnapshotFlowError::store(peer_id, path, SnapshotFlowEvent::ConfirmedAbsent, source)
            })
    }

    pub(super) fn displaced(
        &mut self,
        peer_id: PeerId,
        path: &RelPath,
        kind: EntryKind,
    ) -> SnapshotFlowResult<()> {
        self.store_mut(peer_id, path, SnapshotFlowEvent::Displaced)?
            .mark_displaced(path, kind)
            .map_err(|source| {
                SnapshotFlowError::store(peer_id, path, SnapshotFlowEvent::Displaced, source)
            })
    }

    pub(super) fn cleanup_stale_rows(
        &mut self,
        peer_id: PeerId,
        listed_paths: &dyn SnapshotListedPaths,
        keep_del_days: u32,
    ) -> SnapshotFlowResult<()> {
        let event = SnapshotFlowEvent::Cleanup;
        let store = self
            .stores
            .iter_mut()
            .find(|store| store.peer() == peer_id)
            .map(|store| &mut **store)
            .ok_or_else(|| SnapshotFlowError::missing_store(peer_id, None, event))?;

        store
            .cleanup_stale_rows(SnapshotCleanupScope {
                listed_paths,
                keep_del_days,
            })
            .map_err(|source| SnapshotFlowError {
                peer_id,
                path: None,
                event,
                kind: SnapshotFlowErrorKind::Store(source),
            })
    }

    pub(super) fn cleanup_stale_rows_from_config(
        &mut self,
        peer_id: PeerId,
        listed_paths: &dyn SnapshotListedPaths,
        config: &RunConfig,
    ) -> SnapshotFlowResult<()> {
        self.cleanup_stale_rows(
            peer_id,
            listed_paths,
            keep_del_days_from_config(config),
        )
    }

    fn store_mut(
        &mut self,
        peer_id: PeerId,
        path: &RelPath,
        event: SnapshotFlowEvent,
    ) -> SnapshotFlowResult<&mut SnapshotStore> {
        self.stores
            .iter_mut()
            .find(|store| store.peer() == peer_id)
            .map(|store| &mut **store)
            .ok_or_else(|| SnapshotFlowError::missing_store(peer_id, Some(path), event))
    }

    fn record_notifier_error(&mut self, error: SnapshotFlowError) {
        self.notifier_errors.push(error);
    }
}

pub(super) type SnapshotFlowResult<T> = Result<T, SnapshotFlowError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotFlowError {
    pub peer_id: PeerId,
    pub path: Option<RelPath>,
    pub event: SnapshotFlowEvent,
    pub kind: SnapshotFlowErrorKind,
}

impl SnapshotFlowError {
    fn store(
        peer_id: PeerId,
        path: &RelPath,
        event: SnapshotFlowEvent,
        source: SnapshotError,
    ) -> Self {
        Self {
            peer_id,
            path: Some(path.clone()),
            event,
            kind: SnapshotFlowErrorKind::Store(source),
        }
    }

    fn missing_store(
        peer_id: PeerId,
        path: Option<&RelPath>,
        event: SnapshotFlowEvent,
    ) -> Self {
        Self {
            peer_id,
            path: path.cloned(),
            event,
            kind: SnapshotFlowErrorKind::MissingStore,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SnapshotFlowErrorKind {
    Store(SnapshotError),
    MissingStore,
    InvalidDisplacementKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SnapshotFlowEvent {
    ConfirmedPresent,
    IntendedCopy,
    CopyCompleted,
    DirectoryCreated,
    ConfirmedAbsent,
    Displaced,
    Cleanup,
}

pub(super) fn record_confirmed_present(
    store: &mut SnapshotStore,
    path: &RelPath,
    meta: &EntryMeta,
) -> Result<Timestamp, SnapshotError> {
    store.upsert_confirmed_present(path, meta)
}

pub(super) fn record_intended_copy(
    store: &mut SnapshotStore,
    path: &RelPath,
    winning_meta: &EntryMeta,
) -> Result<(), SnapshotError> {
    store.upsert_intended_copy(path, winning_meta)
}

pub(super) fn record_copy_completed(
    store: &mut SnapshotStore,
    result: &CopyResult,
) -> Result<Option<Timestamp>, SnapshotError> {
    if copy_succeeded(result) {
        store.mark_copy_complete(&result.destination_path).map(Some)
    } else {
        Ok(None)
    }
}

pub(super) fn record_directory_created(
    store: &mut SnapshotStore,
    path: &RelPath,
    meta: &EntryMeta,
) -> Result<Timestamp, SnapshotError> {
    store.upsert_confirmed_present(path, meta)
}

pub(super) fn record_confirmed_absent(
    store: &mut SnapshotStore,
    path: &RelPath,
) -> Result<(), SnapshotError> {
    store.mark_absent(path)
}

pub(super) fn record_displaced(
    store: &mut SnapshotStore,
    path: &RelPath,
    kind: EntryKind,
) -> Result<(), SnapshotError> {
    store.mark_displaced(path, kind)
}

pub(super) fn request_cleanup(
    store: &mut SnapshotStore,
    listed_paths: &dyn SnapshotListedPaths,
    keep_del_days: u32,
) -> Result<(), SnapshotError> {
    store.cleanup_stale_rows(SnapshotCleanupScope {
        listed_paths,
        keep_del_days,
    })
}

pub(super) fn keep_del_days_from_config(config: &RunConfig) -> u32 {
    config.keep_del_days
}

fn copy_succeeded(result: &CopyResult) -> bool {
    result.completed && result.error.is_none()
}

impl SnapshotFlowNotifier for RefCell<SnapshotFlow<'_>> {
    fn intended_copy(&self, peer_id: PeerId, path: &RelPath, winning_meta: &EntryMeta) {
        let result = self
            .borrow_mut()
            .intended_copy(peer_id, path, winning_meta);
        if let Err(error) = result {
            self.borrow_mut().record_notifier_error(error);
        }
    }

    fn directory_created(&self, peer_id: PeerId, path: &RelPath, meta: &EntryMeta) {
        let result = self.borrow_mut().directory_created(peer_id, path, meta);
        if let Err(error) = result {
            self.borrow_mut().record_notifier_error(error);
        }
    }

    fn displaced(&self, peer_id: PeerId, path: &RelPath, kind: SnapshotEntryKind) {
        let Some(kind) = entry_kind_from_snapshot_kind(kind) else {
            self.borrow_mut()
                .record_notifier_error(SnapshotFlowError {
                    peer_id,
                    path: Some(path.clone()),
                    event: SnapshotFlowEvent::Displaced,
                    kind: SnapshotFlowErrorKind::InvalidDisplacementKind,
                });
            return;
        };

        let result = self.borrow_mut().displaced(peer_id, path, kind);
        if let Err(error) = result {
            self.borrow_mut().record_notifier_error(error);
        }
    }

    fn copy_completed(&self, result: &CopyResult) {
        let result = self.borrow_mut().copy_completed(result);
        if let Err(error) = result {
            self.borrow_mut().record_notifier_error(error);
        }
    }
}

fn entry_kind_from_snapshot_kind(kind: SnapshotEntryKind) -> Option<EntryKind> {
    match kind {
        SnapshotEntryKind::File => Some(EntryKind::File),
        SnapshotEntryKind::Directory => Some(EntryKind::Directory),
        SnapshotEntryKind::Tombstone => None,
    }
}
