//! Trusted local-file adapter for CG-24. Keep the directory outside model/worker write access.
use gateway_application::procedure_promotion::{PromotionError, PromotionStore};
use gateway_domain::procedure_promotion::{PromotionEvent, PromotionJournal};
use gateway_registry::learned_procedures::LearnedProcedureRegistry;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct FilePromotionStore {
    path: PathBuf,
}
fn storage(error: impl std::fmt::Display) -> PromotionError {
    PromotionError::Store(error.to_string())
}
struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
impl FilePromotionStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, PromotionError> {
        let store = Self { path: path.into() };
        store.load()?;
        Ok(store)
    }
    fn parent(&self) -> &Path {
        self.path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    }
    fn sidecar(&self, suffix: &str) -> PathBuf {
        let mut name = self.path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }
}
impl PromotionStore for FilePromotionStore {
    fn load(&self) -> Result<PromotionJournal, PromotionError> {
        let journal = match fs::read_to_string(&self.path) {
            Ok(json) => serde_json::from_str(&json).map_err(storage)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                PromotionJournal::default()
            }
            Err(error) => return Err(storage(error)),
        };
        LearnedProcedureRegistry::from_journal(&journal)?;
        Ok(journal)
    }
    fn append(
        &mut self,
        expected_revision: usize,
        event: PromotionEvent,
    ) -> Result<(), PromotionError> {
        let lock_path = self.sidecar(".lock");
        let lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    PromotionError::Conflict
                } else {
                    storage(error)
                }
            })?;
        let _lock_cleanup = RemoveOnDrop(lock_path);
        let mut journal = self.load()?;
        if journal.events.len() != expected_revision {
            return Err(PromotionError::Conflict);
        }
        journal.events.push(event);
        LearnedProcedureRegistry::from_journal(&journal)?;
        let next_path = self.sidecar(".next");
        let mut next = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&next_path)
            .map_err(storage)?;
        let _next_cleanup = RemoveOnDrop(next_path.clone());
        next.write_all(&serde_json::to_vec_pretty(&journal).map_err(storage)?)
            .map_err(storage)?;
        next.sync_all().map_err(storage)?;
        drop(next);
        fs::rename(next_path, &self.path).map_err(storage)?;
        // Rename is the atomic commit. If directory fsync fails, callers must reload;
        // an I/O error at this point may already have committed the event.
        fs::File::open(self.parent())
            .and_then(|parent| parent.sync_all())
            .map_err(storage)?;
        drop(lock);
        Ok(())
    }
}
