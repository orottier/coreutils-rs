//! `ping` - send ICMP ECHO_REQUEST to network hosts
//!
//! Warning, needs to be run with `sudo`
//!
//! Todo:
//!  - actually measure ping and lost packets
//!  - resolve addrs
//!  - support IPv6
//!  - report duplicate or damaged replies
//!  - all other options

use coreutils::{emit_bell, print_help_and_exit};
use std::net::IpAddr;
use std::process::exit;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use pnet::packet::icmp::echo_request::MutableEchoRequestPacket;
use pnet::packet::icmp::IcmpTypes;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::util;
use pnet::packet::Packet;
use pnet::transport::TransportChannelType::Layer4;
use pnet::transport::TransportProtocol::{Ipv4, Ipv6};
use pnet::transport::{icmp_packet_iter, icmpv6_packet_iter, transport_channel};

const USAGE: &str = "ping <dest>: send ICMP ECHO_REQUEST to network hosts";

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
        Ok(_) => exit(0),
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    }
}

/// `ping` implementation
fn ping(dest: String, audible: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ip = IpAddr::from_str(&dest)?;

    let protocol = Layer4(Ipv4(IpNextHeaderProtocols::Icmp));
    let (mut tx, mut rx) = transport_channel(4096, protocol)?;

    thread::spawn(move || {
        let mut iter = icmp_packet_iter(&mut rx);
        while let Ok((packet, addr)) = iter.next() {
            println!("received reply {:?} from {:?}", packet, addr);

            if audible {
                emit_bell();
            }
        }
        eprintln!("thread listener quit unexpectedly");
    });

    let mut sequence_number = 0;
    loop {
        let mut vec: Vec<u8> = vec![0; 16];
        let mut echo_packet = MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
        echo_packet.set_sequence_number(sequence_number);
        echo_packet.set_identifier(0);
        echo_packet.set_icmp_type(IcmpTypes::EchoRequest);

        let checksum = util::checksum(echo_packet.packet(), 1);
        echo_packet.set_checksum(checksum);

        tx.send_to(echo_packet, ip)?;
        println!("sent packet");

        sequence_number += 1;
        thread::sleep(Duration::from_millis(1000));
    }
}
