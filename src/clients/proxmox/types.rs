use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ProxmoxEnvelope<T> {
    pub(crate) data: T,
}
