#!/usr/bin/env python3
"""Send or receive ATAK GeoChat CoT packets on a Kaonic-facing network.

Examples:
  python3 cot_test.py listen --interface-ip 192.168.10.2
  python3 cot_test.py send --interface-ip 192.168.10.2 --callsign PI-A \
      --message "Hello from Pi A"
"""

from __future__ import annotations

import argparse
import datetime as datetime
import socket
import sys
import time
import uuid
import xml.etree.ElementTree as ET

GEOCHAT_GROUP = "224.10.10.1"
GEOCHAT_PORT = 17012
MAX_IDENTITY_LENGTH = 128


def ipv4_address(value: str) -> str:
    """Validate an IPv4 address supplied for multicast interface selection."""
    try:
        socket.inet_aton(value)
    except OSError as error:
        raise argparse.ArgumentTypeError(f"invalid IPv4 address: {value}") from error
    return value


def cot_timestamp(value: datetime.datetime) -> str:
    """Format a UTC time in an ATAK/CoT-friendly ISO-8601 form."""
    utc_value = value.astimezone(datetime.timezone.utc)
    return utc_value.strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def validate_identity(name: str, value: str) -> None:
    if not value.strip():
        raise ValueError(f"{name} must not be empty")
    if len(value) > MAX_IDENTITY_LENGTH:
        raise ValueError(f"{name} must not exceed {MAX_IDENTITY_LENGTH} characters")


def make_geochat_packet(callsign: str, sender_uid: str, message: str) -> bytes:
    """Build a valid CoT GeoChat-style event for bridge testing."""
    validate_identity("callsign", callsign)
    validate_identity("uid", sender_uid)
    if not message:
        raise ValueError("message must not be empty")

    now = datetime.datetime.now(datetime.timezone.utc)
    timestamp = cot_timestamp(now)
    stale = cot_timestamp(now + datetime.timedelta(minutes=5))
    message_id = str(uuid.uuid4())

    event = ET.Element(
        "event",
        {
            "version": "2.0",
            "uid": f"GeoChat.{message_id}",
            "type": "b-t-f",
            "time": timestamp,
            "start": timestamp,
            "stale": stale,
            "how": "h-g-i-g-o",
        },
    )
    ET.SubElement(
        event,
        "point",
        {
            "lat": "0.0",
            "lon": "0.0",
            "hae": "0.0",
            "ce": "9999999.0",
            "le": "9999999.0",
        },
    )
    detail = ET.SubElement(event, "detail")
    chat = ET.SubElement(
        detail,
        "__chat",
        {
            "parent": "RootContactGroup",
            "groupOwner": "false",
            "messageId": message_id,
            "chatroom": "All Chat Rooms",
            "id": "All Chat Rooms",
            "senderCallsign": callsign,
        },
    )
    ET.SubElement(
        chat,
        "chatgrp",
        {"uid0": sender_uid, "uid1": "All Chat Rooms", "id": "All Chat Rooms"},
    )
    ET.SubElement(detail, "link", {"uid": sender_uid, "type": "a-f-G-U-C", "relation": "p-p"})
    ET.SubElement(detail, "contact", {"callsign": callsign})
    remarks = ET.SubElement(
        detail,
        "remarks",
        {"source": sender_uid, "to": "All Chat Rooms", "time": timestamp},
    )
    remarks.text = message
    return ET.tostring(event, encoding="utf-8")


def multicast_sender(interface_ip: str) -> socket.socket:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(interface_ip))
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
    return sock


def multicast_listener(interface_ip: str, timeout: float | None) -> socket.socket:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("", GEOCHAT_PORT))
    membership = socket.inet_aton(GEOCHAT_GROUP) + socket.inet_aton(interface_ip)
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, membership)
    if timeout is not None:
        sock.settimeout(timeout)
    return sock


def send_message(args: argparse.Namespace) -> int:
    packet = make_geochat_packet(args.callsign, args.uid, args.message)
    destination = (GEOCHAT_GROUP, GEOCHAT_PORT)

    with multicast_sender(args.interface_ip) as sock:
        for sequence in range(1, args.count + 1):
            sock.sendto(packet, destination)
            print(
                f"Sent GeoChat CoT message {sequence}/{args.count} "
                f"from {args.callsign} to {GEOCHAT_GROUP}:{GEOCHAT_PORT}"
            )
            if args.show_xml:
                print(packet.decode("utf-8"))
            if sequence < args.count:
                time.sleep(args.interval)
    return 0


def print_packet(packet: bytes, source: tuple[str, int]) -> None:
    print(f"\nReceived {len(packet)} bytes from {source[0]}:{source[1]}")
    try:
        root = ET.fromstring(packet)
    except ET.ParseError as error:
        print(f"XML parse error: {error}")
        print(packet.decode("utf-8", errors="replace"))
        return

    chat = root.find(".//__chat")
    contact = root.find(".//contact")
    remarks = root.find(".//remarks")
    sender = None
    if chat is not None:
        sender = chat.get("senderCallsign")
    if not sender and contact is not None:
        sender = contact.get("callsign")

    print(f"UID:      {root.get('uid', '(missing)')}")
    print(f"Type:     {root.get('type', '(missing)')}")
    print(f"Time:     {root.get('time', '(missing)')}")
    print(f"Sender:   {sender or '(unknown)'}")
    print(f"Message:  {remarks.text if remarks is not None and remarks.text else '(none)'}")
    print("Raw XML:")
    print(packet.decode("utf-8", errors="replace"))


def listen_for_messages(args: argparse.Namespace) -> int:
    timeout = None if args.timeout == 0 else args.timeout
    print(
        f"Listening for GeoChat CoT on {GEOCHAT_GROUP}:{GEOCHAT_PORT} "
        f"through interface address {args.interface_ip}"
    )
    received = 0
    with multicast_listener(args.interface_ip, timeout) as sock:
        while args.count == 0 or received < args.count:
            try:
                packet, source = sock.recvfrom(65535)
            except socket.timeout:
                print("Timed out waiting for a message.", file=sys.stderr)
                return 1
            print_packet(packet, source)
            received += 1
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Send or listen for valid GeoChat CoT test messages through a Kaonic.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    listen = subparsers.add_parser("listen", help="listen for GeoChat CoT packets")
    listen.add_argument(
        "--interface-ip",
        required=True,
        type=ipv4_address,
        help="Raspberry Pi IPv4 address on the Kaonic-facing network",
    )
    listen.add_argument(
        "--timeout",
        type=float,
        default=0,
        help="seconds to wait for each packet; 0 waits forever (default: 0)",
    )
    listen.add_argument(
        "--count",
        type=int,
        default=0,
        help="exit after this many packets; 0 listens forever (default: 0)",
    )
    listen.set_defaults(func=listen_for_messages)

    send = subparsers.add_parser("send", help="send a valid GeoChat CoT packet")
    send.add_argument(
        "--interface-ip",
        required=True,
        type=ipv4_address,
        help="Raspberry Pi IPv4 address on the Kaonic-facing network",
    )
    send.add_argument("--callsign", default="PI-A", help="display sender callsign")
    send.add_argument("--uid", default=f"PI-{socket.gethostname()}", help="sender UID")
    send.add_argument("--message", required=True, help="GeoChat test message text")
    send.add_argument("--count", type=int, default=1, help="number of packets to send (default: 1)")
    send.add_argument("--interval", type=float, default=1.0, help="seconds between repeated packets")
    send.add_argument("--show-xml", action="store_true", help="print the generated CoT XML")
    send.set_defaults(func=send_message)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if getattr(args, "count", 0) < 0:
        parser.error("--count must be zero or greater")
    if getattr(args, "timeout", 0) < 0:
        parser.error("--timeout must be zero or greater")
    if getattr(args, "interval", 0) < 0:
        parser.error("--interval must be zero or greater")
    try:
        return args.func(args)
    except (OSError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
