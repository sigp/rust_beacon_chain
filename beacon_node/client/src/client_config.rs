use clap::ArgMatches;
use db::DBType;
use fork_choice::ForkChoiceAlgorithm;
use libp2p::multiaddr::ToMultiaddr;
use network::NetworkConfig;
use slog::error;
use std::fs;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::path::PathBuf;
use types::ChainSpec;

/// Stores the client configuration for this Lighthouse instance.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub data_dir: PathBuf,
    pub spec: ChainSpec,
    pub net_conf: network::NetworkConfig,
    pub fork_choice: ForkChoiceAlgorithm,
    pub db_type: DBType,
    pub db_name: PathBuf,
    //pub rpc_conf:
    //pub ipc_conf:
}

impl Default for ClientConfig {
    /// Build a new lighthouse configuration from defaults.
    fn default() -> Self {
        let data_dir = {
            let home = dirs::home_dir().expect("Unable to determine home dir.");
            home.join(".lighthouse/")
        };
        fs::create_dir_all(&data_dir)
            .unwrap_or_else(|_| panic!("Unable to create {:?}", &data_dir));
        Self {
            data_dir: data_dir.clone(),
            // default to foundation for chain specs
            spec: ChainSpec::foundation(),
            net_conf: NetworkConfig::default(),
            // default to bitwise LMD Ghost
            fork_choice: ForkChoiceAlgorithm::BitwiseLMDGhost,
            // default to memory db for now
            db_type: DBType::Memory,
            // default db name for disk-based dbs
            db_name: data_dir.join("chain.db"),
        }
    }
}

impl ClientConfig {
    /// Parses the CLI arguments into a `Config` struct.
    pub fn parse_args(args: ArgMatches, log: &slog::Logger) -> Result<Self, &'static str> {
        let mut config = ClientConfig::default();

        // Network related args

        // Custom p2p listen port
        if let Some(port_str) = args.value_of("port") {
            if let Ok(port) = port_str.parse::<u16>() {
                config.net_conf.listen_port = port;
            } else {
                error!(log, "Invalid port"; "port" => port_str);
                return Err("Invalid port");
            }
        }
        // Custom listening address ipv4/ipv6
        // TODO: Handle list of addresses
        if let Some(listen_address_str) = args.value_of("listen_address") {
            if let Ok(listen_address) = listen_address_str.parse::<IpAddr>() {
                let multiaddr = SocketAddr::new(listen_address, config.net_conf.listen_port)
                    .to_multiaddr()
                    .expect("Invalid listen address format");
                config.net_conf.listen_addresses = vec![multiaddr];
            } else {
                error!(log, "Invalid IP Address"; "Address" => listen_address_str);
                return Err("Invalid IP Address");
            }
        }

        // filesystem args

        // Custom datadir
        if let Some(dir) = args.value_of("datadir") {
            config.data_dir = PathBuf::from(dir.to_string());
        };

        Ok(config)
    }
}
