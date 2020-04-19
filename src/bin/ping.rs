//! `ping` - send ICMP ECHO_REQUEST to network hosts
//!
//! Warning, needs to be run with `sudo`
//!
//! Todo:
//!  - fix ipv6 sequence number (payload)
//!  - use icmp identifier so you can run `ping` bins concurrently
//!  - report ttl, damaged replies
//!  - all other options

use coreutils::util::{emit_bell, print_help_and_exit};

use std::net::IpAddr;
use std::net::ToSocketAddrs;
use std::process::exit;
use std::time::Duration;
use std::time::Instant;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use pnet::packet::icmp::echo_reply::EchoReplyPacket;
use pnet::packet::icmp::echo_request::MutableEchoRequestPacket;
use pnet::packet::icmp::IcmpTypes;
use pnet::packet::icmpv6::Icmpv6Types;
use pnet::packet::icmpv6::{Icmpv6Packet, MutableIcmpv6Packet};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::util;
use pnet::packet::Packet;
use pnet::transport::TransportChannelType::Layer4;
use pnet::transport::TransportProtocol::{Ipv4, Ipv6};
use pnet::transport::{icmp_packet_iter, icmpv6_packet_iter, transport_channel, TransportReceiver};

const USAGE: &str = "ping [-a] [-4|-6] <dest>: send ICMP ECHO_REQUEST to network hosts";

/// State of the ICMP echo request
enum Echo {
    /// sent at given instant
    Sent(Instant),
    /// received with ping in millis
    Received(u128),
}

/// IP version number
enum IpVersion {
    V4,
    V6,
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut audible = false;
    let mut pref_ip_version = None;
    let mut dest = None;

    while let Some(arg) = args.next() {
        if &arg == "-a" {
            audible = true;
        } else if &arg == "-4" {
            pref_ip_version = Some(IpVersion::V4);
        } else if &arg == "-6" {
            pref_ip_version = Some(IpVersion::V6);
        } else {
            dest = Some(arg);
            break;
        }
    }

    if args.next().is_some() {
        print_help_and_exit(USAGE);
    }

    let dest = match dest {
        None => print_help_and_exit(USAGE),
        Some(dest) => dest,
    };

    match ping(dest, audible, pref_ip_version) {
        Ok(0) => exit(2), // no replies received
        Ok(_) => exit(0), // replies received
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    }
}

/// register received echo reply
fn register_echo_reply(slot: Option<&mut Echo>) -> Option<u128> {
    match slot {
        None => {
            eprintln!("received unsollicited ICMP echo response");
            None
        }
        Some(Echo::Received(_)) => {
            eprintln!("received duplicate response");
            None
        }
        Some(echo) => {
            let sent = match *echo {
                Echo::Sent(sent) => sent,
                _ => unreachable!(),
            };
            let ping = Instant::now().duration_since(sent).as_millis();
            *echo = Echo::Received(ping);

            Some(ping)
        }
    }
}

/// IPv4 ICMP reply handling
fn listen_ipv4(mut rx: TransportReceiver, audible: bool, pings_recv: Arc<Mutex<Vec<Echo>>>) -> ! {
    let mut iter = icmp_packet_iter(&mut rx);
    while let Ok((packet, addr)) = iter.next() {
        if audible {
            emit_bell();
        }

        let echo_reply = EchoReplyPacket::new(packet.packet()).unwrap();
        let sequence_number = echo_reply.get_sequence_number();

        // todo better handling of sequence_number 0
        let mut lock = pings_recv.lock().unwrap();
        let slot = lock.get_mut(sequence_number as usize - 1);
        if let Some(ping) = register_echo_reply(slot) {
            println!(
                "{} bytes from {}: icmp_seq={} ttl=.. time={} ms",
                packet.packet().len(),
                addr,
                sequence_number,
                ping
            );
        }
    }

    panic!("thread listener quit unexpectedly");
}

/// IPv6 ICMP reply handling
fn listen_ipv6(mut rx: TransportReceiver, audible: bool, pings_recv: Arc<Mutex<Vec<Echo>>>) -> ! {
    let mut iter = icmpv6_packet_iter(&mut rx);
    while let Ok((packet, addr)) = iter.next() {
        if audible {
            emit_bell();
        }

        let _echo_reply = Icmpv6Packet::new(packet.packet()).unwrap();
        let sequence_number = 1; // TODO

        let mut lock = pings_recv.lock().unwrap();
        let slot = lock.get_mut(sequence_number as usize - 1);
        if let Some(ping) = register_echo_reply(slot) {
            println!(
                "{} bytes from {}: icmp_seq={} ttl=.. time={} ms",
                packet.packet().len(),
                addr,
                sequence_number,
                ping
            );
        }
    }

    panic!("thread listener quit unexpectedly");
}

fn build_ipv4_request<'a>(vec: &'a mut Vec<u8>, sequence_number: u16) -> impl Packet + 'a {
    let mut echo_packet = MutableEchoRequestPacket::new(vec).unwrap();

    echo_packet.set_identifier(0);
    echo_packet.set_icmp_type(IcmpTypes::EchoRequest);
    echo_packet.set_sequence_number(sequence_number);

    let checksum = util::checksum(echo_packet.packet(), 1);
    echo_packet.set_checksum(checksum);

    echo_packet
}

fn build_ipv6_request<'a>(vec: &'a mut Vec<u8>, _sequence_number: u16) -> impl Packet + 'a {
    let mut echo_packet = MutableIcmpv6Packet::new(&mut vec[..]).unwrap();

    echo_packet.set_icmpv6_type(Icmpv6Types::EchoRequest);
    // todo set payload (sequence number)

    let checksum = util::checksum(echo_packet.packet(), 1);
    echo_packet.set_checksum(checksum);

    echo_packet
}

/// `ping` implementation
fn ping(
    dest: String,
    audible: bool,
    pref_ip_version: Option<IpVersion>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let addrs_iter = format!("{}:12345", dest).to_socket_addrs()?;
    let ip = addrs_iter
        .map(|addr| addr.ip())
        .find(|ip| match pref_ip_version {
            None => true,
            Some(IpVersion::V4) => match ip {
                IpAddr::V4(_) => true,
                _ => false,
            },
            Some(IpVersion::V6) => match ip {
                IpAddr::V6(_) => true,
                _ => false,
            },
        })
        .expect("unable to resolve address");

    let protocol = match ip {
        IpAddr::V4(_) => Layer4(Ipv4(IpNextHeaderProtocols::Icmp)),
        IpAddr::V6(_) => Layer4(Ipv6(IpNextHeaderProtocols::Icmpv6)),
    };
    let (mut tx, rx) = transport_channel(4096, protocol)?;

    let pings = Arc::new(Mutex::new(vec![]));

    // spin up the thread that receives the ICMP echo replies
    let pings_recv = pings.clone();
    thread::spawn(move || match ip {
        IpAddr::V4(_) => listen_ipv4(rx, audible, pings_recv),
        IpAddr::V6(_) => listen_ipv6(rx, audible, pings_recv),
    });

    let mut vec: Vec<u8> = vec![0; 16];
    println!("PING {} ({}): {} data bytes", dest, ip, vec.len());

    // setup CTRL-C handler at this point, not earlier
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    while running.load(Ordering::SeqCst) {
        let sequence_number = {
            let mut pings = pings.lock().unwrap();
            pings.push(Echo::Sent(Instant::now()));
            pings.len() as u16 // start at 1
        };
        match ip {
            IpAddr::V4(_) => {
                let echo_packet = build_ipv4_request(&mut vec, sequence_number);
                tx.send_to(echo_packet, ip)?
            }
            IpAddr::V6(_) => {
                let echo_packet = build_ipv6_request(&mut vec, sequence_number);
                tx.send_to(echo_packet, ip)?
            }
        };

        // sleep for small periods and check for ctrl-c
        for _ in 0..10 {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    let pings = pings.lock().unwrap();
    let transmitted = pings.len();
    let received_ms: Vec<_> = pings
        .iter()
        .filter_map(|echo| match echo {
            Echo::Sent(_) => None,
            Echo::Received(millis) => Some(millis),
        })
        .cloned()
        .collect();
    let received = received_ms.len();

    print!("{} packets transmitted, ", transmitted);
    print!("{} packets received, ", received_ms.len());
    if transmitted > 0 {
        println!(
            "{:.2}% packet loss",
            ((transmitted - received) as f32) / transmitted as f32
        );
    }

    if received > 0 {
        let mean = received_ms.iter().sum::<u128>() as f32 / (received as f32);
        let variance = received_ms
            .iter()
            .map(|value| {
                let diff = mean - (*value as f32);
                diff * diff
            })
            .sum::<f32>()
            / received as f32;

        println!(
            "round-trip min/avg/max/stddev = {:4.2}/{:4.2}/{:4.2}/{:4.2} ms",
            received_ms.iter().min().unwrap(),
            mean,
            received_ms.iter().max().unwrap(),
            variance.sqrt(),
        );
    }

    Ok(received)
}
