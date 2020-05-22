// Copyright 2019 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The special-purpose event loop used by the network manager.
//!
//! This event loop takes in events from Admin and State FIDL,
//! and implements handlers for FIDL calls.
//!
//! This is implemented with a single mpsc queue for all event types - `EventLoop` holds the
//! consumer, and any event handling that requires state within `EventLoop` holds a producer,
//! allowing it to delegate work to the `EventLoop` by sending a message. In this documentation, we
//! call anything that holds a producer a "worker".
//!
//! Having a single queue for all of the message types is beneficial, since in guarantees a FIFO
//! ordering for all messages - whichever messages arrive first, will be handled first.
//!
//! We'll look at each type of message, to see how each one is handled - starting with FIDL
//! messages, since they can be thought of as the entrypoint for the whole loop (as nothing happens
//! until a FIDL call is made).
//!
//! # FIDL Worker
//!
//! The FIDL part of the event loop implements the fuchsia.router.config Admin and State
//! interfaces. The type of the event loop message for a FIDL call is
//! simply the generated FIDL type. When the event loop starts up, we use `fuchsia_app` to start a
//! FIDL server that simply sends all of the events it receives to the event loop (via the sender
//! end of the mpsc queue). When `EventLoop` receives this message, it calls the
//! `handle_fidl_router_admin_request` or `handle_fidl_router_state_request` method, which,
//! depending on what the request is, either:
//!
//! * Responds with the requested information.
//! * Modifies the router state byt accessinc netcfg.
//! * Updates local state.

use std::cmp::Ordering;
use std::collections::HashSet;

use fidl_fuchsia_net_name as fname;
use fidl_fuchsia_net_name_ext::{CloneExt as _, IntoExt as _};
use fidl_fuchsia_router_config::{
    Id, Lif, Port, RouterAdminRequest, RouterStateGetPortsResponder, RouterStateRequest,
    SecurityFeatures,
};

use anyhow::{Context as _, Error};
use futures::channel::mpsc;
use futures::StreamExt;
use network_manager_core::{
    hal::NetCfg, lifmgr::LIFType, packet_filter::PacketFilter, portmgr::PortId, CloneExt as _,
    DeviceState,
};

use crate::dns_server_watcher::{DnsServerWatcher, DnsServerWatcherEvent, DnsServerWatcherSource};
use crate::event::Event;

pub(crate) const DEFAULT_DNS_PORT: u16 = 53;

macro_rules! router_error {
    ($code:ident, $desc:expr) => {
        Some(&mut fidl_fuchsia_router_config::Error {
            code: fidl_fuchsia_router_config::ErrorCode::$code,
            description: $desc,
        })
    };
}

macro_rules! not_supported {
    () => {
        router_error!(NotSupported, None)
    };
}
macro_rules! not_found {
    () => {
        router_error!(NotFound, None)
    };
}
macro_rules! internal_error {
    ($s:expr) => {
        router_error!(Internal, Some($s.to_string()))
    };
}

trait FromExt<T> {
    fn from(f: T) -> Self;
}

impl FromExt<fidl_fuchsia_net::IpAddress> for fname::DnsServer_ {
    /// Convert a [`fidl_fuchsia_net::IpAddress`] to a [`fname::DnsServer_`].
    ///
    /// The returned `fname::DnsServer_` will have its source set to static.
    fn from(f: fidl_fuchsia_net::IpAddress) -> fname::DnsServer_ {
        let socket_addr = match f {
            fidl_fuchsia_net::IpAddress::Ipv4(addr) => {
                fidl_fuchsia_net::SocketAddress::Ipv4(fidl_fuchsia_net::Ipv4SocketAddress {
                    address: addr,
                    port: DEFAULT_DNS_PORT,
                })
            }
            fidl_fuchsia_net::IpAddress::Ipv6(addr) => {
                fidl_fuchsia_net::SocketAddress::Ipv6(fidl_fuchsia_net::Ipv6SocketAddress {
                    address: addr,
                    port: DEFAULT_DNS_PORT,
                    zone_index: 0,
                })
            }
        };

        fname::DnsServer_ {
            address: Some(socket_addr),
            source: Some(fname::DnsServerSource::StaticSource(fname::StaticDnsServerSource {})),
        }
    }
}

/// The DNS servers learned from all sources.
#[derive(Default)]
struct DnsServers {
    /// DNS servers obtained from the netstack.
    netstack: Vec<fname::DnsServer_>,
}

impl DnsServers {
    /// Returns a consolidated list of servers.
    ///
    /// The servers will be returned deduplicated by their address and sorted by the source
    /// that each server was learned from. The servers will be sorted in most to least
    /// preferred order, with the most preferred server first. The preference of the servers
    /// is NDP, DHCPv4, DHCPv6 then Static, where NDP is the most preferred.
    ///
    /// Note, if multiple `fname::DnsServer_`s have the same address but different sources, only
    /// the `fname::DnsServer_` with the most prefered source will be present in the consolidated
    /// list of servers.
    fn consolidated(&self) -> Vec<fname::DnsServer_> {
        let mut set = HashSet::new();
        let mut servers = self.netstack.clone();
        servers.sort_by(DnsServers::ordering);

        servers
            .iter()
            .filter(move |s| set.insert(s.address))
            .map(fname::DnsServer_::clone)
            .collect()
    }

    /// Returns the ordering of [`fname::DnsServer_`]s.
    ///
    /// The ordering in greatest to least order is NDP, DHCPv4, DHCPv6 then Static.
    /// An unspecified source will be treated as a static address.
    fn ordering(a: &fname::DnsServer_, b: &fname::DnsServer_) -> Ordering {
        let a_source = a
            .source
            .as_ref()
            .map(|a| a.clone())
            .unwrap_or(fname::StaticDnsServerSource {}.into_ext());
        let b_source = b
            .source
            .as_ref()
            .map(|a| a.clone())
            .unwrap_or(fname::StaticDnsServerSource {}.into_ext());

        macro_rules! compare {
            ($a:ident, $b:ident, $is:path) => {
                if let $is(_) = $a {
                    return Ordering::Less;
                }
                if let $is(_) = $b {
                    return Ordering::Greater;
                }
            };
        }

        if a_source == b_source {
            return Ordering::Equal;
        }

        compare!(a_source, b_source, fname::DnsServerSource::Ndp);
        compare!(a_source, b_source, fname::DnsServerSource::Dhcp);
        compare!(a_source, b_source, fname::DnsServerSource::Dhcpv6);
        compare!(a_source, b_source, fname::DnsServerSource::StaticSource);

        // Should never end up here as the equality check for sources was done earlier.
        unreachable!("a and b's sources should not be equal; a = {:?}, b = {:?}", a, b)
    }
}

/// The event loop.
pub struct EventLoop {
    event_recv: mpsc::UnboundedReceiver<Event>,
    device: DeviceState,
    dns_servers: DnsServers,
}

impl EventLoop {
    pub fn new() -> Result<Self, Error> {
        let netcfg = NetCfg::new()?;
        let packet_filter = PacketFilter::start()
            .context("network_manager failed to start packet filter!")
            .unwrap();
        let mut device = DeviceState::new(netcfg, packet_filter);

        let streams = device.take_event_streams();
        let (event_send, event_recv) = futures::channel::mpsc::unbounded::<Event>();
        if cfg!(test) {
            // Network Manager's FIDL is currently disabled. We may eventually
            // remove the support for it from the codebase entirely, but for the
            // time being, we would still like to exercise the functionality
            // during testing to avoid bitrot.
            let fidl_worker = crate::fidl_worker::FidlWorker;
            let _ = fidl_worker.spawn(event_send.clone());
        }
        let overnet_worker = crate::overnet_worker::OvernetWorker;
        let _r = overnet_worker.spawn(event_send.clone());
        let event_worker = crate::event_worker::EventWorker;
        event_worker.spawn(streams, event_send.clone());
        let oir_worker = crate::oir_worker::OirWorker;
        oir_worker.spawn(event_send.clone());

        let netstack_dns_watcher = DnsServerWatcher::new(
            DnsServerWatcherSource::Netstack,
            device.get_netstack_dns_server_watcher()?,
        );
        netstack_dns_watcher.spawn(event_send);

        Ok(EventLoop { event_recv, device, dns_servers: Default::default() })
    }

    pub async fn run(mut self) -> Result<(), Error> {
        if let Err(e) = self.device.load_config().await {
            // TODO(cgibson): Kick off some sort of recovery process here: Prompt the user to
            // download a recovery app, set a static IP on the first interface and set everything
            // else down, restructure packet filter rules, etc.
            error!("Failed to load a device config: {}", e);
        }
        self.device.setup_services().await?;
        self.device.populate_state().await?;

        loop {
            match self.event_recv.next().await {
                Some(Event::FidlRouterAdminEvent(req)) => {
                    self.handle_fidl_router_admin_request(req).await;
                }
                Some(Event::FidlRouterStateEvent(req)) => {
                    self.handle_fidl_router_state_request(req).await;
                }
                Some(Event::StackEvent(event)) => self.handle_stack_event(event).await,
                Some(Event::NetstackEvent(event)) => self.handle_netstack_event(event).await,
                Some(Event::OIR(event)) => self.handle_oir_event(event).await,
                Some(Event::DnsServerWatcher(event)) => {
                    self.handle_dns_server_watcher_event(event).await
                }
                None => return Err(anyhow::format_err!("Stream of events ended unexpectedly")),
            }
        }
    }

    async fn handle_dns_server_watcher_event(&mut self, event: DnsServerWatcherEvent) {
        let DnsServerWatcherEvent { source, servers } = event;

        match source {
            DnsServerWatcherSource::Netstack => self.dns_servers.netstack = servers,
        }

        let () = self
            .device
            .set_dns_resolvers(self.dns_servers.consolidated())
            .await
            .map(|_| ())
            .unwrap_or_else(|err| warn!("error setting dns servers: {:?}", err));
    }

    async fn handle_stack_event(&mut self, event: fidl_fuchsia_net_stack::StackEvent) {
        self.device
            .update_state_for_stack_event(event)
            .await
            .unwrap_or_else(|err| warn!("error updating state: {:?}", err));
    }

    async fn handle_netstack_event(&mut self, event: fidl_fuchsia_netstack::NetstackEvent) {
        self.device
            .update_state_for_netstack_event(event)
            .await
            .unwrap_or_else(|err| warn!("error updating state: {:?}", err));
    }

    async fn handle_oir_event(&mut self, event: network_manager_core::oir::OIRInfo) {
        self.device
            .oir_event(event)
            .await
            .unwrap_or_else(|err| warn!("error processing oir event: {:?}", err));
    }

    async fn handle_fidl_router_admin_request(&mut self, req: RouterAdminRequest) {
        match req {
            RouterAdminRequest::CreateWan { name, vlan, ports, responder } => {
                let r = self
                    .fidl_create_lif(
                        LIFType::WAN,
                        name,
                        vlan,
                        ports.iter().map(|x| PortId::from(u64::from(*x))).collect(),
                    )
                    .await;
                match r {
                    Ok(mut id) => responder.send(Some(&mut id), None),
                    Err(mut e) => responder.send(None, Some(&mut e)),
                }
            }
            RouterAdminRequest::CreateLan { name, vlan, ports, responder } => {
                let r = self
                    .fidl_create_lif(
                        LIFType::LAN,
                        name,
                        vlan,
                        ports.iter().map(|x| PortId::from(u64::from(*x))).collect(),
                    )
                    .await;
                match r {
                    Ok(mut id) => responder.send(Some(&mut id), None),
                    Err(mut e) => responder.send(None, Some(&mut e)),
                }
            }
            RouterAdminRequest::RemoveWan { wan_id, responder } => {
                let mut r = self.fidl_delete_lif(wan_id).await;
                responder.send(r.as_mut()).or_else(|e| {
                    error!("Error sending response: {:?}", e);
                    Err(e)
                })
            }
            RouterAdminRequest::RemoveLan { lan_id, responder } => {
                let mut r = self.fidl_delete_lif(lan_id).await;
                responder.send(r.as_mut()).or_else(|e| {
                    error!("Error sending response: {:?}", e);
                    Err(e)
                })
            }
            RouterAdminRequest::SetWanProperties { wan_id, properties, responder } => {
                let properties = fidl_fuchsia_router_config::LifProperties::Wan(properties);
                if self
                    .device
                    .update_lif_properties_fidl(u128::from_ne_bytes(wan_id.uuid), &properties)
                    .await
                    .is_err()
                {
                    warn!("WAN {:?} found but failed to update properties", wan_id);
                    responder.send(not_found!())
                } else {
                    info!("WAN properties updated");
                    responder.send(None)
                }
            }
            RouterAdminRequest::SetLanProperties { lan_id, properties, responder } => {
                let properties = fidl_fuchsia_router_config::LifProperties::Lan(properties);
                if self
                    .device
                    .update_lif_properties_fidl(u128::from_ne_bytes(lan_id.uuid), &properties)
                    .await
                    .is_err()
                {
                    warn!("failed to update LAN properties");
                    responder.send(not_found!())
                } else {
                    info!("LAN properties updated");
                    responder.send(None)
                }
            }
            RouterAdminRequest::SetDhcpServerOptions { lan_id, options, responder } => {
                info!("{:?}, {:?}", lan_id, options);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetDhcpAddressPool { lan_id, pool, responder } => {
                info!("{:?}, {:?}", lan_id, pool);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetDhcpReservation { lan_id, reservation, responder } => {
                info!("{:?}, {:?}", lan_id, reservation);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeleteDhcpReservation { reservation_id, responder } => {
                info!("{:?}", reservation_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetDnsResolver { config, responder } => {
                info!("{:?}", config);
                match self
                    .device
                    .set_dns_resolvers(
                        config
                            .search
                            .servers
                            .into_iter()
                            .map(<fname::DnsServer_ as FromExt<fidl_fuchsia_net::IpAddress>>::from)
                            .collect::<Vec<_>>(),
                    )
                    .await
                {
                    Ok(i) => responder.send(
                        Some(&mut Id { uuid: i.uuid().to_ne_bytes(), version: i.version() }),
                        None,
                    ),
                    Err(e) => responder.send(None, internal_error!(e.to_string())),
                }
            }
            RouterAdminRequest::SetDnsForwarder { config, responder } => {
                info!("{:?}", config);
                responder.send(not_supported!())
            }
            RouterAdminRequest::AddDnsEntry { entry, responder } => {
                info!("{:?}", entry);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeleteDnsEntry { entry_id, responder } => {
                info!("{:?}", entry_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetRoute { route, responder } => {
                info!("{:?}", route);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::UpdateRouteMetric { route_id, metric, responder } => {
                info!("{:?} {:?}", route_id, metric);
                responder.send(not_supported!())
            }
            RouterAdminRequest::DeleteRoute { route_id, responder } => {
                info!("{:?}", route_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetSecurityFeatures { features, responder } => {
                info!("Updating SecurityFeatures: {:?}", features);
                match self.update_security_features(&features).await {
                    Ok(_) => responder.send(None),
                    Err(e) => responder.send(internal_error!(e.to_string())),
                }
            }
            RouterAdminRequest::SetPortForward { rule, responder } => {
                info!("{:?}", rule);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeletePortForward { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetPortTrigger { rule, responder } => {
                info!("{:?}", rule);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeletePortTrigger { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetFilter { rule, responder } => {
                let r = self
                    .device
                    // TODO(45024): The Router Config FIDL API doesn't have a way to provide a
                    // specific interface identifier.
                    .set_filter_on_interface(&rule, 0u32)
                    .await
                    .context("Error installing new packet filter on all interfaces");
                match r {
                    Ok(()) => responder.send(None, None),
                    Err(e) => responder.send(None, internal_error!(e.to_string())),
                }
            }
            RouterAdminRequest::DeleteFilter { rule_id, responder } => {
                info!("{:?}", rule_id);
                let r = self
                    .device
                    .delete_filter(rule_id)
                    .await
                    .context("Error deleting packet filter rule");
                match r {
                    Ok(()) => responder.send(None),
                    Err(e) => responder.send(internal_error!(e.to_string())),
                }
            }
            RouterAdminRequest::SetIpv6PinHole { rule, responder } => {
                info!("{:?}", rule);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeleteIpv6PinHole { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetDmzHost { rule, responder } => {
                info!("{:?}", rule);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeleteDmzHost { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(not_supported!())
            }
            RouterAdminRequest::SetSystemConfig { config, responder } => {
                info!("{:?}", config);
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::CreateWlanNetwork { responder, .. } => {
                // TODO(guzt): implement
                responder.send(None, not_supported!())
            }
            RouterAdminRequest::DeleteWlanNetwork { responder, .. } => {
                // TODO(guzt): implement
                responder.send(not_supported!())
            }
        }
        .unwrap_or_else(|e| error!("Error sending response {}", e))
    }

    async fn fidl_create_lif(
        &mut self,
        lif_type: LIFType,
        name: String,
        vlan: u16,
        ports: Vec<PortId>,
    ) -> Result<Id, fidl_fuchsia_router_config::Error> {
        let lif = self.device.create_lif(lif_type, name, Some(vlan), &ports).await;
        match lif {
            Err(e) => {
                error!("Error creating lif {:?}", e);
                Err(fidl_fuchsia_router_config::Error {
                    code: fidl_fuchsia_router_config::ErrorCode::AlreadyExists,
                    description: None,
                })
            }
            Ok(l) => {
                let i = l.id();
                Ok(Id { uuid: i.uuid().to_ne_bytes(), version: i.version() })
            }
        }
    }

    async fn fidl_delete_lif(&mut self, id: Id) -> Option<fidl_fuchsia_router_config::Error> {
        let lif = self.device.delete_lif(u128::from_ne_bytes(id.uuid)).await;
        match lif {
            Err(e) => {
                error!("Error deleting lif {:?}", e);
                Some(fidl_fuchsia_router_config::Error {
                    code: fidl_fuchsia_router_config::ErrorCode::NotFound,
                    description: None,
                })
            }
            Ok(()) => None,
        }
    }

    async fn update_security_features(
        &mut self,
        security_features: &SecurityFeatures,
    ) -> Result<(), Error> {
        if let Some(nat) = security_features.nat {
            if nat {
                self.device.enable_nat();
            } else {
                self.device.disable_nat();
            }
        }
        // TODO(cgibson): Handle additional SecurityFeatures.
        Ok(())
    }

    async fn handle_fidl_router_state_request(&mut self, req: RouterStateRequest) {
        match req {
            RouterStateRequest::GetWanPorts { wan_id, responder } => {
                let lif = self.device.lif(u128::from_ne_bytes(wan_id.uuid));
                match lif {
                    None => {
                        warn!("WAN {:?} not found", wan_id);
                        responder.send(&[], not_found!())
                    }
                    Some(l) => {
                        responder.send(&l.ports().map(|p| p.to_u32()).collect::<Vec<_>>(), None)
                    }
                }
            }
            RouterStateRequest::GetLanPorts { lan_id, responder } => {
                let lif = self.device.lif(u128::from_ne_bytes(lan_id.uuid));
                match lif {
                    None => {
                        warn!("LAN {:?} not found", lan_id);
                        responder.send(&[], not_found!())
                    }
                    Some(l) => {
                        responder.send(&l.ports().map(|p| p.to_u32()).collect::<Vec<_>>(), None)
                    }
                }
            }
            RouterStateRequest::GetWan { wan_id, responder } => {
                let lif = self.device.lif(u128::from_ne_bytes(wan_id.uuid));
                info!("lifs {:?}", lif);
                match lif {
                    None => {
                        warn!("WAN {:?} not found", wan_id);
                        responder.send(
                            fidl_fuchsia_router_config::Lif {
                                element: None,
                                name: None,
                                port_ids: None,
                                properties: None,
                                vlan: None,
                                type_: None,
                            },
                            not_found!(),
                        )
                    }
                    Some(l) => {
                        let ll = l.into();
                        responder.send(ll, None)
                    }
                }
            }
            RouterStateRequest::GetLan { lan_id, responder } => {
                let lif = self.device.lif(u128::from_ne_bytes(lan_id.uuid));
                match lif {
                    None => {
                        warn!("LAN {:?} not found", lan_id);
                        responder.send(
                            fidl_fuchsia_router_config::Lif {
                                element: None,
                                name: None,
                                port_ids: None,
                                properties: None,
                                vlan: None,
                                type_: None,
                            },
                            not_found!(),
                        )
                    }
                    Some(l) => {
                        let ll = l.into();
                        responder.send(ll, None)
                    }
                }
            }
            RouterStateRequest::GetWans { responder } => {
                let lifs: Vec<Lif> = self.device.lifs(LIFType::WAN).map(|l| l.into()).collect();
                info!("result: {:?} ", lifs);
                responder.send(&mut lifs.into_iter())
            }
            RouterStateRequest::GetLans { responder } => {
                let lifs: Vec<Lif> = self.device.lifs(LIFType::LAN).map(|l| l.into()).collect();
                info!("result: {:?} ", lifs);
                responder.send(&mut lifs.into_iter())
            }
            RouterStateRequest::GetWanProperties { wan_id, responder } => {
                info!("{:?}", wan_id);
                let properties = fidl_fuchsia_router_config::WanProperties {
                    connection_type: None,
                    connection_parameters: None,
                    address_method: None,
                    address_v4: None,
                    gateway_v4: None,
                    connection_v6_mode: None,
                    address_v6: None,
                    gateway_v6: None,
                    hostname: None,
                    clone_mac: None,
                    mtu: None,
                    enable: None,
                    metric: None,
                };
                responder.send(properties, not_supported!())
            }
            RouterStateRequest::GetLanProperties { lan_id, responder } => {
                info!("{:?}", lan_id);
                let properties = fidl_fuchsia_router_config::LanProperties {
                    address_v4: None,
                    enable_dhcp_server: None,
                    dhcp_config: None,
                    address_v6: None,
                    enable_dns_forwarder: None,
                    enable: None,
                };

                responder.send(properties, not_supported!())
            }
            RouterStateRequest::GetDhcpConfig { lan_id, responder } => {
                info!("{:?}", lan_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetDnsResolver { responder } => {
                let mut resolver = self.device.get_dns_resolver().await;
                responder.send(&mut resolver)
            }
            RouterStateRequest::GetDnsForwarder { responder } => {
                let config = fidl_fuchsia_router_config::DnsForwarderConfig {
                    element: Id {
                        uuid: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                        version: 0,
                    },
                    search: fidl_fuchsia_router_config::DnsSearch {
                        domain_name: None,
                        servers: vec![],
                    },
                };
                let mut forwarder = fidl_fuchsia_router_config::DnsForwarder {
                    config,
                    interfaces: vec![],
                    resolver: vec![],
                };
                responder.send(&mut forwarder)
            }
            RouterStateRequest::GetRoutes { responder } => responder.send(&mut [].iter_mut()),
            RouterStateRequest::GetRoute { route_id, responder } => {
                info!("{:?}", route_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetSecurityFeatures { responder } => {
                let security_features = fidl_fuchsia_router_config::SecurityFeatures {
                    allow_multicast: None,
                    drop_icmp_echo: None,
                    firewall: None,
                    h323_passthru: None,
                    ipsec_passthru: None,
                    l2_tp_passthru: None,
                    nat: Some(self.device.is_nat_enabled()),
                    pptp_passthru: None,
                    rtsp_passthru: None,
                    sip_passthru: None,
                    upnp: None,
                    v6_firewall: None,
                };
                responder.send(security_features)
            }
            RouterStateRequest::GetPortForwards { responder } => responder.send(&mut [].iter_mut()),
            RouterStateRequest::GetPortForward { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetPorts { responder } => {
                self.fidl_get_ports(responder).await;
                Ok(())
            }
            RouterStateRequest::GetPort { port_id, responder } => {
                info!("{:?}", port_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetPortTrigger { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetPortTriggers { responder } => responder.send(&mut [].iter_mut()),
            RouterStateRequest::GetFilter { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetFilters { responder } => {
                let result = self.device.get_filters().await.context("Error getting filters");
                let mut filter_rules = Vec::new();
                match result {
                    Ok(f) => {
                        filter_rules = f.into_iter().collect();
                        info!("Filter rules returned: {:?}", filter_rules.len());
                    }
                    Err(e) => error!("Failed parsing filter rules: {}", e),
                }
                responder.send(&mut filter_rules.iter_mut())
            }
            RouterStateRequest::GetIpv6PinHole { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetIpv6PinHoles { responder } => responder.send(&mut [].iter_mut()),
            RouterStateRequest::GetDmzHost { rule_id, responder } => {
                info!("{:?}", rule_id);
                responder.send(None, not_supported!())
            }
            RouterStateRequest::GetSystemConfig { responder } => {
                let config = fidl_fuchsia_router_config::SystemConfig {
                    element: None,
                    timezone: None,
                    daylight_savings_time_enabled: None,
                    leds_enabled: None,
                    hostname: None,
                };
                responder.send(config)
            }
            RouterStateRequest::GetDevice { responder } => {
                let device = fidl_fuchsia_router_config::Device {
                    version: None,
                    topology: None,
                    config: None,
                };
                responder.send(device)
            }
            RouterStateRequest::GetWlanNetworks { responder } => {
                responder.send(&mut vec![].into_iter())
            }
            RouterStateRequest::GetRadios { responder } => responder.send(&mut vec![].into_iter()),
        }
        .unwrap_or_else(|e| error!("Error sending response {}", e))
    }

    async fn fidl_get_ports(&mut self, responder: RouterStateGetPortsResponder) {
        let ps = self.device.ports();
        let mut ports: Vec<Port> = ps
            .map(|p| Port {
                element: Id { uuid: p.e_id.uuid().to_ne_bytes(), version: p.e_id.version() },
                id: p.port_id.to_u32(),
                path: p.path.clone(),
            })
            .collect();
        responder.send(&mut ports.iter_mut()).context("Error sending a response").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::dns::*;

    #[test]
    fn test_dnsservers_consolidation() {
        // Simple deduplication and sorting of repeated `DnsServer_`.
        let servers = DnsServers {
            netstack: vec![
                DHCP_SERVER,
                DHCPV6_SERVER,
                NDP_SERVER,
                STATIC_SERVER,
                NDP_SERVER,
                DHCPV6_SERVER,
                DHCP_SERVER,
                STATIC_SERVER,
            ],
        };
        assert_eq!(
            servers.consolidated(),
            vec![NDP_SERVER, DHCP_SERVER, DHCPV6_SERVER, STATIC_SERVER]
        );

        // Deduplication and sorting of same address across different sources.

        // DHCPv6 is not as preferred as NDP so this should not be in the consolidated
        // list.
        let mut dhcpv6_with_ndp_address = NDP_SERVER;
        dhcpv6_with_ndp_address.source =
            Some(fname::DnsServerSource::Dhcpv6(fname::Dhcpv6DnsServerSource {
                source_interface: Some(3),
            }));
        let servers = DnsServers {
            netstack: vec![
                dhcpv6_with_ndp_address.clone(),
                DHCP_SERVER,
                DHCPV6_SERVER,
                NDP_SERVER,
                STATIC_SERVER,
            ],
        };
        let expected = vec![NDP_SERVER, DHCP_SERVER, DHCPV6_SERVER, STATIC_SERVER];
        assert_eq!(servers.consolidated(), expected);
        let servers = DnsServers {
            netstack: vec![
                DHCP_SERVER,
                DHCPV6_SERVER,
                NDP_SERVER,
                STATIC_SERVER,
                dhcpv6_with_ndp_address,
            ],
        };
        assert_eq!(servers.consolidated(), expected);

        // NDP is more preferred than DHCPv6 so `DHCPV6_SERVER` should not be in the consolidated
        // list.
        let mut ndp_with_dhcpv6_address = DHCPV6_SERVER;
        ndp_with_dhcpv6_address.source =
            Some(fname::DnsServerSource::Ndp(fname::NdpDnsServerSource {
                source_interface: Some(3),
            }));
        let servers = DnsServers {
            netstack: vec![
                ndp_with_dhcpv6_address.clone(),
                DHCP_SERVER,
                DHCPV6_SERVER,
                STATIC_SERVER,
            ],
        };
        let expected = vec![ndp_with_dhcpv6_address.clone(), DHCP_SERVER, STATIC_SERVER];
        assert_eq!(servers.consolidated(), expected);
        let servers = DnsServers {
            netstack: vec![DHCP_SERVER, DHCPV6_SERVER, STATIC_SERVER, ndp_with_dhcpv6_address],
        };
        assert_eq!(servers.consolidated(), expected);
    }

    #[test]
    fn test_dns_servers_ordering() {
        assert_eq!(DnsServers::ordering(&NDP_SERVER, &NDP_SERVER), Ordering::Equal);
        assert_eq!(DnsServers::ordering(&DHCP_SERVER, &DHCP_SERVER), Ordering::Equal);
        assert_eq!(DnsServers::ordering(&DHCPV6_SERVER, &DHCPV6_SERVER), Ordering::Equal);
        assert_eq!(DnsServers::ordering(&STATIC_SERVER, &STATIC_SERVER), Ordering::Equal);
        assert_eq!(
            DnsServers::ordering(&UNSPECIFIED_SOURCE_SERVER, &UNSPECIFIED_SOURCE_SERVER),
            Ordering::Equal
        );
        assert_eq!(
            DnsServers::ordering(&STATIC_SERVER, &UNSPECIFIED_SOURCE_SERVER),
            Ordering::Equal
        );

        let servers =
            [NDP_SERVER, DHCP_SERVER, DHCPV6_SERVER, STATIC_SERVER, UNSPECIFIED_SOURCE_SERVER];
        // We don't compare the last two servers in the list because their ordering is equal
        // w.r.t. eachother.
        for (i, a) in servers[..servers.len() - 2].iter().enumerate() {
            for b in servers[i + 1..].iter() {
                assert_eq!(DnsServers::ordering(a, b), Ordering::Less);
            }
        }

        let mut servers = vec![DHCPV6_SERVER, DHCP_SERVER, STATIC_SERVER, NDP_SERVER];
        servers.sort_by(DnsServers::ordering);
        assert_eq!(servers, vec![NDP_SERVER, DHCP_SERVER, DHCPV6_SERVER, STATIC_SERVER]);
    }
}
