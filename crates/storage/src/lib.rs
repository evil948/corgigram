mod error;
mod store;

pub use error::StorageError;
pub use store::{ChatPreview, ContactRecord, MessageRecord, OutboxRecord, Storage};
