//! Braidpool-specific types bridging the SV2 pool to the braidpool node.
//!
//! This module is the boundary layer between the generic SV2 pool and
//! Braidpool's bead-based consensus. It defines the types that flow from
//! the pool (where shares are validated) to the node (where beads are built).
//!
//! # Data flow
//! ```text
//! SubmitSharesExtended
//!   → validate_share (ExtendedChannel)
//!   → ValidatedShare  ──[ShareBridgeSender]──► node::propagate_valid_bead  (PR 4)
//! ```

use tokio::sync::mpsc;

/// A share that passed pool-level PoW validation, ready for bead construction.
///
/// `extranonce1` is `Vec<u8>` rather than `u64` because in audit mode the
/// pool-assigned prefix is `N + 7` bytes (upstream extranonce1 + 2-byte miner
/// prefix + 5-byte commitment), which can exceed 8 bytes for common 8-byte
/// upstream configurations.
#[derive(Debug, Clone)]
pub struct ValidatedShare {
    /// Template the share was mined against. `None` for custom-job shares.
    pub template_id: Option<u64>,
    /// SV2 extended channel ID the share arrived on.
    pub channel_id: u32,
    /// Pool-assigned extranonce prefix (extranonce1 in Stratum v1 terms).
    pub extranonce1: Vec<u8>,
    /// Miner-rolled extranonce suffix (extranonce2).
    pub extranonce2: Vec<u8>,
    /// Block version field (may include BIP-320 rolled bits).
    pub version: u32,
    /// Block header nTime.
    pub ntime: u32,
    /// Block header nonce.
    pub nonce: u32,
}

/// Template context attached to a validated share for bead metadata construction.
///
/// Carries the information needed to build `CommittedMetadata` in the node.
/// Populated when a `ValidatedShare` is matched to a template.
#[derive(Debug)]
pub struct BeadContext {
    /// Template that was being mined when the share arrived.
    pub template_id: u64,
    /// Wall-clock time the template was received from the template provider.
    /// Used to measure share propagation latency for bead timing.
    pub received_at: std::time::Instant,
    /// Committed transactions from the template (for CommittedMetadata).
    pub transactions: Vec<Vec<u8>>,
}

/// Extranonce allocation strategy for the pool↔tproxy Extended Channel.
///
/// Determines the split between pool-assigned prefix (extranonce1) and
/// miner-rolled space (extranonce2) based on the upstream pool configuration.
///
/// # Normal mode (4+8)
/// ```text
/// upstream negotiates: ext1=4, ext2=8
/// pool sends to tproxy: prefix=4 bytes (random), extranonce_size=12
/// ```
///
/// # Audit mode
/// Braidpool embeds commitment bytes inside the extranonce space it sends to
/// downstream miners, then reconstructs the upstream-agreed format before
/// submitting shares back upstream.
///
/// ```text
/// upstream negotiates: ext1=N, ext2=8  (common: N=4 or N=8)
/// pool prefix sent to tproxy: N + 7 bytes  (N upstream + 2 miner prefix + 5 commitment)
/// miner rolling space: total - (N+7) bytes
/// on share submission upstream: reconstruct ext1=N, ext2=8
/// ```
#[derive(Debug, Clone)]
pub enum ExtraNonceConfig {
    /// Standard mode: 4-byte pool prefix, up to 12-byte miner rolling space.
    Normal,
    /// Audit mode: prefix embeds upstream extranonce1 + commitment bytes.
    Audit {
        /// Size (bytes) of extranonce1 as negotiated with the upstream pool.
        upstream_ext1_size: usize,
    },
}

impl ExtraNonceConfig {
    /// Pool-assigned prefix size (extranonce1) for this configuration.
    pub fn extranonce1_size(&self) -> usize {
        match self {
            Self::Normal => 4,
            // N bytes upstream + 2 miner prefix + 5 commitment
            Self::Audit { upstream_ext1_size } => upstream_ext1_size + 7,
        }
    }

    /// Miner-controlled rolling space size (extranonce2) for this configuration,
    /// given `total_extranonce_bytes` agreed with the upstream pool.
    pub fn extranonce2_size(&self, total_extranonce_bytes: usize) -> usize {
        total_extranonce_bytes.saturating_sub(self.extranonce1_size())
    }
}

/// Sender half of the share bridge — held by the pool's `ChannelManager`.
pub type ShareBridgeSender = mpsc::UnboundedSender<ValidatedShare>;

/// Receiver half of the share bridge — consumed by the node IPC task (PR 4).
pub type ShareBridgeReceiver = mpsc::UnboundedReceiver<ValidatedShare>;

/// Create the share bridge channel connecting pool validation to node bead construction.
///
/// The sender is stored on `ChannelManager`; the receiver is kept alive in
/// `PoolRuntime` until PR 4 wires it to `propagate_valid_bead` over IPC.
pub fn create_share_bridge() -> (ShareBridgeSender, ShareBridgeReceiver) {
    mpsc::unbounded_channel()
}
