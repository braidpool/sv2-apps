//! Braidpool Template Provider
//!
//! Converts BraidpoolTemplate (from braidpool-common) to SV2
//! TemplateDistribution messages for the pool downstream channel manager.


use async_channel::{Receiver, Sender};
use braidpool_common::template::BraidpoolTemplate;
use stratum_apps::stratum_core::{
    parsers_sv2::TemplateDistributionOwned,
    template_distribution_sv2::RequestTransactionDataErrorOwned,
};
pub use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

pub mod error;
pub mod sv2_messages;

pub use error::BraidpoolTemplateProviderError;

/// Consumes BraidpoolTemplates and converts to SV2 TemplateDistribution messages.
///
/// Runs as a sibling task to ipc_template_consumer in the braidpool node.
/// Receives templates via a dedicated channel rather than sharing the node's
/// notification_tx (which is single-consumer).
///
/// # Parameters
/// * `template_rx` - Receiver for BraidpoolTemplates from the node
/// * `sv2_outgoing_tx` - Sender for outgoing SV2 TemplateDistribution messages to pool
/// * `sv2_incoming_rx` - Receiver for incoming SV2 messages (SubmitSolution etc.)
/// * `cancellation_token` - Token for graceful shutdown
pub async fn sv2_template_consumer(
    mut template_rx: tokio::sync::mpsc::Receiver<BraidpoolTemplate>,
    sv2_outgoing_tx: Sender<TemplateDistributionOwned>,
    sv2_incoming_rx: Receiver<TemplateDistributionOwned>,
    cancellation_token: CancellationToken,
) -> Result<(), BraidpoolTemplateProviderError> {
    info!("SV2 template consumer started");

    let mut _current_template_id: Option<u64> = None;

    // Wait for CoinbaseOutputConstraints from pool before processing templates.
    // The pool sends this on startup to tell us the max coinbase output size.
    info!("Waiting for CoinbaseOutputConstraints from pool");
    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                warn!("Cancelled before receiving CoinbaseOutputConstraints");
                return Ok(());
            }
            Ok(message) = sv2_incoming_rx.recv() => {
                if let TemplateDistributionOwned::CoinbaseOutputConstraints(constraints) = message {
                    info!(
                        max_size = constraints.coinbase_output_max_additional_size,
                        "Received CoinbaseOutputConstraints from pool"
                    );
                    break;
                }
            }
        }
    }

    // Main loop — receive BraidpoolTemplates and send SV2 messages
    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                info!("SV2 template consumer shutting down");
                return Ok(());
            }

            Some(template) = template_rx.recv() => {
                let template_id = template.template_id;
                debug!(template_id, "Received BraidpoolTemplate");

                // Send future job first (no prev_hash yet — miners prepare work)
                match sv2_messages::build_new_template(&template, true) {
                    Ok(new_template) => {
                        if let Err(e) = sv2_outgoing_tx
                            .send(TemplateDistributionOwned::NewTemplate(new_template))
                            .await
                        {
                            error!(error = %e, "Failed to send NewTemplate");
                            return Err(BraidpoolTemplateProviderError::ChannelSendError(
                                e.to_string(),
                            ));
                        }
                        info!(template_id, "Sent NewTemplate (future job)");
                    }
                    Err(e) => {
                        error!(template_id, error = %e, "Failed to build NewTemplate");
                        continue;
                    }
                }

                // Send SetNewPrevHash to activate the job
                match sv2_messages::build_set_new_prev_hash(&template) {
                    Ok(set_new_prev_hash) => {
                        if let Err(e) = sv2_outgoing_tx
                            .send(TemplateDistributionOwned::SetNewPrevHash(set_new_prev_hash))
                            .await
                        {
                            error!(error = %e, "Failed to send SetNewPrevHash");
                            return Err(BraidpoolTemplateProviderError::ChannelSendError(
                                e.to_string(),
                            ));
                        }
                        info!(template_id, "Sent SetNewPrevHash");
                    }
                    Err(e) => {
                        error!(template_id, error = %e, "Failed to build SetNewPrevHash");
                        continue;
                    }
                }

                _current_template_id = Some(template_id);
            }

            Ok(message) = sv2_incoming_rx.recv() => {
                match message {
                    TemplateDistributionOwned::SubmitSolution(_solution) => {
                        debug!("Received SubmitSolution — forwarding to node via bridge");
                        // TODO(sv2-integration): forward to node's block submission channel
                        // This will be wired in PR 4 when the node bridge is added
                    }
                    TemplateDistributionOwned::RequestTransactionData(req) => {
                        debug!(template_id = req.template_id, "Received RequestTransactionData");
                        // Braidpool does not use TDP transaction data requests
                        // Send error response per SV2 spec
                        let err = RequestTransactionDataErrorOwned {
                            template_id: req.template_id,
                            error_code: b"template-id-not-found"
                                .to_vec()
                                .try_into()
                                .expect("static ASCII fits Str0255"),
                        };
                        let _ = sv2_outgoing_tx
                            .send(TemplateDistributionOwned::RequestTransactionDataError(err))
                            .await;
                    }
                    _ => {
                        debug!("Ignoring unhandled incoming SV2 message");
                    }
                }
            }
        }
    }
}
