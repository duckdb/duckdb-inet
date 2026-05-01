#pragma once

#ifndef INET_INET_EXTENSION_HPP
#define INET_INET_EXTENSION_HPP
#include "duckdb/stable/exception.hpp"

#include <string>

class TypeCompatibilityException : public duckdb_stable::Exception {
public:
	explicit TypeCompatibilityException(const std::string &msg) : duckdb_stable::Exception("Type Compatibility Exception: " + msg) {}
	template <typename... ARGS>
	explicit TypeCompatibilityException(const std::string &msg, ARGS... params)
		: TypeCompatibilityException(ConstructMessage(msg, params...)) {
	}
};


#endif // INET_INET_EXTENSION_HPP
