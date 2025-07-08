pub mod generic;

mod qdrant;

#[cfg(feature = "qdrant-backend")]
pub use qdrant::QdrantBackend as Qdrant;
