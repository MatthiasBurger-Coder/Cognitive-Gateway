use super::*;
impl SessionConnectorComponents {
    /// Consume the inseparable host/accounting proof. The factory callback sees
    /// no independently replaceable host or quote parts.
    pub(crate) fn from_qualified(
        bundle: crate::mcp_client::scalar_quotes::QualifiedScalarHost,
        authority: Arc<dyn SessionResourceAuthorityPort>,
    ) -> Self {
        let (host, quotes, _pure_quote) = bundle.into_parts();
        Self {
            pure_quote: None,
            full_join: None,
            host,
            authority,
            quotes,
        }
    }
}
