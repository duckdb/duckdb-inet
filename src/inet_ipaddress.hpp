#pragma once

#include "duckdb_extension.h"

#include <stdint.h>

// Constants

// Enum for IP address type
typedef enum { INET_IP_ADDRESS_INVALID = 0, INET_IP_ADDRESS_V4 = 1, INET_IP_ADDRESS_V6 = 2 } INET_IPAddressType;

// IPAddress struct
typedef struct {
	INET_IPAddressType type;
	duckdb_uhugeint address;
	uint16_t mask;
} INET_IPAddress;

// Try to parse an IP address from a string.
// If error_message is not NULL, it will be set to an error message on failure.
// The error_message does not need to be freed, it is a static string.
INET_IPAddress ipaddress_from_string(const char *buffer, size_t buffer_size);

// Returns length of string written to buffer. Returns 0 if buffer is too small.
size_t ipaddress_to_string(const INET_IPAddress *ip, char *buffer, size_t buffer_size);

INET_IPAddress ipaddress_netmask(const INET_IPAddress *ip);
INET_IPAddress ipaddress_network(const INET_IPAddress *ip);
INET_IPAddress ipaddress_broadcast(const INET_IPAddress *ip);
