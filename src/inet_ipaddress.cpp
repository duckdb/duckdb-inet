#include "inet_ipaddress.hpp"

#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdexcept>

static const int32_t HEX_BITSIZE = 4;
static const int32_t MAX_QUIBBLE_DIGITS = 4;
static const size_t QUIBBLES_PER_HALF = 4;
static const uint64_t IPV4_NETWORK_MASK = 0xffffffff;
static const duckdb_uhugeint IPV6_NETWORK_MASK = {0xffffffffffffffff, 0xffffffffffffffff};

static const size_t IPV4_DEFAULT_MASK = 32;
static const size_t IPV6_DEFAULT_MASK = 128;
static const size_t IPV6_QUIBBLE_BITS = 16;

#define INET_IPV6_NUM_QUIBBLE (8)

static duckdb_uhugeint uhugeint_shift_right(duckdb_uhugeint lhs, duckdb_uhugeint rhs) {
	duckdb_uhugeint result = lhs;

	const uint64_t shift = rhs.lower;
	if (rhs.upper != 0 || shift >= 128) {
		result.upper = 0;
		result.lower = 0;
		return result;
	}
	if (shift == 0) {
		return result;
	}
	if (shift == 64) {
		result.upper = 0;
		result.lower = lhs.upper;
		return result;
	}
	if (shift < 64) {
		result.upper = lhs.upper >> shift;
		result.lower = (lhs.upper << (64 - shift)) | (lhs.lower >> shift);
		return result;
	}
	if ((128 > shift) && (shift > 64)) {
		result.upper = 0;
		result.lower = lhs.upper >> (shift - 64);
		return result;
	}
	result.lower = 0;
	result.upper = 0;
	return result;
}

static duckdb_uhugeint uhugeint_xor(duckdb_uhugeint lhs, duckdb_uhugeint rhs) {
	duckdb_uhugeint result = lhs;
	result.upper = lhs.upper ^ rhs.upper;
	result.lower = lhs.lower ^ rhs.lower;
	return result;
}

// Helper functions
static int is_hex_char(char c) {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

static int is_digit_char(char c) {
	return c >= '0' && c <= '9';
}

static int hex_char_to_value(char c) {
	if (c >= '0' && c <= '9')
		return c - '0';
	if (c >= 'a' && c <= 'f')
		return c - 'a' + 10;
	if (c >= 'A' && c <= 'F')
		return c - 'A' + 10;
	return 0;
}

static int try_cast_string_to_uint8(const char *str, size_t len, uint8_t *result) {
	if (len == 0)
		return 0;

	uint32_t value = 0;
	for (size_t i = 0; i < len; i++) {
		if (!is_digit_char(str[i]))
			return 0;
		value = value * 10 + (str[i] - '0');
	}

	if (value > 255)
		return 0;
	*result = (uint8_t)value;
	return 1;
}

static void parse_quibble(uint16_t *result, const char *buf, size_t len) {
	*result = 0;
	for (size_t c = 0; c < len; c++) {
		*result = (*result << HEX_BITSIZE) + hex_char_to_value(buf[c]);
	}
}

static size_t quibble_half_address_bit_shift(size_t quibble, int *is_upper) {
	size_t this_offset = quibble % QUIBBLES_PER_HALF;
	size_t quibble_shift = (QUIBBLES_PER_HALF - 1) - this_offset;
	*is_upper = quibble < QUIBBLES_PER_HALF;
	return quibble_shift * IPV6_QUIBBLE_BITS;
}

static INET_IPAddress parse_ipv4(const char *data, size_t size) {
	size_t c = 0;
	size_t number_count = 0;
	uint32_t address = 0;
	size_t start = 0;

	INET_IPAddress result;

	result.type = INET_IP_ADDRESS_V4;

parse_number:

	start = c;

	while (c < size && data[c] >= '0' && data[c] <= '9') {
		c++;
	}
	if (start == c) {
		throw std::runtime_error("Expected a number");
	}
	uint8_t number;
	if (!try_cast_string_to_uint8(data + start, c - start, &number)) {
		throw std::runtime_error("Expected a number between 0 and 255");
	}

	address <<= 8;
	address += number;
	number_count++;
	result.address.upper = 0;
	result.address.lower = address;
	if (number_count == 4) {
		goto parse_mask;
	} else {
		goto parse_dot;
	}
parse_dot:
	if (c == size || data[c] != '.') {
		throw std::runtime_error("Expected a dot");
	}
	c++;
	goto parse_number;
parse_mask:
	if (c == size) {
		// no mask, set to default
		result.mask = IPV4_DEFAULT_MASK;
		return result;
	}
	if (data[c] != '/') {
		throw std::runtime_error("Expected a slash");
	}
	c++;
	start = c;
	while (c < size && data[c] >= '0' && data[c] <= '9') {
		c++;
	}
	uint8_t mask;
	if (!try_cast_string_to_uint8(data + start, c - start, &mask)) {
		throw std::runtime_error("Expected a number between 0 and 32");
	}
	if (mask > 32) {
		throw std::runtime_error("Expected a number between 0 and 32");
	}
	result.mask = mask;
	return result;
}

static INET_IPAddress parse_ipv6(const char *data, size_t size) {
	size_t c = 0;
	int parsed_quibble_count = 0;
	uint16_t quibbles[INET_IPV6_NUM_QUIBBLE] = {};
	int first_quibble_count = -1;

	INET_IPAddress result;

	result.type = INET_IP_ADDRESS_V6;
	result.mask = IPV6_DEFAULT_MASK;

	while (c < size && parsed_quibble_count < INET_IPV6_NUM_QUIBBLE) {
		size_t start = c;
		while (c < size && is_hex_char(data[c])) {
			c++;
		}
		size_t len = c - start;

		if (len > MAX_QUIBBLE_DIGITS) {
			throw std::runtime_error("Expected 4 or fewer hex digits");
		}

		if (c < size && data[c] == '.') {
			// IPv4 dotted decimal form
			c = start;
			while (c < size && (is_digit_char(data[c]) || data[c] == '.')) {
				c++;
			}

			if (c < size && data[c] != '/') {
				throw std::runtime_error("IPv4 format can only be used for the final 2 quibbles.");
			}

			auto ipv4 = parse_ipv4(data + start, c - start);

			quibbles[parsed_quibble_count++] = ipv4.address.lower >> IPV6_QUIBBLE_BITS;
			quibbles[parsed_quibble_count++] = ipv4.address.lower & 0xffff;
			continue;
		}

		if (c < size && data[c] != ':' && data[c] != '/') {
			throw std::runtime_error("Unexpected character found");
		}

		if (len > 0) {
			parse_quibble(&quibbles[parsed_quibble_count++], data + start, len);
		}

		// Check for double colon
		if (c + 1 < size && data[c] == ':' && data[c + 1] == ':') {
			if (first_quibble_count != -1) {
				throw std::runtime_error("Encountered more than one double-colon");
			}
			if (c + 2 < size && data[c + 2] == ':') {
				throw std::runtime_error("Encountered more than two consecutive colons");
			}

			first_quibble_count = parsed_quibble_count;
			c++;
		}

		// Parse mask
		if (c < size && data[c] == '/') {
			start = ++c;
			while (c < size && is_digit_char(data[c])) {
				c++;
			}

			uint8_t mask;
			if (!try_cast_string_to_uint8(data + start, c - start, &mask)) {
				throw std::runtime_error("Expected a number between 0 and 128");
			}
			if (mask > IPV6_DEFAULT_MASK) {
				throw std::runtime_error("Expected a number between 0 and 128");
			}

			result.mask = mask;
			break;
		}
		c++;
	}

	if (parsed_quibble_count < INET_IPV6_NUM_QUIBBLE && first_quibble_count == -1) {
		throw std::runtime_error("Expected 8 sets of 4 hex digits.");
	}

	if (c < size) {
		throw std::runtime_error("Unexpected extra characters");
	}

	result.address.upper = 0;
	result.address.lower = 0;

	size_t output_idx = 0;
	for (int parsed_idx = 0; parsed_idx < parsed_quibble_count; parsed_idx++, output_idx++) {
		if (parsed_idx == first_quibble_count) {
			int missing_quibbles = INET_IPV6_NUM_QUIBBLE - parsed_quibble_count;
			if (missing_quibbles == 0) {
				throw std::runtime_error("Invalid double-colon, too many hex digits.");
			}
			output_idx += missing_quibbles;
		}

		int is_upper;
		size_t bitshift = quibble_half_address_bit_shift(output_idx, &is_upper);
		if (is_upper) {
			result.address.upper |= (uint64_t)quibbles[parsed_idx] << bitshift;
		} else {
			result.address.lower |= (uint64_t)quibbles[parsed_idx] << bitshift;
		}
	}

	return result;
}

// Public function implementations
INET_IPAddress ipaddress_from_ipv4(int32_t address, uint16_t mask) {
	INET_IPAddress result;
	result.type = INET_IP_ADDRESS_V4;
	result.address.upper = 0;
	result.address.lower = address;
	result.mask = mask;
	return result;
}

INET_IPAddress ipaddress_from_ipv6(duckdb_uhugeint address, uint16_t mask) {
	INET_IPAddress result;
	result.type = INET_IP_ADDRESS_V6;
	result.address = address;
	result.mask = mask;
	return result;
}

INET_IPAddress ipaddress_from_string(const char *data, size_t size) {
	size_t c = 0;

	// Detect IPv4 vs IPv6
	while (c < size && is_hex_char(data[c])) {
		c++;
	}

	if (c == size) {
		throw std::runtime_error("Expected an IP address");
	}

	if (data[c] == ':') {
		return parse_ipv6(data, size);
	}

	if (c == 0) {
		throw std::runtime_error("Expected a number");
	}

	if (data[c] == '.') {
		return parse_ipv4(data, size);
	}
	throw std::runtime_error("Expected an IP address");
}

static size_t ipadress_to_string_ipv4(const INET_IPAddress *ip, char *buffer, size_t buffer_size) {
	size_t pos = 0;
	pos += snprintf(buffer + pos, buffer_size - pos, "%u.%u.%u.%u", (uint32_t)((ip->address.lower >> 24) & 0xff),
	                (uint32_t)((ip->address.lower >> 16) & 0xff), (uint32_t)((ip->address.lower >> 8) & 0xff),
	                (uint32_t)(ip->address.lower & 0xff));
	if (ip->mask != IPV4_DEFAULT_MASK) {
		pos += snprintf(buffer + pos, buffer_size - pos, "/%u", ip->mask);
	}
	return pos;
}

static size_t ipaddress_to_string_ipv6(const INET_IPAddress *ip, char *buffer, size_t buffer_size) {
	uint16_t quibbles[INET_IPV6_NUM_QUIBBLE];
	size_t zero_run = 0;
	size_t zero_start = 0;
	// The total number of quibbles can't be a start index, so use it to track
	// when a zero run is not in progress.
	size_t this_zero_start = INET_IPV6_NUM_QUIBBLE;

	// Convert the packed bits into quibbles while looking for the maximum run of
	// zeros
	for (size_t i = 0; i < INET_IPV6_NUM_QUIBBLE; ++i) {
		int is_upper;
		size_t bitshift = quibble_half_address_bit_shift(i, &is_upper);
		// Operate on each half separately to make the bit operations more
		// efficient.
		if (is_upper) {
			quibbles[i] = (uint16_t)((ip->address.upper >> bitshift) & 0xFFFF);
		} else {
			quibbles[i] = (uint16_t)((ip->address.lower >> bitshift) & 0xFFFF);
		}

		if (quibbles[i] == 0 && this_zero_start == INET_IPV6_NUM_QUIBBLE) {
			this_zero_start = i;
		} else if (quibbles[i] != 0 && this_zero_start != INET_IPV6_NUM_QUIBBLE) {
			// This is the end of the current run of zero quibbles
			size_t this_run = i - this_zero_start;
			// Save this run if it is larger than previous runs. If it is equal,
			// the left-most should be used according to the standard, so keep
			// the previous start value. Also per the standard, do not count a
			// single zero quibble as a run.
			if (this_run > 1 && this_run > zero_run) {
				zero_run = this_run;
				zero_start = this_zero_start;
			}
			this_zero_start = INET_IPV6_NUM_QUIBBLE;
		}
	}

	// Handle a zero run through the end of the address
	if (this_zero_start != INET_IPV6_NUM_QUIBBLE) {
		size_t this_run = INET_IPV6_NUM_QUIBBLE - this_zero_start;
		if (this_run > 1 && this_run > zero_run) {
			zero_run = this_run;
			zero_start = this_zero_start;
		}
	}

	size_t zero_end = zero_start + zero_run;
	size_t pos = 0;

	for (size_t i = 0; i < INET_IPV6_NUM_QUIBBLE; ++i) {
		if (i > 0) {
			pos += snprintf(buffer + pos, buffer_size - pos, ":");
		}

		if (i < zero_end && i >= zero_start) {
			// Handle the special case of the run being at the beginning
			if (i == 0) {
				pos += snprintf(buffer + pos, buffer_size - pos, ":");
			}
			// Adjust the index to skip past the zero quibbles
			i = zero_end - 1;

			// Handle the special case of the run being at the end
			if (i == INET_IPV6_NUM_QUIBBLE - 1) {
				pos += snprintf(buffer + pos, buffer_size - pos, ":");
			}
		} else if (
		    // Deprecated IPv4 form with all leading zeros (except handle special
		    // case ::1)
		    (i == 6 && zero_start == 0 && zero_end == 6 && quibbles[7] != 1)
		    // Ipv4-mapped addresses: ::ffff:111.222.33.44
		    || (i == 6 && zero_start == 0 && zero_end == 5 && quibbles[5] == 0xffff)
		    // Ipv4 translated addresses: ::ffff:0:111.222.33.44
		    || (i == 6 && zero_start == 0 && zero_end == 4 && quibbles[4] == 0xffff && quibbles[5] == 0)) {
			// Pass along the lower 2 quibbles, and use the IPv4 default mask to
			// suppress IPv4 formatting from trying to print a mask value
			uint32_t ipv4_addr = (uint32_t)(ip->address.lower & 0xffffffff);
			pos += snprintf(buffer + pos, buffer_size - pos, "%u.%u.%u.%u", (ipv4_addr >> 24) & 0xff,
			                (ipv4_addr >> 16) & 0xff, (ipv4_addr >> 8) & 0xff, ipv4_addr & 0xff);
			break;
		} else {
			pos += snprintf(buffer + pos, buffer_size - pos, "%x", quibbles[i]);
		}
	}

	if (ip->mask != IPV6_DEFAULT_MASK) {
		pos += snprintf(buffer + pos, buffer_size - pos, "/%u", ip->mask);
	}

	return pos;
}

size_t ipaddress_to_string(const INET_IPAddress *ip, char *buffer, size_t buffer_size) {
	switch (ip->type) {
	case INET_IP_ADDRESS_V4:
		return ipadress_to_string_ipv4(ip, buffer, buffer_size);
	case INET_IP_ADDRESS_V6:
		return ipaddress_to_string_ipv6(ip, buffer, buffer_size);
	default:
		// Invalid IP address type
		return 0;
	}
}

INET_IPAddress ipaddress_netmask(const INET_IPAddress *ip) {
	duckdb_uhugeint ipv4_mask = { IPV4_NETWORK_MASK, 0 };
	duckdb_uhugeint ip_shift = { ip->mask, 0 };
	duckdb_uhugeint mask =
	    ip->type == INET_IP_ADDRESS_V4 ? ipv4_mask : IPV6_NETWORK_MASK;
	duckdb_uhugeint shift = uhugeint_shift_right(mask, ip_shift);
	duckdb_uhugeint netmask = uhugeint_xor(mask, shift);

	INET_IPAddress result;
	result.type = ip->type;
	result.address = netmask;
	result.mask = ip->mask;
	return result;
}

INET_IPAddress ipaddress_network(const INET_IPAddress *ip) {
	INET_IPAddress netmask = ipaddress_netmask(ip);
	INET_IPAddress result = {};
	result.type = ip->type;
	result.mask = ip->mask;
	result.address.upper = ip->address.upper & netmask.address.upper;
	result.address.lower = ip->address.lower & netmask.address.lower;
	return result;
}

INET_IPAddress ipaddress_broadcast(const INET_IPAddress *ip) {
	INET_IPAddress network = ipaddress_network(ip);
	INET_IPAddress netmask = ipaddress_netmask(ip);
	INET_IPAddress result = {};
	result.type = ip->type;
	result.mask = ip->mask;
	result.address.upper = network.address.upper | (~netmask.address.upper);
	result.address.lower = network.address.lower | (~netmask.address.lower);
	return result;
}
