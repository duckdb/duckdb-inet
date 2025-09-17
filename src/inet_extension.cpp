#include "duckdb.hpp"
#include "duckdb/common/exception.hpp"
#include "duckdb/common/string_util.hpp"
#include "duckdb/common/pair.hpp"
#include "duckdb/main/extension/extension_loader.hpp"
#include "duckdb/function/scalar_function.hpp"
#include "duckdb/parser/parsed_data/create_type_info.hpp"
#include "duckdb/parser/parsed_data/create_scalar_function_info.hpp"
#include "duckdb/catalog/catalog.hpp"
#include "duckdb/main/config.hpp"
#include "inet_extension.hpp"
#include "inet_functions.hpp"

namespace duckdb {

static constexpr auto INET_TYPE_NAME = "INET";

static void LoadInternal(ExtensionLoader &loader) {
  // add the "inet" type
  child_list_t<LogicalType> children;
  children.push_back(make_pair("ip_type", LogicalType::UTINYINT));
  // The address type would ideally be UHUGEINT, but the initial version was
  // HUGEINT so maintain backwards-compatibility with db written with older
  // versions.
  children.push_back(make_pair("address", LogicalType::HUGEINT));
  children.push_back(make_pair("mask", LogicalType::USMALLINT));
  auto inet_type = LogicalType::STRUCT(std::move(children));
  inet_type.SetAlias(INET_TYPE_NAME);
  loader.RegisterType(INET_TYPE_NAME, inet_type);

  // add the casts to and from INET type
  loader.RegisterCastFunction(LogicalType::VARCHAR, inet_type,
    INetFunctions::CastVarcharToINET);
  loader.RegisterCastFunction(inet_type, LogicalType::VARCHAR,
    INetFunctions::CastINETToVarchar);

  // add inet functions
  loader.RegisterFunction(
      ScalarFunction("host", {inet_type}, LogicalType::VARCHAR,
                                   INetFunctions::Host));
  loader.RegisterFunction(
      ScalarFunction("family", {inet_type}, LogicalType::UTINYINT,
                                   INetFunctions::Family));
  loader.RegisterFunction(
      ScalarFunction("netmask", {inet_type}, {inet_type},
                                   INetFunctions::Netmask));
  loader.RegisterFunction(
      ScalarFunction("network", {inet_type}, {inet_type},
                                   INetFunctions::Network));
  loader.RegisterFunction(
      ScalarFunction("broadcast", {inet_type}, {inet_type},
                                   INetFunctions::Broadcast));
  loader.RegisterFunction(InetExtension::GetEscapeFunctionSet());
  loader.RegisterFunction(InetExtension::GetUnescapeFunction());

  // Add - function with ALTER_ON_CONFLICT
  ScalarFunction substract_fun("-", {inet_type, LogicalType::HUGEINT},
                               inet_type, INetFunctions::Subtract);
  loader.AddFunctionOverload(substract_fun);

  ScalarFunction add_fun("+", {inet_type, LogicalType::HUGEINT}, inet_type,
                         INetFunctions::Add);
  loader.AddFunctionOverload(add_fun);

  // Add IP range functions
  loader.RegisterFunction(
      ScalarFunction("subnet_contained_by_or_equals", {inet_type, inet_type},
                                                       LogicalType::BOOLEAN,
                                                       INetFunctions::ContainsLeft));
  loader.RegisterFunction(
      ScalarFunction("subnet_contains_or_equals", {inet_type, inet_type},
                                                   LogicalType::BOOLEAN,
                                                   INetFunctions::ContainsRight));
  // Add IP range operators
  loader.RegisterFunction(ScalarFunction("<<=", {inet_type, inet_type},
                                                 LogicalType::BOOLEAN,
                                                 INetFunctions::ContainsLeft));
  loader.RegisterFunction(ScalarFunction(">>=", {inet_type, inet_type},
                                                 LogicalType::BOOLEAN,
                                                 INetFunctions::ContainsRight));
}

void InetExtension::Load(ExtensionLoader &loader) {
  LoadInternal(loader);
}

std::string InetExtension::Name() { return "inet"; }

std::string InetExtension::Version() const {
#ifdef EXT_VERSION_INET
  return EXT_VERSION_INET;
#else
  return "";
#endif
}

} // namespace duckdb

extern "C" {

DUCKDB_CPP_EXTENSION_ENTRY(inet, loader) {
  duckdb::LoadInternal(loader);
}

}
