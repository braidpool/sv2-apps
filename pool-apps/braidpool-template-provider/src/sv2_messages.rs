//! SV2 message builders for TemplateDistribution protocol.
//!
//! Builds NewTemplate and SetNewPrevHash from BraidpoolTemplate.

use crate::error::TemplateDataError;
use bitcoin::{consensus::deserialize, Transaction};
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
    let coinbase_tx: Transaction = deserialize(&template.coinbase_tx).map_err(|e| {
        TemplateDataError::InvalidCoinbaseTx(format!("Failed to deserialize coinbase_tx: {e}"))
    })?;

    if coinbase_tx.input.is_empty() {
        return Err(TemplateDataError::InvalidCoinbaseTx(
            "coinbase transaction has no inputs".into(),
        ));
    }

    let version: u32 = template
        .version
        .try_into()
        .map_err(|_| TemplateDataError::InvalidBlockVersion)?;

    let coinbase_tx_version: u32 = coinbase_tx
        .version
        .0
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
    let coinbase_tx_value_remaining: u64 =
        coinbase_tx.output.iter().map(|o| o.value.to_sat()).sum();

    // braidpool_mode=true: include ALL outputs (reward[0] + segwit[1] + OP_RETURN[2])
    // Standard SRI mode would only include output[0]
    let outputs_to_include = coinbase_tx.output.to_vec();

    let mut serialized_outputs = Vec::new();
    for output in &outputs_to_include {
        serialized_outputs.extend_from_slice(&bitcoin::consensus::serialize(output));
    }

    let coinbase_tx_outputs: B064KOwned = serialized_outputs.try_into().map_err(|_| {
        TemplateDataError::SerializationError("coinbase outputs too large for B064K".into())
    })?;

    let coinbase_tx_locktime = coinbase_tx.lock_time.to_consensus_u32();

    // Build merkle path: each [u8; 32] branch → U256Owned
    let merkle_path_vec: Vec<U256Owned> = template
        .merkle_path
        .iter()
        .map(|branch| U256Owned::from(*branch))
        .collect();

    let merkle_path: Seq0255Owned<U256Owned> =
        Seq0255Owned::new(merkle_path_vec).map_err(|_| {
            TemplateDataError::MerklePathError("merkle path exceeds 255 elements".into())
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{
        consensus::serialize, locktime::absolute::LockTime, OutPoint, ScriptBuf, Sequence,
        Transaction, TxIn, TxOut, Witness,
    };
    use braidpool_common::template::BraidpoolTemplate;

    fn make_template(coinbase_tx: Transaction) -> BraidpoolTemplate {
        BraidpoolTemplate {
            coinbase_tx: serialize(&coinbase_tx),
            merkle_path: vec![[2u8; 32]],
            prev_hash: [1u8; 32],
            nbits: 0x1d00ffff,
            header_timestamp: 1_700_000_000,
            version: 2,
            height: 840_000,
            template_id: 99,
        }
    }

    fn coinbase_tx(outputs: Vec<TxOut>) -> Transaction {
        Transaction {
            version: bitcoin::transaction::Version::TWO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::from_bytes(vec![0x03, 0x01, 0x00, 0x00]),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: outputs,
            lock_time: LockTime::ZERO,
        }
    }

    fn dummy_output(sats: u64) -> TxOut {
        TxOut {
            value: bitcoin::Amount::from_sat(sats),
            script_pubkey: ScriptBuf::new(),
        }
    }

    #[test]
    fn test_build_new_template_maps_fields() {
        let tx = coinbase_tx(vec![dummy_output(5_000_000_000), dummy_output(0)]);
        let template = make_template(tx.clone());

        let result = build_new_template(&template, true).unwrap();

        assert_eq!(result.template_id, 99);
        assert!(result.future_template);
        assert_eq!(result.version, 2u32);
        assert_eq!(result.coinbase_tx_value_remaining, 5_000_000_000);
        assert_eq!(result.coinbase_tx_outputs_count, 2);
        assert_eq!(result.coinbase_tx_locktime, 0);
        assert_eq!(result.merkle_path.len(), 1);
    }

    #[test]
    fn test_build_new_template_empty_input_returns_error() {
        let tx = Transaction {
            version: bitcoin::transaction::Version::TWO,
            input: vec![],
            output: vec![dummy_output(0)],
            lock_time: LockTime::ZERO,
        };
        let template = make_template(tx);

        let err = build_new_template(&template, false).unwrap_err();
        assert!(matches!(err, TemplateDataError::InvalidCoinbaseTx(_)));
    }

    #[test]
    fn test_build_new_template_invalid_bytes_returns_error() {
        let template = BraidpoolTemplate {
            coinbase_tx: vec![0xde, 0xad, 0xbe, 0xef],
            merkle_path: vec![],
            prev_hash: [0u8; 32],
            nbits: 0,
            header_timestamp: 0,
            version: 2,
            height: 0,
            template_id: 1,
        };

        let err = build_new_template(&template, false).unwrap_err();
        assert!(matches!(err, TemplateDataError::InvalidCoinbaseTx(_)));
    }

    #[test]
    fn test_build_set_new_prev_hash_maps_fields() {
        let tx = coinbase_tx(vec![dummy_output(0)]);
        let template = make_template(tx);

        let result = build_set_new_prev_hash(&template).unwrap();

        assert_eq!(result.template_id, 99);
        assert_eq!(result.header_timestamp, 1_700_000_000);
        assert_eq!(result.n_bits, 0x1d00ffff);
        assert_eq!(result.prev_hash.as_ref(), &[1u8; 32]);
    }
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
        header_timestamp: template.header_timestamp,
        n_bits: template.nbits,
        target: U256Owned::from([0u8; 32]), // pool sets target via SetTarget separately
    })
}
