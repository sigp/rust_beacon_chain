/*
 * Minimal Beacon Node API for Validator
 *
 * A minimal API specification for the beacon node, which enables a validator to connect and perform its obligations on the Ethereum 2.0 phase 0 beacon chain.
 *
 * The version of the OpenAPI document: 0.2.0
 * 
 * Generated by: https://openapi-generator.tech
 */

/// VoluntaryExit : The [`VoluntaryExit`](https://github.com/ethereum/eth2.0-specs/blob/master/specs/core/0_beacon-chain.md#voluntaryexit) object from the Eth2.0 spec.

#[allow(unused_imports)]
use serde_json::Value;


#[derive(Debug, Serialize, Deserialize)]
pub struct VoluntaryExit {
    /// Minimum epoch for processing exit.
    #[serde(rename = "epoch", skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i32>,
    /// Index of the exiting validator.
    #[serde(rename = "validator_index", skip_serializing_if = "Option::is_none")]
    pub validator_index: Option<i32>,
    /// Validator signature.
    #[serde(rename = "signature", skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl VoluntaryExit {
    /// The [`VoluntaryExit`](https://github.com/ethereum/eth2.0-specs/blob/master/specs/core/0_beacon-chain.md#voluntaryexit) object from the Eth2.0 spec.
    pub fn new() -> VoluntaryExit {
        VoluntaryExit {
            epoch: None,
            validator_index: None,
            signature: None,
        }
    }
}


