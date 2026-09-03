//! SV2 message builders for TemplateDistribution protocol.
//!
//! Builds NewTemplate and SetNewPrevHash from BraidpoolTemplate.
//! Key difference from Sansh's version: uses BraidpoolTemplate.coinbase_tx
//! (already built by template_creator with OP_RETURN commitment) instead
//! of deserializing from processed_block_hex.

use crate::error::TemplateDataError;
use bitcoin::{Transaction, consensus::deserialize};
use braidpool_common::template::BraidpoolTemplate;
use stratum_apps::stratum_core::{
    binary_sv2::{B0255Owned, B064KOwned, Seq0255Owned, U256Owned},
    template_distribution_sv2::{NewTemplateOwned, SetNewPrevHashOwned},
};
use tracing::debug;

/// Build a NewTemplate SV2 message from a BraidpoolTemplate.
///
/// braidpool_mode=true: includes ALL coinbase outputs (reward + segwit + OP_RETURN).
/// This is the Braidpool-specific behavior — factory.rs uses these outputs
/// directly to construct the full coinbase for downstream miners.
///
/// # Parameters
/// * `template` - The BraidpoolTemplate from the node
/// * `future_template` - true = send job without prev_hash (Future Job)
pub fn build_new_template(
    template: &BraidpoolTemplate,
    future_template: bool,
) -> Result<NewTemplateOwned, TemplateDataError> {
    // Deserialize the full coinbase built by template_creator.rs.
    // This coinbase already contains the Braidpool OP_RETURN commitment.
    let coinbase_tx: Transaction = deserialize(&template.coinbase_tx)
        .map_err(|e| TemplateDataError::InvalidCoinbaseTx(format!(
            "Failed to deserialize coinbase_tx: {}", e
        )))?;

    let version: u32 = template.version
        .try_into()
        .map_err(|_| TemplateDataError::InvalidBlockVersion)?;

    let coinbase_tx_version: u32 = coinbase_tx.version.0
        .try_into()
        .map_err(|_| TemplateDataError::InvalidCoinbaseTxVersion)?;

    // coinbase scriptSig becomes the coinbase_prefix in SV2
    let coinbase_prefix: B0255Owned = coinbase_tx.input[0]
        .script_sig
        .to_bytes()
        .try_into()
        .map_err(|_| TemplateDataError::InvalidCoinbaseScriptSig)?;

    let coinbase_tx_input_sequence = coinbase_tx.input[0].sequence.to_consensus_u32();

    // Total value across all outputs
    let coinbase_tx_value_remaining: u64 = coinbase_tx
        .output
        .iter()
        .map(|o| o.value.to_sat())
        .sum();

    // braidpool_mode=true: include ALL outputs (reward[0] + segwit[1] + OP_RETURN[2])
    // Standard SRI mode would only include output[0]
    let outputs_to_include: Vec<_> = coinbase_tx.output.iter().cloned().collect();

    let mut serialized_outputs = Vec::new();
    for output in &outputs_to_include {
        serialized_outputs.extend_from_slice(
            &bitcoin::consensus::serialize(output)
        );
    }

    let coinbase_tx_outputs: B064KOwned = serialized_outputs
        .try_into()
        .map_err(|_| TemplateDataError::SerializationError(
            "coinbase outputs too large for B064K".into()
        ))?;

    let coinbase_tx_locktime = coinbase_tx.lock_time.to_consensus_u32();

    // Build merkle path: each [u8; 32] branch → U256Owned
    let merkle_path_vec: Vec<U256Owned> = template.merkle_path.iter()
        .map(|branch| U256Owned::from(*branch))
        .collect();

    let merkle_path: Seq0255Owned<U256Owned> = Seq0255Owned::new(merkle_path_vec)
        .map_err(|_| TemplateDataError::MerklePathError(
            "merkle path exceeds 255 elements".into()
        ))?;

    debug!(
        template_id = template.template_id,
        future_template,
        outputs = outputs_to_include.len(),
        "Building NewTemplate (braidpool_mode=true)"
    );

    Ok(NewTemplateOwned {
        template_id: template.template_id,
        future_template,
        version,
        coinbase_tx_version,
        coinbase_prefix,
        coinbase_tx_input_sequence,
        coinbase_tx_value_remaining,
        coinbase_tx_outputs_count: outputs_to_include.len() as u32,
        coinbase_tx_outputs,
        coinbase_tx_locktime,
        merkle_path,
    })
}

/// Build a SetNewPrevHash SV2 message from a BraidpoolTemplate.
/// Activates the Future Job sent by build_new_template.
pub fn build_set_new_prev_hash(
    template: &BraidpoolTemplate,
) -> Result<SetNewPrevHashOwned, TemplateDataError> {
    let prev_hash = U256Owned::from(template.prev_hash);

    Ok(SetNewPrevHashOwned {
        template_id: template.template_id,
        prev_hash,
        header_timestamp: 0, // miner uses current time
        n_bits: template.nbits,
        target: U256Owned::from([0u8; 32]), // pool sets target via SetTarget separately
    })
}
