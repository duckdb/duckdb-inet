//! IPv4 and IPv6 addresses with a network prefix, the values of the `INET` type.

use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

/// The address family of an [`IpAddress`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    /// The number of bits in an address, which is also the longest prefix.
    pub const fn bits(self) -> u8 {
        match self {
            Family::V4 => 32,
            Family::V6 => 128,
        }
    }

    /// The IP version number: 4 or 6.
    pub const fn version(self) -> u8 {
        match self {
            Family::V4 => 4,
            Family::V6 => 6,
        }
    }

    const fn max_address(self) -> u128 {
        match self {
            Family::V4 => u32::MAX as u128,
            Family::V6 => u128::MAX,
        }
    }
}

/// An IP address with the length of its network prefix, such as `192.168.0.1/24`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IpAddress {
    family: Family,
    address: u128,
    prefix: u8,
}

/// A string that is not a valid IP address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError(&'static str);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ParseError {}

/// An offset that moves an address outside its family's address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetError {
    Overflow,
    Underflow,
    Ipv4OutOfRange { address: IpAddress, offset: u128 },
}

impl fmt::Display for OffsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OffsetError::Overflow => f.write_str("Overflow in addition"),
            OffsetError::Underflow => f.write_str("Overflow in subtraction"),
            OffsetError::Ipv4OutOfRange { address, offset } => {
                write!(f, "Cannot add {offset} to IPv4 Address {}", address.host())
            }
        }
    }
}

impl std::error::Error for OffsetError {}

impl IpAddress {
    /// Return `None` if the address or prefix does not fit the family.
    pub fn new(family: Family, address: u128, prefix: u8) -> Option<Self> {
        (address <= family.max_address() && prefix <= family.bits()).then_some(Self {
            family,
            address,
            prefix,
        })
    }

    pub fn family(&self) -> Family {
        self.family
    }

    pub fn address(&self) -> u128 {
        self.address
    }

    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    /// The same address as a single host, without a network prefix.
    #[must_use]
    pub fn host(self) -> Self {
        self.with_prefix(self.family.bits())
    }

    /// The mask selecting the network bits, such as `255.255.255.0/24`.
    #[must_use]
    pub fn netmask(self) -> Self {
        self.with_address(self.netmask_bits())
    }

    /// The first address of the network, such as `192.168.1.0/24`.
    #[must_use]
    pub fn network(self) -> Self {
        self.with_address(self.address & self.netmask_bits())
    }

    /// The last address of the network, such as `192.168.1.255/24`.
    #[must_use]
    pub fn broadcast(self) -> Self {
        let host_bits = self.family.max_address() ^ self.netmask_bits();
        self.with_address(self.network().address | host_bits)
    }

    /// Whether every address of this network lies within `other`.
    #[must_use]
    pub fn is_subnet_of(self, other: Self) -> bool {
        other.network().address <= self.network().address
            && self.broadcast().address <= other.broadcast().address
    }

    /// Move the address up by `offset`, keeping the prefix.
    pub fn checked_add(self, offset: i128) -> Result<Self, OffsetError> {
        self.offset(offset >= 0, offset.unsigned_abs())
    }

    /// Move the address down by `offset`, keeping the prefix.
    pub fn checked_sub(self, offset: i128) -> Result<Self, OffsetError> {
        self.offset(offset < 0, offset.unsigned_abs())
    }

    fn offset(self, up: bool, distance: u128) -> Result<Self, OffsetError> {
        let address = if up {
            self.address
                .checked_add(distance)
                .ok_or(OffsetError::Overflow)?
        } else {
            self.address
                .checked_sub(distance)
                .ok_or(OffsetError::Underflow)?
        };
        if address > self.family.max_address() {
            return Err(OffsetError::Ipv4OutOfRange {
                address: self,
                offset: distance,
            });
        }
        Ok(self.with_address(address))
    }

    fn netmask_bits(&self) -> u128 {
        let all = self.family.max_address();
        all ^ all.checked_shr(u32::from(self.prefix)).unwrap_or(0)
    }

    fn with_address(self, address: u128) -> Self {
        Self { address, ..self }
    }

    fn with_prefix(self, prefix: u8) -> Self {
        Self { prefix, ..self }
    }
}

impl FromStr for IpAddress {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, ParseError> {
        let bytes = text.as_bytes();
        match bytes.iter().position(|byte| !byte.is_ascii_hexdigit()) {
            Some(index) if bytes[index] == b':' => parse_ipv6(bytes),
            Some(0) => Err(ParseError("Expected a number")),
            Some(index) if bytes[index] == b'.' => parse_ipv4(bytes),
            _ => Err(ParseError("Expected an IP address")),
        }
    }
}

fn split_while(bytes: &[u8], predicate: impl Fn(&u8) -> bool) -> (&[u8], &[u8]) {
    bytes.split_at(
        bytes
            .iter()
            .position(|byte| !predicate(byte))
            .unwrap_or(bytes.len()),
    )
}

fn parse_decimal(digits: &[u8], max: u8) -> Option<u8> {
    std::str::from_utf8(digits)
        .ok()?
        .parse()
        .ok()
        .filter(|value| *value <= max)
}

/// Parse the optional `/<prefix>` that ends an address.
fn parse_prefix(rest: &[u8], family: Family, missing_slash: ParseError) -> Result<u8, ParseError> {
    let prefix = match rest {
        [] => return Ok(family.bits()),
        [b'/', prefix @ ..] => prefix,
        _ => return Err(missing_slash),
    };
    let invalid = match family {
        Family::V4 => ParseError("Expected a number between 0 and 32"),
        Family::V6 => ParseError("Expected a number between 0 and 128"),
    };
    let (digits, rest) = split_while(prefix, u8::is_ascii_digit);
    let prefix = parse_decimal(digits, family.bits()).ok_or(invalid)?;
    if !rest.is_empty() {
        return Err(ParseError("Unexpected extra characters"));
    }
    Ok(prefix)
}

fn parse_ipv4(text: &[u8]) -> Result<IpAddress, ParseError> {
    let mut rest = text;
    let mut address = 0u32;
    for octet in 0..4 {
        if octet > 0 {
            rest = rest
                .strip_prefix(b".")
                .ok_or(ParseError("Expected a dot"))?;
        }
        let (digits, after) = split_while(rest, u8::is_ascii_digit);
        if digits.is_empty() {
            return Err(ParseError("Expected a number"));
        }
        let value = parse_decimal(digits, u8::MAX)
            .ok_or(ParseError("Expected a number between 0 and 255"))?;
        address = address << 8 | u32::from(value);
        rest = after;
    }
    let prefix = parse_prefix(rest, Family::V4, ParseError("Expected a slash"))?;
    Ok(IpAddress {
        family: Family::V4,
        address: address.into(),
        prefix,
    })
}

const IPV6_GROUPS: usize = 8;

fn parse_ipv6(text: &[u8]) -> Result<IpAddress, ParseError> {
    let mut groups = [0u16; IPV6_GROUPS];
    let mut count = 0;
    // The number of groups parsed before `::`.
    let mut gap = None;
    let mut rest = text;

    while count < IPV6_GROUPS && !matches!(rest, [] | [b'/', ..]) {
        let (hex, after) = split_while(rest, u8::is_ascii_hexdigit);
        if hex.len() > 4 {
            return Err(ParseError("Expected 4 or fewer hex digits"));
        }
        if after.first() == Some(&b'.') {
            let (ipv4, after) = split_while(rest, |byte| byte.is_ascii_digit() || *byte == b'.');
            if !matches!(after, [] | [b'/', ..]) || count + 2 > IPV6_GROUPS {
                return Err(ParseError(
                    "IPv4 format can only be used for the final 2 quibbles.",
                ));
            }
            let ipv4 = parse_ipv4(ipv4)?.address;
            groups[count] = (ipv4 >> 16) as u16;
            groups[count + 1] = ipv4 as u16;
            count += 2;
            rest = after;
            continue;
        }
        if !matches!(after, [] | [b':' | b'/', ..]) {
            return Err(ParseError("Unexpected character found"));
        }
        if !hex.is_empty() {
            groups[count] = hex.iter().fold(0, |group, digit| {
                group << 4 | (*digit as char).to_digit(16).unwrap() as u16
            });
            count += 1;
        }
        rest = after;
        if rest.starts_with(b"::") {
            if gap.is_some() {
                return Err(ParseError("Encountered more than one double-colon"));
            }
            if rest.starts_with(b":::") {
                return Err(ParseError("Encountered more than two consecutive colons"));
            }
            gap = Some(count);
            rest = &rest[1..];
        }
        if let [b':', after @ ..] = rest {
            rest = after;
        }
    }

    // Text other than a prefix only remains after eight groups.
    let prefix = parse_prefix(rest, Family::V6, ParseError("Unexpected extra characters"))?;
    if count < IPV6_GROUPS && gap.is_none() {
        return Err(ParseError("Expected 8 sets of 4 hex digits."));
    }

    let missing = IPV6_GROUPS - count;
    if gap.is_some() && missing == 0 {
        return Err(ParseError("Invalid double-colon, too many hex digits."));
    }
    let address = groups[..count]
        .iter()
        .enumerate()
        .fold(0u128, |address, (index, group)| {
            let position = if gap.is_some_and(|gap| index >= gap) {
                index + missing
            } else {
                index
            };
            address | u128::from(*group) << (16 * (IPV6_GROUPS - 1 - position))
        });
    Ok(IpAddress {
        family: Family::V6,
        address,
        prefix,
    })
}

impl fmt::Display for IpAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.family {
            Family::V4 => write!(f, "{}", Ipv4Addr::from(self.address as u32))?,
            Family::V6 => write_ipv6(f, self.address)?,
        }
        if self.prefix != self.family.bits() {
            write!(f, "/{}", self.prefix)?;
        }
        Ok(())
    }
}

/// Write lowercase hex groups, compressing the leftmost longest run of two or more zero groups.
/// IPv4-compatible, -mapped and -translated addresses print their last 32 bits as IPv4.
fn write_ipv6(f: &mut fmt::Formatter<'_>, address: u128) -> fmt::Result {
    let groups: [u16; IPV6_GROUPS] =
        std::array::from_fn(|index| (address >> (16 * (IPV6_GROUPS - 1 - index))) as u16);

    let mut zeros = 0..0;
    let mut start = 0;
    while start < IPV6_GROUPS {
        let len = groups[start..]
            .iter()
            .take_while(|group| **group == 0)
            .count();
        if len > 1 && len > zeros.len() {
            zeros = start..start + len;
        }
        start += len.max(1);
    }

    let embeds_ipv4 = zeros.start == 0
        && match zeros.end {
            // IPv4-compatible (deprecated); a final group of 1 stays hex.
            6 => groups[7] != 1,
            // IPv4-mapped.
            5 => groups[5] == 0xffff,
            // IPv4-translated.
            4 => groups[4] == 0xffff && groups[5] == 0,
            _ => false,
        };
    let hex_groups = if embeds_ipv4 { 6 } else { IPV6_GROUPS };

    let mut index = 0;
    while index < hex_groups {
        if index == zeros.start && !zeros.is_empty() {
            f.write_str("::")?;
            index = zeros.end;
            continue;
        }
        if index > 0 && index != zeros.end {
            f.write_str(":")?;
        }
        write!(f, "{:x}", groups[index])?;
        index += 1;
    }
    if embeds_ipv4 {
        if hex_groups != zeros.end {
            f.write_str(":")?;
        }
        write!(f, "{}", Ipv4Addr::from(address as u32))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> IpAddress {
        text.parse()
            .unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    fn roundtrip(text: &str) -> String {
        parse(text).to_string()
    }

    fn parse_error(text: &str) -> &'static str {
        text.parse::<IpAddress>().expect_err(text).0
    }

    #[test]
    fn parses_ipv4() {
        assert_eq!(
            parse("127.0.0.1"),
            IpAddress::new(Family::V4, 0x7f00_0001, 32).unwrap()
        );
        assert_eq!(roundtrip("127.000.0000.1"), "127.0.0.1");
        assert_eq!(roundtrip("127.0.0.1/17"), "127.0.0.1/17");
        assert_eq!(roundtrip("0.0.0.0/0"), "0.0.0.0/0");
    }

    #[test]
    fn rejects_invalid_ipv4() {
        assert_eq!(parse_error("127"), "Expected an IP address");
        assert_eq!(parse_error("127..1"), "Expected a number");
        assert_eq!(parse_error("-1.0.0.1"), "Expected a number");
        assert_eq!(parse_error("1.2.3"), "Expected a dot");
        assert_eq!(
            parse_error("256.0.0.1"),
            "Expected a number between 0 and 255"
        );
        assert_eq!(
            parse_error("127.0.0.1/33"),
            "Expected a number between 0 and 32"
        );
        assert_eq!(
            parse_error("127.0.0.1/"),
            "Expected a number between 0 and 32"
        );
        assert_eq!(
            parse_error("127.0.0.1/4294967320"),
            "Expected a number between 0 and 32"
        );
        assert_eq!(parse_error("1.2.3.4#34"), "Expected a slash");
        assert_eq!(parse_error("1.2.3.4/24abc"), "Unexpected extra characters");
        assert_eq!(parse_error("3,786"), "Expected an IP address");
        assert_eq!(parse_error(""), "Expected an IP address");
    }

    #[test]
    fn parses_ipv6() {
        assert_eq!(
            roundtrip("2001:DB8:100:0:C59B:CEF3:A0D1:6DE4"),
            "2001:db8:100:0:c59b:cef3:a0d1:6de4"
        );
        assert_eq!(
            roundtrip("2001:DB8:100::C59B:CEF3:A0D1:6DE4"),
            "2001:db8:100:0:c59b:cef3:a0d1:6de4"
        );
        assert_eq!(
            roundtrip("2001:db8:0:0:0:cef3:35:363/96"),
            "2001:db8::cef3:35:363/96"
        );
        assert_eq!(roundtrip("0:0:0:0:0:0:0:1/128"), "::1");
        assert_eq!(roundtrip("0:0:0:0:0:0:0:0"), "::");
        assert_eq!(roundtrip("::/0"), "::/0");
        assert_eq!(roundtrip("2001:db8:0:0:0:0:0:0"), "2001:db8::");
        assert_eq!(roundtrip("1:0:0:2:0:0:0:3"), "1:0:0:2::3");
        assert_eq!(roundtrip("1:0:0:2:3:0:0:4"), "1::2:3:0:0:4");
        assert_eq!(roundtrip("1:2:3:4:5:6:7:0"), "1:2:3:4:5:6:7:0");
        assert_eq!(
            roundtrip("2001:db8:100:0:c59b:cef3:160.209.109.228"),
            "2001:db8:100:0:c59b:cef3:a0d1:6de4"
        );
        assert_eq!(
            roundtrip("1:2:3:4:5:6:1.2.3.4/64"),
            "1:2:3:4:5:6:102:304/64"
        );
    }

    #[test]
    fn formats_embedded_ipv4() {
        assert_eq!(roundtrip("64:ff9b::192.0.2.128/96"), "64:ff9b::c000:280/96");
        assert_eq!(roundtrip("::c000:280/96"), "::192.0.2.128/96");
        assert_eq!(roundtrip("::ffff:c000:280/96"), "::ffff:192.0.2.128/96");
        assert_eq!(roundtrip("::ffff:0:c000:280/96"), "::ffff:0:192.0.2.128/96");
        assert_eq!(roundtrip("::1234:5678:9abc"), "::1234:5678:9abc");
    }

    #[test]
    fn rejects_invalid_ipv6() {
        assert_eq!(
            parse_error("2001:db8:100:0:c59b:cef3:a0d189:6de4"),
            "Expected 4 or fewer hex digits"
        );
        assert_eq!(
            parse_error("2001:db8:100:0:c59b:cef3:a0d1:6de4:d16d"),
            "Unexpected extra characters"
        );
        assert_eq!(
            parse_error("2001:db8:100:0:c59b:cef3:a0d1"),
            "Expected 8 sets of 4 hex digits."
        );
        assert_eq!(parse_error(":2:"), "Expected 8 sets of 4 hex digits.");
        assert_eq!(
            parse_error("2001:db8::cef3::363/96"),
            "Encountered more than one double-colon"
        );
        assert_eq!(
            parse_error(":::1234:5678"),
            "Encountered more than two consecutive colons"
        );
        assert_eq!(
            parse_error("2001:db8:100:0:c59b:cef3::/130"),
            "Expected a number between 0 and 128"
        );
        assert_eq!(
            parse_error("2001:db8:100:0z:c59b:cef3::/120"),
            "Unexpected character found"
        );
        assert_eq!(parse_error("::/64x"), "Unexpected extra characters");
        assert_eq!(
            parse_error("64:ff9b::192.0.2.128:1:3/96"),
            "IPv4 format can only be used for the final 2 quibbles."
        );
        assert_eq!(
            parse_error("a:b:c:d:e:f:a:1.2.3.4"),
            "IPv4 format can only be used for the final 2 quibbles."
        );
        assert_eq!(
            parse_error("::ffff:0:192.0.268.128/96"),
            "Expected a number between 0 and 255"
        );
        assert_eq!(
            parse_error("2001:DB8:100:30::C59B:CEF3:A0D1:6DE4"),
            "Invalid double-colon, too many hex digits."
        );
        assert_eq!(
            parse_error("1:2:3:4:5:6:7:8::"),
            "Invalid double-colon, too many hex digits."
        );
        assert_eq!(
            parse_error("gggg:gggg:gggg:gggg:gggg:gggg:gggg:gggg/128"),
            "Expected a number"
        );
    }

    #[test]
    fn computes_networks() {
        assert_eq!(
            parse("192.168.1.5/24").netmask().to_string(),
            "255.255.255.0/24"
        );
        assert_eq!(parse("192.168.1.5/0").netmask().to_string(), "0.0.0.0/0");
        assert_eq!(
            parse("192.168.1.5/32").netmask().to_string(),
            "255.255.255.255"
        );
        assert_eq!(
            parse("192.168.1.5/24").network().to_string(),
            "192.168.1.0/24"
        );
        assert_eq!(
            parse("192.168.1.5/24").broadcast().to_string(),
            "192.168.1.255/24"
        );
        assert_eq!(
            parse("10.0.0.1/0").broadcast().to_string(),
            "255.255.255.255/0"
        );
        assert_eq!(
            parse("2001:db8:3c4d::/48").netmask().to_string(),
            "ffff:ffff:ffff::/48"
        );
        assert_eq!(
            parse("2001:db8:3c4d::/32").network().to_string(),
            "2001:db8::/32"
        );
        assert_eq!(
            parse("2001:db8:3c4d::/48").broadcast().to_string(),
            "2001:db8:3c4d:ffff:ffff:ffff:ffff:ffff/48"
        );
        assert_eq!(parse("::1/127").host().to_string(), "::1");
    }

    #[test]
    fn checks_containment() {
        assert!(parse("192.168.1.5/32").is_subnet_of(parse("192.168.1.0/24")));
        assert!(parse("10.1.3.248/30").is_subnet_of(parse("10.1.2.0/23")));
        assert!(parse("0.0.0.0/0").is_subnet_of(parse("192.168.1.0/0")));
        assert!(!parse("127.0.0.1/8").is_subnet_of(parse("127.0.0.1")));
        assert!(parse("2001:db8:0:1::/64").is_subnet_of(parse("2001:db8::/32")));
        assert!(!parse("2001:db8::/16").is_subnet_of(parse("2001:db8::/32")));
    }

    #[test]
    fn applies_offsets() {
        let address = parse("127.0.0.31");
        assert_eq!(
            address.checked_sub(32).unwrap().to_string(),
            "126.255.255.255"
        );
        assert_eq!(address.checked_sub(-32).unwrap().to_string(), "127.0.0.63");
        assert_eq!(
            parse("0.0.0.0").checked_sub(32),
            Err(OffsetError::Underflow)
        );
        assert_eq!(
            parse("255.255.255.255")
                .checked_add(1)
                .unwrap_err()
                .to_string(),
            "Cannot add 1 to IPv4 Address 255.255.255.255"
        );
        assert!(parse("0.0.0.0").checked_add(1 << 64).is_err());
        assert_eq!(parse("::").checked_sub(32), Err(OffsetError::Underflow));
        assert_eq!(
            parse("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff").checked_add(32),
            Err(OffsetError::Overflow)
        );
        assert_eq!(
            parse("::").checked_sub(i128::MIN).unwrap().to_string(),
            "8000::"
        );
        assert_eq!(
            parse("8000::").checked_add(i128::MIN).unwrap().to_string(),
            "::"
        );
        assert!(parse("0.0.0.0").checked_sub(i128::MIN).is_err());
        assert_eq!(parse("0.0.0.0").checked_sub(0), Ok(parse("0.0.0.0")));
    }
}
