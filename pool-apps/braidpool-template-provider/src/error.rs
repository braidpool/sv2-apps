/// Errors from the SV2 template consumer task.
///
/// Message-build failures use [`TemplateDataError`]; this type covers the
/// consumer's channel lifecycle. Variants for the block-submission path
/// (SubmitSolution → node) will be added when that leg is wired.
#[derive(Debug)]
pub enum BraidpoolTemplateProviderError {
    ChannelSendError(String),
    ChannelRecvError(String),
}

/// Errors specific to building SV2 template messages
#[derive(Debug)]
pub enum TemplateDataError {
    InvalidBlockVersion,
    InvalidCoinbaseTxVersion,
    InvalidCoinbaseScriptSig,
    InvalidCoinbaseTx(String),
    MerklePathError(String),
    SerializationError(String),
}

impl std::fmt::Display for BraidpoolTemplateProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::fmt::Display for TemplateDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for BraidpoolTemplateProviderError {}
impl std::error::Error for TemplateDataError {}
