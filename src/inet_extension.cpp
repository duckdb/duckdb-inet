#include "duckdb_extension.h"
#include "inet_html.hpp"
#include "inet_ipaddress.hpp"

#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <vector>
#include <memory>

#include "duckdb/duckdb_stable.hpp"

// Forward declare vtable
DUCKDB_EXTENSION_EXTERN

using namespace duckdb_stable;

//----------------------------------------------------------------------------------------------------------------------
// INET TYPE DEFINITION
//----------------------------------------------------------------------------------------------------------------------
using INET_EXECUTOR_TYPE = StructTypeTernary<PrimitiveType<uint8_t>, PrimitiveType<duckdb_hugeint>, PrimitiveType<uint16_t>>;
using INET_T = INET_EXECUTOR_TYPE::ARG_TYPE;

static LogicalType make_inet_type() {
	const char *child_names[] = {"ip_type", "address", "mask"};
	std::vector<LogicalType> child_types;
	child_types.push_back(LogicalType::UTINYINT());
	child_types.push_back(LogicalType::HUGEINT());
	child_types.push_back(LogicalType::USMALLINT());

	auto inet_type = LogicalType::STRUCT(child_types.data(), child_names, 3);
	inet_type.SetAlias("INET");
	return inet_type;
}

namespace duckdb_stable {

template<>
LogicalType TemplateToType::Convert<INET_EXECUTOR_TYPE>() {
	return make_inet_type();
}

}

//----------------------------------------------------------------------------------------------------------------------
// CAST FUNCTIONS
//----------------------------------------------------------------------------------------------------------------------
static duckdb_uhugeint from_compatible_address(duckdb_hugeint compat_addr, INET_IPAddressType addr_type) {
	duckdb_uhugeint retval;
	memcpy(&retval, &compat_addr, sizeof(duckdb_uhugeint));
	// Only flip the bit for order on IPv6 addresses. It can never be set in IPv4
	if (addr_type == INET_IP_ADDRESS_V6) {
		// The top bit is flipped when storing as the signed hugeint so that sorting
		// works correctly. Flip it back here to have a proper unsigned value.
		retval.upper ^= (((uint64_t)(1)) << 63);
	}
	return retval;
}

static duckdb_hugeint to_compatible_address(duckdb_uhugeint new_addr, INET_IPAddressType addr_type) {
	if (addr_type == INET_IP_ADDRESS_V6) {
		// Flip the top bit when storing as a signed hugeint_t so that sorting
		// works correctly.
		new_addr.upper ^= (((uint64_t)(1)) << 63);
	}
	// Don't need to flip the bit for IPv4, and the original IPv4 only
	// implementation didn't do the flipping, so maintain compatibility.
	duckdb_hugeint retval;
	memcpy(&retval, &new_addr, sizeof(duckdb_hugeint));
	return retval;
}

//----------------------------------------------------------------------------------------------------------------------
// HTML ESCAPE
//----------------------------------------------------------------------------------------------------------------------
struct HTMLEscapeBuffer {
	std::unique_ptr<char[]> buffer;
	idx_t size = 0;

	char *GetData() {
		return buffer.get();
	}
	void Allocate(idx_t new_size) {
		if (new_size <= size) {
			// already have enough space
			return;
		}
		buffer = std::unique_ptr<char[]>(new char[new_size]);
		size = new_size;
	}
};

static string_t escape_html(string_t input, bool input_quote, HTMLEscapeBuffer &buffer) {
	auto input_data = input.GetData();
	auto input_size = input.GetSize();

	const idx_t QUOTE_SZ = 1;
	const idx_t AMPERSAND_SZ = 5;
	const idx_t ANGLE_BRACKET_SZ = 4;
	const idx_t TRANSLATED_QUOTE_SZ = 6; // e.g. \" is translated to &quot;, \' is translated to &#x27;

	size_t result_size = 0;
	for (idx_t j = 0; j < input_size; j++) {
		switch (input_data[j]) {
		case '&':
			result_size += AMPERSAND_SZ;
			break;
		case '<':
		case '>':
			result_size += ANGLE_BRACKET_SZ;
			break;
		case '\"':
		case '\'':
			result_size += input_quote ? TRANSLATED_QUOTE_SZ : QUOTE_SZ;
			break;
		default:
			result_size++;
		}
	}

	buffer.Allocate(result_size);
	auto result_data = buffer.GetData();

	size_t pos = 0;
	for (idx_t j = 0; j < input_size; j++) {
		switch (input_data[j]) {
		case '&':
			memcpy(result_data + pos, "&amp;", AMPERSAND_SZ);
			pos += AMPERSAND_SZ;
			break;
		case '<':
			memcpy(result_data + pos, "&lt;", ANGLE_BRACKET_SZ);
			pos += ANGLE_BRACKET_SZ;
			break;
		case '>':
			memcpy(result_data + pos, "&gt;", ANGLE_BRACKET_SZ);
			pos += ANGLE_BRACKET_SZ;
			break;
		case '"':
			if (input_quote) {
				memcpy(result_data + pos, "&quot;", TRANSLATED_QUOTE_SZ);
				pos += TRANSLATED_QUOTE_SZ;
			} else {
				result_data[pos++] = input_data[j];
			}
			break;
		case '\'':
			if (input_quote) {
				memcpy(result_data + pos, "&#x27;", TRANSLATED_QUOTE_SZ);
				pos += TRANSLATED_QUOTE_SZ;
			} else {
				result_data[pos++] = input_data[j];
			}
			break;
		default:
			result_data[pos++] = input_data[j];
		}
	}

	return string_t(result_data, result_size);
}

struct StringBuffer {
	char buffer[256];
};

class INetToVarcharCast : public StandardCastFunctionExt<INetToVarcharCast, INET_EXECUTOR_TYPE, PrimitiveType<string_t>, StringBuffer>  {
public:
	int64_t ImplicitCastCost() override {
		return -1;
	}

	static TARGET_TYPE::ARG_TYPE Cast(const SOURCE_TYPE::ARG_TYPE &input, STATIC_DATA &data) {
		auto &buffer = data.buffer;
		INET_IPAddress inet;
		inet.type = (INET_IPAddressType)input.a_val;
		inet.address = from_compatible_address(input.b_val, inet.type);
		inet.mask = input.c_val;

		size_t written = ipaddress_to_string(&inet, buffer, sizeof(buffer));
		return string_t(buffer, written);
	}
};

class VarcharToINetCast : public StandardCastFunction<VarcharToINetCast, PrimitiveType<string_t>, INET_EXECUTOR_TYPE> {
public:
	int64_t ImplicitCastCost() override {
		return -1;
	}

	static TARGET_TYPE::ARG_TYPE Cast(const SOURCE_TYPE::ARG_TYPE &input) {
		auto data = input.GetData();
		auto size = input.GetSize();

		INET_IPAddress inet = ipaddress_from_string(data, size);

		TARGET_TYPE::ARG_TYPE result;
		result.a_val = (uint8_t)inet.type;
		result.b_val = to_compatible_address(inet.address, inet.type);
		result.c_val = inet.mask;
		return result;
	}
};

class HostFunction : public UnaryFunctionExt<HostFunction, INET_EXECUTOR_TYPE, PrimitiveType<string_t>, StringBuffer> {
public:
	const char *Name() const override {
		return "host";
	}

	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input, STATIC_DATA &data) {
		auto &buffer = data.buffer;
		INET_IPAddress inet;
		inet.type = (INET_IPAddressType)input.a_val;
		inet.address = from_compatible_address(input.b_val, inet.type);
		inet.mask = inet.type == INET_IP_ADDRESS_V4 ? 32 : 128;

		size_t len = ipaddress_to_string(&inet, buffer, sizeof(buffer));

		if (len == 0) {
			throw std::runtime_error("Could not write inet string");
		}
		if (len >= sizeof(buffer)) {
			throw std::runtime_error("Could not write string");
		}
		return string_t(buffer, len);
	}
};

class FamilyFunction : public UnaryFunction<FamilyFunction, INET_EXECUTOR_TYPE, PrimitiveType<uint8_t>> {
public:
	const char *Name() const override {
		return "family";
	}

	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input) {
		switch ((INET_IPAddressType)input.a_val) {
		case INET_IP_ADDRESS_V4:
			return 4;
		break;
		case INET_IP_ADDRESS_V6:
			return 6;
		default:
			throw std::runtime_error("Invalid IP address type");
		}
	}
};

class NetmaskFunction : public UnaryFunction<NetmaskFunction, INET_EXECUTOR_TYPE, INET_EXECUTOR_TYPE> {
public:
	const char *Name() const override {
		return "netmask";
	}
	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input) {
		INET_IPAddress old_inet = {};
		old_inet.type = (INET_IPAddressType)input.a_val;
		old_inet.address = from_compatible_address(input.b_val, old_inet.type);
		old_inet.mask = input.c_val;

		// Apply the function
		INET_IPAddress new_inet = ipaddress_netmask(&old_inet);

		RESULT_TYPE::ARG_TYPE result;
		result.a_val = (uint8_t)new_inet.type;
		result.b_val = to_compatible_address(new_inet.address, new_inet.type);
		result.c_val = new_inet.mask;
		return result;
	}
};

class NetworkFunction : public UnaryFunction<NetworkFunction, INET_EXECUTOR_TYPE, INET_EXECUTOR_TYPE> {
public:
	const char *Name() const override {
		return "network";
	}
	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input) {
		INET_IPAddress old_inet = {};
		old_inet.type = (INET_IPAddressType)input.a_val;
		old_inet.address = from_compatible_address(input.b_val, old_inet.type);
		old_inet.mask = input.c_val;

		// Apply the function
		INET_IPAddress new_inet = ipaddress_network(&old_inet);

		RESULT_TYPE::ARG_TYPE result;
		result.a_val = (uint8_t)new_inet.type;
		result.b_val = to_compatible_address(new_inet.address, new_inet.type);
		result.c_val = new_inet.mask;
		return result;
	}
};

class BroadcastFunction : public UnaryFunction<BroadcastFunction, INET_EXECUTOR_TYPE, INET_EXECUTOR_TYPE> {
public:
	const char *Name() const override {
		return "broadcast";
	}
	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input) {
		INET_IPAddress old_inet = {};
		old_inet.type = (INET_IPAddressType)input.a_val;
		old_inet.address = from_compatible_address(input.b_val, old_inet.type);
		old_inet.mask = input.c_val;

		// Apply the function
		INET_IPAddress new_inet = ipaddress_broadcast(&old_inet);

		RESULT_TYPE::ARG_TYPE result;
		result.a_val = (uint8_t)new_inet.type;
		result.b_val = to_compatible_address(new_inet.address, new_inet.type);
		result.c_val = new_inet.mask;
		return result;
	}
};

namespace duckdb_stable {

template<>
FormatValue FormatValue::CreateFormatValue(INET_T input) {
	INET_IPAddress inet;
	inet.type = (INET_IPAddressType)input.a_val;
	inet.address = from_compatible_address(input.b_val, inet.type);
	inet.mask = inet.type == INET_IP_ADDRESS_V4 ? 32 : 128;

	char buffer[256];
	size_t len = ipaddress_to_string(&inet, buffer, sizeof(buffer));
	return FormatValue(std::string(buffer, len));
}

}

static INET_T AddImplementation(const INET_T &lhs, const hugeint_t &rhs) {
	if (rhs == 0) {
		return lhs;
	}

	INET_EXECUTOR_TYPE result;
	auto addr_type = (INET_IPAddressType) lhs.a_val;
	uhugeint_t address_in = from_compatible_address(lhs.b_val, addr_type);
	uhugeint_t address_out;

	if (rhs > 0) {
		auto rhs_val = uhugeint_t::from_hugeint(rhs.c_val());
		address_out = address_in.add(rhs_val);
	} else {
		auto rhs_val = uhugeint_t::from_hugeint(rhs.negate().c_val());
		address_out = address_in.subtract(rhs_val);
	}
	if (lhs.a_val == INET_IP_ADDRESS_V4) {
		// Check if overflow ipv4
		if (address_out.lower() >= 0xffffffff) {
			throw OutOfRangeException("Cannot add {} to IPv4 Address {}", rhs, lhs);
		}
	}

	result.a_val = lhs.a_val;
	result.b_val = to_compatible_address(address_out.c_val(), addr_type);
	result.c_val = lhs.c_val;
	return result;

}

class AddFunction : public BinaryFunction<AddFunction, INET_EXECUTOR_TYPE, PrimitiveType<hugeint_t>, INET_EXECUTOR_TYPE> {
public:
	const char *Name() const override {
		return "+";
	}
	static RESULT_TYPE::ARG_TYPE Operation(const A_TYPE::ARG_TYPE &lhs, const B_TYPE::ARG_TYPE &rhs) {
		return AddImplementation(lhs, rhs);
	}
};

class SubtractFunction : public BinaryFunction<SubtractFunction, INET_EXECUTOR_TYPE, PrimitiveType<hugeint_t>, INET_EXECUTOR_TYPE> {
public:
	const char *Name() const override {
		return "-";
	}
	static RESULT_TYPE::ARG_TYPE Operation(const A_TYPE::ARG_TYPE &lhs, const B_TYPE::ARG_TYPE &rhs) {
		return AddImplementation(lhs, rhs.negate());
	}
};

static bool ContainsImplementation(const INET_T &lhs, const INET_T &rhs) {
	INET_IPAddress lhs_inet;
	lhs_inet.type = (INET_IPAddressType)lhs.a_val;
	lhs_inet.address = from_compatible_address(lhs.b_val, lhs_inet.type);
	lhs_inet.mask = lhs.c_val;

	INET_IPAddress rhs_inet;
	rhs_inet.type = (INET_IPAddressType)rhs.a_val;
	rhs_inet.address = from_compatible_address(rhs.b_val, rhs_inet.type);
	rhs_inet.mask = rhs.c_val;

	INET_IPAddress lhs_network = ipaddress_network(&lhs_inet);
	INET_IPAddress lhs_broadcast = ipaddress_broadcast(&lhs_inet);

	INET_IPAddress rhs_network = ipaddress_network(&rhs_inet);
	INET_IPAddress rhs_broadcast = ipaddress_broadcast(&rhs_inet);

	// Set the output
	const bool network_in_lower = lhs_network.address.lower >= rhs_network.address.lower;
	const bool network_in_upper = lhs_network.address.upper >= rhs_network.address.upper;
	const bool broadcast_in_lower = lhs_broadcast.address.lower <= rhs_broadcast.address.lower;
	const bool broadcast_in_upper = lhs_broadcast.address.upper <= rhs_broadcast.address.upper;

	return network_in_lower && network_in_upper && broadcast_in_lower && broadcast_in_upper;
}

class ContainsLeftBaseFunction : public BinaryFunction<ContainsLeftBaseFunction, INET_EXECUTOR_TYPE, INET_EXECUTOR_TYPE, PrimitiveType<bool>> {
public:
	static RESULT_TYPE::ARG_TYPE Operation(const A_TYPE::ARG_TYPE &lhs, const B_TYPE::ARG_TYPE &rhs) {
		return ContainsImplementation(lhs, rhs);
	}
};

class ContainsLeftFunction : public ContainsLeftBaseFunction {
public:
	const char *Name() const override {
		return "<<=";
	}
};

class SubnetContainedByOrEquals : public ContainsLeftBaseFunction {
public:
	const char *Name() const override {
		return "subnet_contained_by_or_equals";
	}
};

class ContainsRightBaseFunction : public BinaryFunction<ContainsRightBaseFunction, INET_EXECUTOR_TYPE, INET_EXECUTOR_TYPE, PrimitiveType<bool>> {
public:
	static RESULT_TYPE::ARG_TYPE Operation(const A_TYPE::ARG_TYPE &lhs, const B_TYPE::ARG_TYPE &rhs) {
		return ContainsImplementation(rhs, lhs);
	}
};

class ContainsRightFunction : public ContainsRightBaseFunction {
public:
	const char *Name() const override {
		return ">>=";
	}
};

class SubnetContainsOrEqualsFunction : public ContainsRightBaseFunction {
public:
	const char *Name() const override {
		return "subnet_contains_or_equals";
	}
};

class HTMLEscapeFunction : public UnaryFunctionExt<HTMLEscapeFunction, PrimitiveType<string_t>, PrimitiveType<string_t>, HTMLEscapeBuffer> {
public:
	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input, HTMLEscapeBuffer &buffer) {
		return escape_html(input, true, buffer);
	}
};

class HTMLEscapeQuoteFunction : public BinaryFunctionExt<HTMLEscapeQuoteFunction, PrimitiveType<string_t>, PrimitiveType<bool>, PrimitiveType<string_t>, HTMLEscapeBuffer> {
public:
	static RESULT_TYPE::ARG_TYPE Operation(const A_TYPE::ARG_TYPE &input, const B_TYPE::ARG_TYPE &input_quote, HTMLEscapeBuffer &buffer) {
		return escape_html(input, input_quote, buffer);
	}
};

class HTMLUnescapeFunction : public UnaryFunctionExt<HTMLUnescapeFunction, PrimitiveType<string_t>, PrimitiveType<string_t>, HTMLEscapeBuffer> {
public:
	const char *Name() const override {
		return "html_unescape";
	}
	static RESULT_TYPE::ARG_TYPE Operation(const INPUT_TYPE::ARG_TYPE &input, HTMLEscapeBuffer &buffer) {
		auto input_data = input.GetData();
		auto input_size = input.GetSize();

		// Compute the result size
		auto result_size = inet_html_unescaped_get_required_size(input_data, input_size);
		buffer.Allocate(result_size);
		auto result_data = buffer.GetData();
		inet_html_unescape(input_data, input_size, result_data, result_size);

		return string_t(result_data, result_size);
	}
};

class HTMLEscapeSet : public ScalarFunctionSet {
public:
	HTMLEscapeSet() : ScalarFunctionSet("html_escape") {
		HTMLEscapeFunction html_escape;
		HTMLEscapeQuoteFunction html_quote_escape;
		AddFunction(html_escape);
		AddFunction(html_quote_escape);
	}
};

//----------------------------------------------------------------------------------------------------------------------
// EXTENSION ENTRY
//----------------------------------------------------------------------------------------------------------------------
DUCKDB_EXTENSION_CPP_ENTRYPOINT(INET) {
	auto inet_type = make_inet_type();
	auto text_type = LogicalType::VARCHAR();
	auto bool_type = LogicalType::BOOLEAN();
	auto utinyint_type = LogicalType::UTINYINT();
	auto hugeint_type = LogicalType::HUGEINT();

	Register(inet_type);

	// Register cast functions
	INetToVarcharCast inet_to_text;
	Register(inet_to_text);

	VarcharToINetCast text_to_inet;
	Register(text_to_inet);

	// scalar functions
	HostFunction host_function;
	Register(host_function);

	FamilyFunction family_function;
	Register(family_function);

	NetmaskFunction netmask_function;
	Register(netmask_function);

	NetworkFunction network_function;
	Register(network_function);

	BroadcastFunction broadcast_function;
	Register(broadcast_function);

	AddFunction add_function;
	Register(add_function);

	SubtractFunction subtract_function;
	Register(subtract_function);

	ContainsLeftFunction contains_left;
	Register(contains_left);

	SubnetContainedByOrEquals subnet_contained_by_or_equals;
	Register(subnet_contained_by_or_equals);

	ContainsRightFunction contains_right;
	Register(contains_right);

	SubnetContainsOrEqualsFunction subnet_contains_or_equals;
	Register(subnet_contains_or_equals);

	HTMLEscapeSet html_escape_set;
	Register(html_escape_set);

	HTMLUnescapeFunction html_unescape;
	Register(html_unescape);
}
