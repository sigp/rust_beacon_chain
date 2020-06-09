use crate::discovery::{enr::Eth2Enr, Discovery};
use crate::peer_manager::{PeerManager, PeerManagerEvent};
use crate::rpc::*;
use crate::types::{GossipEncoding, GossipKind, GossipTopic};
use crate::{error, Enr, NetworkConfig, NetworkGlobals, PubsubMessage, TopicHash};
use discv5::Discv5Event;
use futures::prelude::*;
use handler::{BehaviourHandler, BehaviourHandlerIn, BehaviourHandlerOut, DelegateIn, DelegateOut};
use libp2p::{
    core::{
        connection::{ConnectedPoint, ConnectionId, ListenerId},
        identity::Keypair,
        Multiaddr,
    },
    gossipsub::{Gossipsub, GossipsubEvent, MessageId},
    identify::{Identify, IdentifyEvent},
    swarm::{
        NetworkBehaviour, NetworkBehaviourAction as NBAction, PollParameters, ProtocolsHandler,
    },
    PeerId,
};
use lru::LruCache;
use slog::{crit, debug, o};
use std::{
    marker::PhantomData,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};
use types::{EnrForkId, EthSpec, SignedBeaconBlock, SubnetId};

mod handler;

const MAX_IDENTIFY_ADDRESSES: usize = 10;

/// Builds the network behaviour that manages the core protocols of eth2.
/// This core behaviour is managed by `Behaviour` which adds peer management to all core
/// behaviours.
pub struct Behaviour<TSpec: EthSpec> {
    /// The routing pub-sub mechanism for eth2.
    gossipsub: Gossipsub,
    /// The Eth2 RPC specified in the wire-0 protocol.
    eth2_rpc: RPC<TSpec>,
    /// Keep regular connection to peers and disconnect if absent.
    // TODO: Using id for initial interop. This will be removed by mainnet.
    /// Provides IP addresses and peer information.
    identify: Identify,
    /// Discovery behaviour.
    discovery: Discovery<TSpec>,
    /// The peer manager that keeps track of peer's reputation and status.
    peer_manager: PeerManager<TSpec>,
    /// The events generated by this behaviour to be consumed in the swarm poll.
    events: Vec<BehaviourEvent<TSpec>>,
    // TODO: add events to send to the handler
    /// The current meta data of the node, so respond to pings and get metadata
    meta_data: MetaData<TSpec>,
    /// A cache of recently seen gossip messages. This is used to filter out any possible
    /// duplicates that may still be seen over gossipsub.
    // TODO: Remove this
    seen_gossip_messages: LruCache<MessageId, ()>,
    /// A collections of variables accessible outside the network service.
    network_globals: Arc<NetworkGlobals<TSpec>>,
    /// Keeps track of the current EnrForkId for upgrading gossipsub topics.
    // NOTE: This can be accessed via the network_globals ENR. However we keep it here for quick
    // lookups for every gossipsub message send.
    enr_fork_id: EnrForkId,
    /// Logger for behaviour actions.
    log: slog::Logger,
}

/// Calls the given function with the given args on all sub behaviours.
macro_rules! delegate_to_behaviours {
    ($self: ident, $fn: ident, $($arg: ident), *) => {
        $self.gossipsub.$fn($($arg),*);
        $self.eth2_rpc.$fn($($arg),*);
        $self.identify.$fn($($arg),*);
        $self.discovery.$fn($($arg),*);
    };
}

impl<TSpec: EthSpec> NetworkBehaviour for Behaviour<TSpec> {
    type ProtocolsHandler = BehaviourHandler<TSpec>;
    type OutEvent = BehaviourEvent<TSpec>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        BehaviourHandler::new(
            &mut self.gossipsub,
            &mut self.eth2_rpc,
            &mut self.identify,
            &mut self.discovery,
        )
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        let mut out = Vec::new();
        out.extend(self.gossipsub.addresses_of_peer(peer_id));
        out.extend(self.eth2_rpc.addresses_of_peer(peer_id));
        out.extend(self.identify.addresses_of_peer(peer_id));
        out.extend(self.discovery.addresses_of_peer(peer_id));
        out
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        delegate_to_behaviours!(self, inject_connected, peer_id);
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        delegate_to_behaviours!(self, inject_disconnected, peer_id);
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        conn_id: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        delegate_to_behaviours!(
            self,
            inject_connection_established,
            peer_id,
            conn_id,
            endpoint
        );
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        conn_id: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        delegate_to_behaviours!(self, inject_connection_closed, peer_id, conn_id, endpoint);
    }

    fn inject_addr_reach_failure(
        &mut self,
        peer_id: Option<&PeerId>,
        addr: &Multiaddr,
        error: &dyn std::error::Error,
    ) {
        delegate_to_behaviours!(self, inject_addr_reach_failure, peer_id, addr, error);
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        delegate_to_behaviours!(self, inject_dial_failure, peer_id);
    }

    fn inject_new_listen_addr(&mut self, addr: &Multiaddr) {
        delegate_to_behaviours!(self, inject_new_listen_addr, addr);
    }

    fn inject_expired_listen_addr(&mut self, addr: &Multiaddr) {
        delegate_to_behaviours!(self, inject_expired_listen_addr, addr);
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        delegate_to_behaviours!(self, inject_new_external_addr, addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        delegate_to_behaviours!(self, inject_listener_error, id, err);
    }
    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        delegate_to_behaviours!(self, inject_listener_closed, id, reason);
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        conn_id: ConnectionId,
        event: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        match event {
            // Events comming from the handler, redirected to each behaviour
            BehaviourHandlerOut::Delegate(delegate) => match delegate {
                DelegateOut::Gossipsub(ev) => self.gossipsub.inject_event(peer_id, conn_id, ev),
                DelegateOut::RPC(ev) => self.eth2_rpc.inject_event(peer_id, conn_id, ev),
                DelegateOut::Identify(ev) => self.identify.inject_event(peer_id, conn_id, ev),
                DelegateOut::Discovery(ev) => self.discovery.inject_event(peer_id, conn_id, ev),
            },
            /* Custom events sent BY the handler */
            BehaviourHandlerOut::Custom => {
                // TODO: implement
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        poll_params: &mut impl PollParameters,
    ) -> Poll<NBAction<<Self::ProtocolsHandler as ProtocolsHandler>::InEvent, Self::OutEvent>> {
        // TODO: move where it's less distracting
        macro_rules! poll_behaviour {
            /* $behaviour:  The sub-behaviour being polled.
             * $on_event_fn:  Function to call if we get an event from the sub-behaviour.
             * $notify_handler_event_closure:  Closure mapping the received event type to
             *     the one that the handler should get.
             */
            ($behaviour: ident, $on_event_fn: ident, $notify_handler_event_closure: expr) => {
                loop {
                    // poll the sub-behaviour
                    match self.$behaviour.poll(cx, poll_params) {
                        Poll::Ready(action) => match action {
                            // call the designated function to handle the event from sub-behaviour
                            NBAction::GenerateEvent(event) => self.$on_event_fn(event),
                            NBAction::DialAddress { address } => {
                                return Poll::Ready(NBAction::DialAddress { address })
                            }
                            NBAction::DialPeer { peer_id, condition } => {
                                return Poll::Ready(NBAction::DialPeer { peer_id, condition })
                            }
                            NBAction::NotifyHandler {
                                peer_id,
                                handler,
                                event,
                            } => {
                                return Poll::Ready(NBAction::NotifyHandler {
                                    peer_id,
                                    handler,
                                    // call the closure mapping the received event to the needed one
                                    // in order to notify the handler
                                    event: BehaviourHandlerIn::Delegate(
                                        $notify_handler_event_closure(event),
                                    ),
                                });
                            }
                            NBAction::ReportObservedAddr { address } => {
                                return Poll::Ready(NBAction::ReportObservedAddr { address })
                            }
                        },
                        Poll::Pending => break,
                    }
                }
            };
        }

        poll_behaviour!(gossipsub, on_gossip_event, DelegateIn::Gossipsub);
        poll_behaviour!(eth2_rpc, on_rpc_event, DelegateIn::RPC);
        poll_behaviour!(identify, on_identify_event, DelegateIn::Identify);
        poll_behaviour!(discovery, on_discovery_event, DelegateIn::Discovery);

        self.custom_poll(cx)
    }
}

/// Implements the combined behaviour for the libp2p service.
impl<TSpec: EthSpec> Behaviour<TSpec> {
    pub fn new(
        local_key: &Keypair,
        net_conf: &NetworkConfig,
        network_globals: Arc<NetworkGlobals<TSpec>>,
        log: &slog::Logger,
    ) -> error::Result<Self> {
        let local_peer_id = local_key.public().into_peer_id();
        let behaviour_log = log.new(o!());

        let identify = Identify::new(
            "lighthouse/libp2p".into(),
            version::version(),
            local_key.public(),
        );

        let enr_fork_id = network_globals
            .local_enr
            .read()
            .eth2()
            .expect("Local ENR must have a fork id");

        let attnets = network_globals
            .local_enr
            .read()
            .bitfield::<TSpec>()
            .expect("Local ENR must have subnet bitfield");

        let meta_data = MetaData {
            seq_number: 1,
            attnets,
        };

        Ok(Behaviour {
            eth2_rpc: RPC::new(log.clone()),
            gossipsub: Gossipsub::new(local_peer_id, net_conf.gs_config.clone()),
            discovery: Discovery::new(local_key, net_conf, network_globals.clone(), log)?,
            identify,
            peer_manager: PeerManager::new(network_globals.clone(), log),
            events: Vec::new(),
            seen_gossip_messages: LruCache::new(100_000),
            meta_data,
            network_globals,
            enr_fork_id,
            log: behaviour_log,
        })
    }

    /// Obtain a reference to the discovery protocol.
    pub fn discovery(&self) -> &Discovery<TSpec> {
        &self.discovery
    }

    /// Obtain a reference to the gossipsub protocol.
    pub fn gs(&self) -> &Gossipsub {
        &self.gossipsub
    }

    /* Pubsub behaviour functions */

    /// Subscribes to a gossipsub topic kind, letting the network service determine the
    /// encoding and fork version.
    pub fn subscribe_kind(&mut self, kind: GossipKind) -> bool {
        let gossip_topic = GossipTopic::new(
            kind,
            GossipEncoding::default(),
            self.enr_fork_id.fork_digest,
        );
        self.subscribe(gossip_topic)
    }

    /// Unsubscribes from a gossipsub topic kind, letting the network service determine the
    /// encoding and fork version.
    pub fn unsubscribe_kind(&mut self, kind: GossipKind) -> bool {
        let gossip_topic = GossipTopic::new(
            kind,
            GossipEncoding::default(),
            self.enr_fork_id.fork_digest,
        );
        self.unsubscribe(gossip_topic)
    }

    /// Subscribes to a specific subnet id;
    pub fn subscribe_to_subnet(&mut self, subnet_id: SubnetId) -> bool {
        let topic = GossipTopic::new(
            subnet_id.into(),
            GossipEncoding::default(),
            self.enr_fork_id.fork_digest,
        );
        self.subscribe(topic)
    }

    /// Un-Subscribes from a specific subnet id;
    pub fn unsubscribe_from_subnet(&mut self, subnet_id: SubnetId) -> bool {
        let topic = GossipTopic::new(
            subnet_id.into(),
            GossipEncoding::default(),
            self.enr_fork_id.fork_digest,
        );
        self.unsubscribe(topic)
    }

    /// Subscribes to a gossipsub topic.
    fn subscribe(&mut self, topic: GossipTopic) -> bool {
        // update the network globals
        self.network_globals
            .gossipsub_subscriptions
            .write()
            .insert(topic.clone());

        let topic_str: String = topic.clone().into();
        debug!(self.log, "Subscribed to topic"; "topic" => topic_str);
        self.gossipsub.subscribe(topic.into())
    }

    /// Unsubscribe from a gossipsub topic.
    fn unsubscribe(&mut self, topic: GossipTopic) -> bool {
        // update the network globals
        self.network_globals
            .gossipsub_subscriptions
            .write()
            .remove(&topic);
        // unsubscribe from the topic
        self.gossipsub.unsubscribe(topic.into())
    }

    /// Publishes a list of messages on the pubsub (gossipsub) behaviour, choosing the encoding.
    pub fn publish(&mut self, messages: Vec<PubsubMessage<TSpec>>) {
        for message in messages {
            for topic in message.topics(GossipEncoding::default(), self.enr_fork_id.fork_digest) {
                match message.encode(GossipEncoding::default()) {
                    Ok(message_data) => {
                        self.gossipsub.publish(&topic.into(), message_data);
                    }
                    Err(e) => crit!(self.log, "Could not publish message"; "error" => e),
                }
            }
        }
    }

    /// Forwards a message that is waiting in gossipsub's mcache. Messages are only propagated
    /// once validated by the beacon chain.
    pub fn propagate_message(&mut self, propagation_source: &PeerId, message_id: MessageId) {
        self.gossipsub
            .propagate_message(&message_id, propagation_source);
    }

    /* Eth2 RPC behaviour functions */

    /// Send a request to a peer over RPC.
    pub fn send_request(&mut self, peer_id: PeerId, request_id: RequestId, request: Request) {
        self.eth2_rpc
            .send_request(peer_id, request_id, request.into())
    }

    /// Send a successful response to a peer over RPC.
    pub fn send_successful_response(
        &mut self,
        peer_id: PeerId,
        id: PeerRequestId,
        response: Response<TSpec>,
    ) {
        self.eth2_rpc.send_response(peer_id, id, response.into())
    }

    /// Inform the peer that their request produced an error.
    pub fn _send_error_reponse(
        &mut self,
        peer_id: PeerId,
        id: PeerRequestId,
        error: RPCResponseErrorCode,
        reason: String,
    ) {
        self.eth2_rpc.send_response(
            peer_id,
            id,
            RPCCodedResponse::from_error_code(error, reason),
        )
    }

    /* Discovery / Peer management functions */

    /// Notify discovery that the peer has been banned.
    pub fn peer_banned(&mut self, peer_id: PeerId) {
        self.discovery.peer_banned(peer_id);
    }

    /// Notify discovery that the peer has been unbanned.
    pub fn peer_unbanned(&mut self, peer_id: &PeerId) {
        self.discovery.peer_unbanned(peer_id);
    }

    /// Returns an iterator over all enr entries in the DHT.
    pub fn enr_entries(&mut self) -> impl Iterator<Item = &Enr> {
        self.discovery.enr_entries()
    }

    /// Add an ENR to the routing table of the discovery mechanism.
    pub fn add_enr(&mut self, enr: Enr) {
        self.discovery.add_enr(enr);
    }

    /// Updates a subnet value to the ENR bitfield.
    ///
    /// The `value` is `true` if a subnet is being added and false otherwise.
    pub fn update_enr_subnet(&mut self, subnet_id: SubnetId, value: bool) {
        if let Err(e) = self.discovery.update_enr_bitfield(subnet_id, value) {
            crit!(self.log, "Could not update ENR bitfield"; "error" => e);
        }
        // update the local meta data which informs our peers of the update during PINGS
        self.update_metadata();
    }

    /// Attempts to discover new peers for a given subnet. The `min_ttl` gives the time at which we
    /// would like to retain the peers for.
    pub fn discover_subnet_peers(&mut self, subnet_id: SubnetId, min_ttl: Option<Instant>) {
        self.discovery.discover_subnet_peers(subnet_id, min_ttl)
    }

    /// Updates the local ENR's "eth2" field with the latest EnrForkId.
    pub fn update_fork_version(&mut self, enr_fork_id: EnrForkId) {
        self.discovery.update_eth2_enr(enr_fork_id.clone());

        // unsubscribe from all gossip topics and re-subscribe to their new fork counterparts
        let subscribed_topics = self
            .network_globals
            .gossipsub_subscriptions
            .read()
            .iter()
            .cloned()
            .collect::<Vec<GossipTopic>>();

        //  unsubscribe from all topics
        for topic in &subscribed_topics {
            self.unsubscribe(topic.clone());
        }

        // re-subscribe modifying the fork version
        for mut topic in subscribed_topics {
            *topic.digest() = enr_fork_id.fork_digest;
            self.subscribe(topic);
        }

        // update the local reference
        self.enr_fork_id = enr_fork_id;
    }

    /* Private internal functions */

    /// Updates the current meta data of the node.
    fn update_metadata(&mut self) {
        self.meta_data.seq_number += 1;
        self.meta_data.attnets = self
            .discovery
            .local_enr()
            .bitfield::<TSpec>()
            .expect("Local discovery must have bitfield");
    }

    /// Sends a Ping request to the peer.
    fn ping(&mut self, id: RequestId, peer_id: PeerId) {
        let ping = crate::rpc::Ping {
            data: self.meta_data.seq_number,
        };
        debug!(self.log, "Sending Ping"; "request_id" => id, "peer_id" => peer_id.to_string());

        self.eth2_rpc
            .send_request(peer_id, id, RPCRequest::Ping(ping));
    }

    /// Sends a Pong response to the peer.
    fn pong(&mut self, id: PeerRequestId, peer_id: PeerId) {
        let ping = crate::rpc::Ping {
            data: self.meta_data.seq_number,
        };
        debug!(self.log, "Sending Pong"; "request_id" => id.1, "peer_id" => peer_id.to_string());
        let event = RPCCodedResponse::Success(RPCResponse::Pong(ping));
        self.eth2_rpc.send_response(peer_id, id, event);
    }

    /// Sends a METADATA request to a peer.
    fn send_meta_data_request(&mut self, peer_id: PeerId) {
        let event = RPCRequest::MetaData(PhantomData);
        self.eth2_rpc
            .send_request(peer_id, RequestId::Behaviour, event);
    }

    /// Sends a METADATA response to a peer.
    fn send_meta_data_response(&mut self, id: PeerRequestId, peer_id: PeerId) {
        let event = RPCCodedResponse::Success(RPCResponse::MetaData(self.meta_data.clone()));
        self.eth2_rpc.send_response(peer_id, id, event);
    }

    /// Returns a reference to the peer manager to allow the swarm to notify the manager of peer
    /// status
    pub fn peer_manager(&mut self) -> &mut PeerManager<TSpec> {
        &mut self.peer_manager
    }

    /* Address in the new behaviour. Connections are now maintained at the swarm level.
    /// Notifies the behaviour that a peer has connected.
    pub fn notify_peer_connect(&mut self, peer_id: PeerId, endpoint: ConnectedPoint) {
        match endpoint {
            ConnectedPoint::Dialer { .. } => self.peer_manager.connect_outgoing(&peer_id),
            ConnectedPoint::Listener { .. } => self.peer_manager.connect_ingoing(&peer_id),
        };

        // Find ENR info about a peer if possible.
        if let Some(enr) = self.discovery.enr_of_peer(&peer_id) {
            let bitfield = match enr.bitfield::<TSpec>() {
                Ok(v) => v,
                Err(e) => {
                    warn!(self.log, "Peer has invalid ENR bitfield";
                                        "peer_id" => format!("{}", peer_id),
                                        "error" => format!("{:?}", e));
                    return;
                }
            };

            // use this as a baseline, until we get the actual meta-data
            let meta_data = MetaData {
                seq_number: 0,
                attnets: bitfield,
            };
            // TODO: Shift to the peer manager
            self.network_globals
                .peers
                .write()
                .add_metadata(&peer_id, meta_data);
        }
    }
    */

    fn on_gossip_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message(propagation_source, id, gs_msg) => {
                // Note: We are keeping track here of the peer that sent us the message, not the
                // peer that originally published the message.
                if self.seen_gossip_messages.put(id.clone(), ()).is_none() {
                    match PubsubMessage::decode(&gs_msg.topics, &gs_msg.data) {
                        Err(e) => {
                            debug!(self.log, "Could not decode gossipsub message"; "error" => format!("{}", e))
                        }
                        Ok(msg) => {
                            // if this message isn't a duplicate, notify the network
                            self.events.push(BehaviourEvent::PubsubMessage {
                                id,
                                source: propagation_source,
                                topics: gs_msg.topics,
                                message: msg,
                            });
                        }
                    }
                } else {
                    match PubsubMessage::<TSpec>::decode(&gs_msg.topics, &gs_msg.data) {
                        Err(e) => {
                            debug!(self.log, "Could not decode gossipsub message"; "error" => format!("{}", e))
                        }
                        Ok(msg) => {
                            debug!(self.log, "A duplicate gossipsub message was received"; "message_source" => format!("{}", gs_msg.source), "propagated_peer" => format!("{}",propagation_source), "message" => format!("{}", msg));
                        }
                    }
                }
            }
            GossipsubEvent::Subscribed { peer_id, topic } => {
                self.events
                    .push(BehaviourEvent::PeerSubscribed(peer_id, topic));
            }
            GossipsubEvent::Unsubscribed { .. } => {}
        }
    }

    /// Queues the response to be sent upwards as long at it was requested outside the Behaviour.
    fn propagate_response(&mut self, id: RequestId, peer_id: PeerId, response: Response<TSpec>) {
        if !matches!(id, RequestId::Behaviour) {
            self.events.push(BehaviourEvent::ResponseReceived {
                peer_id,
                id,
                response,
            });
        }
    }

    /// Convenience function to propagate a request.
    fn propagate_request(&mut self, id: PeerRequestId, peer_id: PeerId, request: Request) {
        self.events.push(BehaviourEvent::RequestReceived {
            peer_id,
            id,
            request,
        });
    }

    fn on_rpc_event(&mut self, message: RPCMessage<TSpec>) {
        let peer_id = message.peer_id;
        let handler_id = message.conn_id;
        // The METADATA and PING RPC responses are handled within the behaviour and not propagated
        match message.event {
            Err(handler_err) => {
                match handler_err {
                    HandlerErr::Inbound {
                        id: _,
                        proto,
                        error,
                    } => {
                        // Inform the peer manager of the error.
                        // An inbound error here means we sent an error to the peer, or the stream
                        // timed out.
                        self.peer_manager.handle_rpc_error(&peer_id, proto, &error);
                    }
                    HandlerErr::Outbound { id, proto, error } => {
                        // Inform the peer manager that a request we sent to the peer failed
                        self.peer_manager.handle_rpc_error(&peer_id, proto, &error);
                        // inform failures of requests comming outside the behaviour
                        if !matches!(id, RequestId::Behaviour) {
                            self.events
                                .push(BehaviourEvent::RPCFailed { peer_id, id, error });
                        }
                    }
                }
            }
            Ok(RPCReceived::Request(id, request)) => {
                let peer_request_id = (handler_id, id);
                match request {
                    /* Behaviour managed protocols: Ping and Metadata */
                    RPCRequest::Ping(ping) => {
                        // inform the peer manager and send the response
                        self.peer_manager.ping_request(&peer_id, ping.data);
                        // send a ping response
                        self.pong(peer_request_id, peer_id);
                    }
                    RPCRequest::MetaData(_) => {
                        // send the requested meta-data
                        self.send_meta_data_response((handler_id, id), peer_id);
                        // TODO: inform the peer manager?
                    }
                    RPCRequest::Goodbye(reason) => {
                        // Peer asked to disconnect. Inform all handlers
                        // TODO: do not propagate
                        self.propagate_request(peer_request_id, peer_id, Request::Goodbye(reason));
                    }
                    /* Protocols propagated to the Network */
                    RPCRequest::Status(msg) => {
                        // inform the peer manager that we have received a status from a peer
                        self.peer_manager.peer_statusd(&peer_id);
                        // propagate the STATUS message upwards
                        self.propagate_request(peer_request_id, peer_id, Request::Status(msg))
                    }
                    RPCRequest::BlocksByRange(req) => self.propagate_request(
                        peer_request_id,
                        peer_id,
                        Request::BlocksByRange(req),
                    ),
                    RPCRequest::BlocksByRoot(req) => {
                        self.propagate_request(peer_request_id, peer_id, Request::BlocksByRoot(req))
                    }
                }
            }
            Ok(RPCReceived::Response(id, resp)) => {
                match resp {
                    /* Behaviour managed protocols */
                    RPCResponse::Pong(ping) => self.peer_manager.pong_response(&peer_id, ping.data),
                    RPCResponse::MetaData(meta_data) => {
                        self.peer_manager.meta_data_response(&peer_id, meta_data)
                    }
                    /* Network propagated protocols */
                    RPCResponse::Status(msg) => {
                        // inform the peer manager that we have received a status from a peer
                        self.peer_manager.peer_statusd(&peer_id);
                        // propagate the STATUS message upwards
                        self.propagate_response(id, peer_id, Response::Status(msg));
                    }
                    RPCResponse::BlocksByRange(resp) => {
                        self.propagate_response(id, peer_id, Response::BlocksByRange(Some(resp)))
                    }
                    RPCResponse::BlocksByRoot(resp) => {
                        self.propagate_response(id, peer_id, Response::BlocksByRoot(Some(resp)))
                    }
                }
            }
            Ok(RPCReceived::EndOfStream(id, termination)) => {
                let response = match termination {
                    ResponseTermination::BlocksByRange => Response::BlocksByRange(None),
                    ResponseTermination::BlocksByRoot => Response::BlocksByRoot(None),
                };
                self.propagate_response(id, peer_id, response);
            }
        }
    }

    /// Consumes the events list when polled.
    fn custom_poll<TBehaviourIn>(
        &mut self,
        cx: &mut Context,
    ) -> Poll<NBAction<TBehaviourIn, BehaviourEvent<TSpec>>> {
        // check the peer manager for events
        loop {
            match self.peer_manager.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => match event {
                    PeerManagerEvent::Status(peer_id) => {
                        // it's time to status. We don't keep a beacon chain reference here, so we inform
                        // the network to send a status to this peer
                        return Poll::Ready(NBAction::GenerateEvent(BehaviourEvent::StatusPeer(
                            peer_id,
                        )));
                    }
                    PeerManagerEvent::Ping(peer_id) => {
                        // send a ping request to this peer
                        self.ping(RequestId::Behaviour, peer_id);
                    }
                    PeerManagerEvent::MetaData(peer_id) => {
                        self.send_meta_data_request(peer_id);
                    }
                    PeerManagerEvent::DisconnectPeer(_peer_id) => {
                        //TODO: Implement
                    }
                },
                Poll::Pending => break,
                Poll::Ready(None) => break, // peer manager ended
            }
        }

        if !self.events.is_empty() {
            return Poll::Ready(NBAction::GenerateEvent(self.events.remove(0)));
        }

        Poll::Pending
    }

    fn on_identify_event(&mut self, event: IdentifyEvent) {
        match event {
            IdentifyEvent::Received {
                peer_id,
                mut info,
                observed_addr,
            } => {
                if info.listen_addrs.len() > MAX_IDENTIFY_ADDRESSES {
                    debug!(
                        self.log,
                        "More than 10 addresses have been identified, truncating"
                    );
                    info.listen_addrs.truncate(MAX_IDENTIFY_ADDRESSES);
                }
                // send peer info to the peer manager.
                self.peer_manager.identify(&peer_id, &info);

                debug!(self.log, "Identified Peer"; "peer" => format!("{}", peer_id),
                "protocol_version" => info.protocol_version,
                "agent_version" => info.agent_version,
                "listening_ addresses" => format!("{:?}", info.listen_addrs),
                "observed_address" => format!("{:?}", observed_addr),
                "protocols" => format!("{:?}", info.protocols)
                );
            }
            IdentifyEvent::Sent { .. } => {}
            IdentifyEvent::Error { .. } => {}
        }
    }

    fn on_discovery_event(&mut self, _event: Discv5Event) {
        // discv5 has no events to inject
    }
}

/* Public API types */

/// The type of RPC requests the Behaviour informs it has received and allows for sending.
///
// NOTE: This is an application-level wrapper over the lower network leve requests that can be
//       sent. The main difference is the absense of the Ping and Metadata protocols, which don't
//       leave the Behaviour. For all protocols managed by RPC see `RPCRequest`.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// A Status message.
    Status(StatusMessage),
    /// A Goobye message.
    Goodbye(GoodbyeReason),
    /// A blocks by range request.
    BlocksByRange(BlocksByRangeRequest),
    /// A request blocks root request.
    BlocksByRoot(BlocksByRootRequest),
}

impl<TSpec: EthSpec> std::convert::From<Request> for RPCRequest<TSpec> {
    fn from(req: Request) -> RPCRequest<TSpec> {
        match req {
            Request::BlocksByRoot(r) => RPCRequest::BlocksByRoot(r),
            Request::BlocksByRange(r) => RPCRequest::BlocksByRange(r),
            Request::Goodbye(r) => RPCRequest::Goodbye(r),
            Request::Status(s) => RPCRequest::Status(s),
        }
    }
}

/// The type of RPC responses the Behaviour informs it has received, and allows for sending.
///
// NOTE: This is an application-level wrapper over the lower network level responses that can be
//       sent. The main difference is the absense of Pong and Metadata, which don't leave the
//       Behaviour. For all protocol reponses managed by RPC see `RPCResponse` and
//       `RPCCodedResponse`.
#[derive(Debug, Clone, PartialEq)]
pub enum Response<TSpec: EthSpec> {
    /// A Status message.
    Status(StatusMessage),
    /// A response to a get BLOCKS_BY_RANGE request. A None response signals the end of the batch.
    BlocksByRange(Option<Box<SignedBeaconBlock<TSpec>>>),
    /// A response to a get BLOCKS_BY_ROOT request.
    BlocksByRoot(Option<Box<SignedBeaconBlock<TSpec>>>),
}

impl<TSpec: EthSpec> std::convert::From<Response<TSpec>> for RPCCodedResponse<TSpec> {
    fn from(resp: Response<TSpec>) -> RPCCodedResponse<TSpec> {
        match resp {
            Response::BlocksByRoot(r) => match r {
                Some(b) => RPCCodedResponse::Success(RPCResponse::BlocksByRoot(b)),
                None => RPCCodedResponse::StreamTermination(ResponseTermination::BlocksByRoot),
            },
            Response::BlocksByRange(r) => match r {
                Some(b) => RPCCodedResponse::Success(RPCResponse::BlocksByRange(b)),
                None => RPCCodedResponse::StreamTermination(ResponseTermination::BlocksByRange),
            },
            Response::Status(s) => RPCCodedResponse::Success(RPCResponse::Status(s)),
        }
    }
}

/// Identifier of requests sent by a peer.
pub type PeerRequestId = (ConnectionId, SubstreamId);

/// The types of events than can be obtained from polling the behaviour.
#[derive(Debug)]
pub enum BehaviourEvent<TSpec: EthSpec> {
    /// An RPC Request that was sent failed.
    RPCFailed {
        /// The id of the failed request.
        id: RequestId,
        /// The peer to which this request was sent.
        peer_id: PeerId,
        /// The error that occurred.
        error: RPCError,
    },
    RequestReceived {
        /// The peer that sent the request.
        peer_id: PeerId,
        /// Identifier of the request. All responses to this request must use this id.
        id: PeerRequestId,
        /// Request the peer sent.
        request: Request,
    },
    ResponseReceived {
        /// Peer that sent the response.
        peer_id: PeerId,
        /// Id of the request to which the peer is responding.
        id: RequestId,
        /// Response the peer sent.
        response: Response<TSpec>,
    },
    PubsubMessage {
        /// The gossipsub message id. Used when propagating blocks after validation.
        id: MessageId,
        /// The peer from which we received this message, not the peer that published it.
        source: PeerId,
        /// The topics that this message was sent on.
        topics: Vec<TopicHash>,
        /// The message itself.
        message: PubsubMessage<TSpec>,
    },
    /// Subscribed to peer for given topic
    PeerSubscribed(PeerId, TopicHash),
    /// Inform the network to send a Status to this peer.
    StatusPeer(PeerId),
}
