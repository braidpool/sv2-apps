#[derive(Debug)]
pub enum BraidpoolTemplateProviderError {
    InvalidTemplateData(String),
    InvalidCoinbaseTx(String),
    InvalidBlockVersion,
    InvalidCoinbaseTxVersion,
    InvalidCoinbaseScriptSig,
    CoinbaseOutputSerializationFailed,
    BlockDeserializationFailed(String),
    TemplateNotFound(u64),
    ChannelSendError(String),
    ChannelRecvError(String),
    MerklePathError(String),
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
