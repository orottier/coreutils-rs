//! `ping` - send ICMP ECHO_REQUEST to network hosts
//!
//! Warning, needs to be run with `sudo`
//!
//! Todo:
//!  - resolve addrs
//!  - support IPv6
//!  - report ttl, damaged replies
//!  - all other options

use coreutils::{emit_bell, print_help_and_exit};

use std::net::IpAddr;
use std::process::exit;
use std::time::Duration;
use std::time::Instant;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use pnet::packet::icmp::echo_reply::EchoReplyPacket;
use pnet::packet::icmp::echo_request::MutableEchoRequestPacket;
use pnet::packet::icmp::IcmpTypes;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::util;
use pnet::packet::Packet;
use pnet::transport::TransportChannelType::Layer4;
use pnet::transport::TransportProtocol::{Ipv4, Ipv6};
use pnet::transport::{icmp_packet_iter, icmpv6_packet_iter, transport_channel};

const USAGE: &str = "ping <dest>: send ICMP ECHO_REQUEST to network hosts";

/// State of the ICMP echo request
enum Echo {
    /// sent at given instant
    Sent(Instant),
    /// received with ping in millis
    Received(u128),
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut audible = false;
    let dest = match args.next() {
        Some(s) if &s == "-a" => match args.next() {
            Some(dest) => {
                audible = true;
                dest
            }
            None => print_help_and_exit(USAGE),
        },
        Some(dest) => dest,
        None => print_help_and_exit(USAGE),
    };

    if args.next().is_some() {
        print_help_and_exit(USAGE);
    }

    match ping(dest, audible) {
        Ok(0) => exit(2), // no replies received
        Ok(_) => exit(0), // replies received
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    }
}

/// `ping` implementation for ipv4
fn ping(dest: String, audible: bool) -> Result<usize, Box<dyn std::error::Error>> {
    let ip: IpAddr = dest.parse()?;
    let pings = Arc::new(Mutex::new(vec![]));

    let protocol = Layer4(Ipv4(IpNextHeaderProtocols::Icmp));
    let (mut tx, mut rx) = transport_channel(4096, protocol)?;

    // spin up the thread that receives the ICMP echo replies
    let pings_recv = pings.clone();
    thread::spawn(move || {
        let mut iter = icmp_packet_iter(&mut rx);
        while let Ok((packet, addr)) = iter.next() {
            if audible {
                emit_bell();
            }

            let echo_reply = EchoReplyPacket::new(packet.packet()).unwrap();
            let sequence_number = echo_reply.get_sequence_number();
            match pings_recv
                .lock()
                .unwrap()
                .get_mut(sequence_number as usize - 1)
            {
                None => eprintln!("received unsollicited ICMP echo response"),
                Some(Echo::Received(_)) => {
                    eprintln!("received duplicate response");
                }
                Some(echo) => {
                    let sent = match *echo {
                        Echo::Sent(sent) => sent,
                        _ => unreachable!(),
                    };
                    let ping = Instant::now().duration_since(sent).as_millis();
                    println!(
                        "{} bytes from {}: icmp_seq={} ttl=.. time={} ms",
                        packet.packet().len(),
                        addr,
                        sequence_number,
                        ping
                    );
                    *echo = Echo::Received(ping);
                }
            }
        }

        eprintln!("thread listener quit unexpectedly");
    });

    // setup CTRL-C handler at this point, not earlier
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    while running.load(Ordering::SeqCst) {
        let mut vec: Vec<u8> = vec![0; 16];
        let mut echo_packet = MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
        echo_packet.set_identifier(0);
        echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

        let sequence_number = {
            let mut pings = pings.lock().unwrap();
            pings.push(Echo::Sent(Instant::now()));
            pings.len() as u16 // start at 1
        };
        echo_packet.set_sequence_number(sequence_number);

        let checksum = util::checksum(echo_packet.packet(), 1);
        echo_packet.set_checksum(checksum);

        tx.send_to(echo_packet, ip)?;

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
