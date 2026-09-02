//! Errors raised while building or configuring this crate's router, as opposed to errors
//! raised by a handler at request time (those stay local to their handler module, e.g.
//! `health::healthz` builds its own JSON body).

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(
        "`{got}` is not a role. Set LAPIDARY_ROLE to `api` (serves the grid and the open \
         path) or `worker` (runs ingest). deploy/compose.yaml sets it per service."
    )]
    UnknownRole { got: String },
}
