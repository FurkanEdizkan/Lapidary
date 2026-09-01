use crate::DbError;
use lapidary_core::{LibraryId, PartSummary};

/// Reading parts for the grid. The open path reads metadata and derivatives only and
/// never touches a source file.
#[async_trait::async_trait]
pub trait PartRepository: Send + Sync {
    /// One keyset page of grid rows, newest first. `after` is the previous page's last
    /// id. Implementation lands in Phase 1.
    async fn page(
        &self,
        library: LibraryId,
        after: Option<lapidary_core::PartId>,
        limit: u16,
    ) -> Result<Vec<PartSummary>, DbError>;
}
