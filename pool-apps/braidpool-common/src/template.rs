/// A block template produced by the Braidpool node, consumed by
/// braidpool-template-provider in sv2-apps.
///
/// The node converts its internal BlockTemplate into this type before
/// sending it over the channel to the SV2 pool. This avoids a direct
/// dependency on node internals from sv2-apps.
#[derive(Debug, Clone)]
pub struct BraidpoolTemplate {
    /// Full serialized coinbase transaction built by template_creator.rs.
    /// Contains the Braidpool OP_RETURN commitment already embedded.
    /// Used by build_new_template in braidpool_mode=true.
    pub coinbase_tx: Vec<u8>,

    /// Merkle path from coinbase txid to merkle root.
    pub merkle_path: Vec<[u8; 32]>,

    /// Previous block hash (32 bytes, little-endian).
    pub prev_hash: [u8; 32],

    /// Compact difficulty target (nbits).
    pub nbits: u32,

    /// Bitcoin block version.
    pub version: i32,

    /// Block height — required for Future Job (NewExtendedMiningJob).
    /// Must come from ipc_template.components, NOT BlockTemplate.height
    /// which is Height::ZERO in ipc_template_consumer.
    pub height: u32,

    /// Monotonically increasing template identifier from the node.
    pub template_id: u64,
}
