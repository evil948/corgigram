mod error;
mod store;

pub use error::StorageError;
pub use store::{ContactRecord, MessageRecord, OutboxRecord, Storage};
