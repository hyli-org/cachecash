use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{self, BufReader, BufWriter},
    path::Path,
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Maximum number of notes stored per recipient tag (FIFO eviction).
const DEFAULT_MAX_NOTES_PER_RECIPIENT: usize = 1000;

/// An encrypted note stored on the server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEncryptedNote {
    /// Unique identifier for this note.
    pub id: String,
    /// Base64-encoded encrypted payload.
    pub encrypted_payload: String,
    /// Hex-encoded ephemeral public key used for ECDH.
    pub ephemeral_pubkey: String,
    /// Optional sender tag for filtering/grouping.
    pub sender_tag: Option<String>,
    /// Unix timestamp when the note was stored.
    pub stored_at: u64,
}

/// In-memory storage for encrypted notes with optional file persistence.
pub struct NoteStore {
    /// Notes indexed by recipient tag.
    notes: RwLock<HashMap<String, VecDeque<StoredEncryptedNote>>>,
    /// Maximum notes per recipient before FIFO eviction.
    max_notes_per_recipient: usize,
    /// Optional path for file persistence.
    persistence_path: Option<String>,
}

/// Serialization format for persistence.
#[derive(Serialize, Deserialize)]
struct PersistedNoteStore {
    notes: HashMap<String, VecDeque<StoredEncryptedNote>>,
}

impl NoteStore {
    /// Creates a new in-memory note store.
    pub fn new(max_notes_per_recipient: Option<usize>) -> Self {
        Self {
            notes: RwLock::new(HashMap::new()),
            max_notes_per_recipient: max_notes_per_recipient
                .unwrap_or(DEFAULT_MAX_NOTES_PER_RECIPIENT),
            persistence_path: None,
        }
    }

    /// Creates a note store with file persistence enabled.
    pub fn with_persistence(
        max_notes_per_recipient: Option<usize>,
        persistence_path: String,
    ) -> io::Result<Self> {
        let mut store = Self {
            notes: RwLock::new(HashMap::new()),
            max_notes_per_recipient: max_notes_per_recipient
                .unwrap_or(DEFAULT_MAX_NOTES_PER_RECIPIENT),
            persistence_path: Some(persistence_path.clone()),
        };

        if Path::new(&persistence_path).exists() {
            store.load_from_file()?;
        }

        Ok(store)
    }

    /// Generates a unique note ID.
    fn generate_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
        format!("{:016x}{:08x}", timestamp, counter as u32)
    }

    /// Returns the current Unix timestamp.
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Inserts a new encrypted note for a recipient.
    ///
    /// Returns the generated note ID and storage timestamp.
    pub fn insert(
        &self,
        recipient_tag: String,
        encrypted_payload: String,
        ephemeral_pubkey: String,
        sender_tag: Option<String>,
    ) -> (String, u64) {
        let id = Self::generate_id();
        let stored_at = Self::current_timestamp();

        let note = StoredEncryptedNote {
            id: id.clone(),
            encrypted_payload,
            ephemeral_pubkey,
            sender_tag,
            stored_at,
        };

        {
            let mut notes = self.notes.write().expect("notes lock poisoned");
            let queue = notes.entry(recipient_tag.clone()).or_default();

            // FIFO eviction if at capacity
            while queue.len() >= self.max_notes_per_recipient {
                if let Some(evicted) = queue.pop_front() {
                    debug!(
                        recipient_tag = %recipient_tag,
                        evicted_id = %evicted.id,
                        "Evicted oldest note due to capacity limit"
                    );
                }
            }

            queue.push_back(note);
        }

        // Persist if enabled
        if let Err(err) = self.maybe_persist() {
            warn!(error = %err, "Failed to persist note store");
        }

        (id, stored_at)
    }

    /// Retrieves notes for a recipient, optionally filtered by timestamp and limited.
    ///
    /// Returns the notes and a boolean indicating if there are more notes.
    pub fn get_notes(
        &self,
        recipient_tag: &str,
        since: Option<u64>,
        limit: Option<usize>,
    ) -> (Vec<StoredEncryptedNote>, bool) {
        let notes = self.notes.read().expect("notes lock poisoned");

        let Some(queue) = notes.get(recipient_tag) else {
            return (Vec::new(), false);
        };

        let filtered: Vec<_> = queue
            .iter()
            .filter(|note| {
                if let Some(since_ts) = since {
                    note.stored_at > since_ts
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        let limit = limit.unwrap_or(100);
        let has_more = filtered.len() > limit;
        let result = filtered.into_iter().take(limit).collect();

        (result, has_more)
    }

    /// Deletes a specific note by recipient tag and note ID.
    ///
    /// Returns true if the note was found and deleted.
    pub fn delete_note(&self, recipient_tag: &str, note_id: &str) -> bool {
        let deleted = {
            let mut notes = self.notes.write().expect("notes lock poisoned");

            let Some(queue) = notes.get_mut(recipient_tag) else {
                return false;
            };

            let initial_len = queue.len();
            queue.retain(|note| note.id != note_id);
            let deleted = queue.len() < initial_len;

            // Clean up empty queues
            if queue.is_empty() {
                notes.remove(recipient_tag);
            }

            deleted
        };

        if deleted {
            if let Err(err) = self.maybe_persist() {
                warn!(error = %err, "Failed to persist note store after deletion");
            }
        }

        deleted
    }

    /// Returns the total number of stored notes across all recipients.
    pub fn total_notes(&self) -> usize {
        let notes = self.notes.read().expect("notes lock poisoned");
        notes.values().map(|q| q.len()).sum()
    }

    /// Returns the number of unique recipient tags.
    pub fn recipient_count(&self) -> usize {
        let notes = self.notes.read().expect("notes lock poisoned");
        notes.len()
    }

    /// Persists the store to disk if persistence is enabled.
    fn maybe_persist(&self) -> io::Result<()> {
        if let Some(ref path) = self.persistence_path {
            self.save_to_file(path)?;
        }
        Ok(())
    }

    /// Saves the current state to a file.
    fn save_to_file(&self, path: &str) -> io::Result<()> {
        let notes = self.notes.read().expect("notes lock poisoned");
        let persisted = PersistedNoteStore {
            notes: notes.clone(),
        };

        let temp_path = format!("{}.tmp", path);
        let file = fs::File::create(&temp_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, &persisted)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        fs::rename(&temp_path, path)?;
        debug!(path = %path, "Persisted note store to disk");
        Ok(())
    }

    /// Loads state from a file.
    fn load_from_file(&mut self) -> io::Result<()> {
        let Some(ref path) = self.persistence_path else {
            return Ok(());
        };

        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let persisted: PersistedNoteStore = serde_json::from_reader(reader)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let mut notes = self.notes.write().expect("notes lock poisoned");
        *notes = persisted.notes;

        let total: usize = notes.values().map(|q| q.len()).sum();
        info!(
            path = %path,
            recipients = notes.len(),
            total_notes = total,
            "Loaded note store from disk"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let store = NoteStore::new(None);

        let (id, stored_at) = store.insert(
            "recipient1".to_string(),
            "payload1".to_string(),
            "ephemeral1".to_string(),
            None,
        );

        assert!(!id.is_empty());
        assert!(stored_at > 0);

        let (notes, has_more) = store.get_notes("recipient1", None, None);
        assert_eq!(notes.len(), 1);
        assert!(!has_more);
        assert_eq!(notes[0].id, id);
        assert_eq!(notes[0].encrypted_payload, "payload1");
    }

    #[test]
    fn test_fifo_eviction() {
        let store = NoteStore::new(Some(3));

        for i in 0..5 {
            store.insert(
                "recipient".to_string(),
                format!("payload{}", i),
                "ephemeral".to_string(),
                None,
            );
        }

        let (notes, _) = store.get_notes("recipient", None, None);
        assert_eq!(notes.len(), 3);

        // Should have the last 3 notes (payload2, payload3, payload4)
        let payloads: Vec<_> = notes.iter().map(|n| n.encrypted_payload.as_str()).collect();
        assert_eq!(payloads, vec!["payload2", "payload3", "payload4"]);
    }

    #[test]
    fn test_delete_note() {
        let store = NoteStore::new(None);

        let (id1, _) = store.insert(
            "recipient".to_string(),
            "payload1".to_string(),
            "ephemeral".to_string(),
            None,
        );
        let (id2, _) = store.insert(
            "recipient".to_string(),
            "payload2".to_string(),
            "ephemeral".to_string(),
            None,
        );

        assert!(store.delete_note("recipient", &id1));
        assert!(!store.delete_note("recipient", &id1)); // Already deleted

        let (notes, _) = store.get_notes("recipient", None, None);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, id2);
    }

    #[test]
    fn test_get_notes_since() {
        let store = NoteStore::new(None);

        let (_, ts1) = store.insert(
            "recipient".to_string(),
            "payload1".to_string(),
            "ephemeral".to_string(),
            None,
        );

        // Insert another note (will have same or later timestamp)
        store.insert(
            "recipient".to_string(),
            "payload2".to_string(),
            "ephemeral".to_string(),
            None,
        );

        // Get notes since ts1 (should only return notes with stored_at > ts1)
        let (notes, _) = store.get_notes("recipient", Some(ts1), None);
        // In practice, both notes might have the same timestamp since they're inserted quickly
        // So we just verify the filter logic works
        assert!(notes.iter().all(|n| n.stored_at > ts1));
    }

    #[test]
    fn test_get_notes_limit() {
        let store = NoteStore::new(None);

        for i in 0..10 {
            store.insert(
                "recipient".to_string(),
                format!("payload{}", i),
                "ephemeral".to_string(),
                None,
            );
        }

        let (notes, has_more) = store.get_notes("recipient", None, Some(5));
        assert_eq!(notes.len(), 5);
        assert!(has_more);

        let (notes, has_more) = store.get_notes("recipient", None, Some(10));
        assert_eq!(notes.len(), 10);
        assert!(!has_more);
    }

    #[test]
    fn test_nonexistent_recipient() {
        let store = NoteStore::new(None);

        let (notes, has_more) = store.get_notes("nonexistent", None, None);
        assert!(notes.is_empty());
        assert!(!has_more);

        assert!(!store.delete_note("nonexistent", "some-id"));
    }
}
