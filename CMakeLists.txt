project(inet)

cmake_minimum_required(VERSION 3.5...3.29)
option(DUCKDB_WASM_EXTENSION "Whether compiling for Wasm target" OFF)

set(EXTENSION_NAME inet)
set(DUCKDB_EXTENSION_API_VERSION_UNSTABLE 1)

###
# Configuration
###

if(NOT DEFINED EXTENSION_NAME)
    message(FATAL_ERROR "DuckDB extension name is required")
endif()
if (DEFINED TARGET_DUCKDB_VERSION_MAJOR)
    add_definitions(-DDUCKDB_EXTENSION_API_VERSION_MAJOR=${TARGET_DUCKDB_VERSION_MAJOR})
endif()
if (DEFINED TARGET_DUCKDB_VERSION_MINOR)
    add_definitions(-DDUCKDB_EXTENSION_API_VERSION_MINOR=${TARGET_DUCKDB_VERSION_MINOR})
endif()
if (DEFINED TARGET_DUCKDB_VERSION_PATCH)
    add_definitions(-DDUCKDB_EXTENSION_API_VERSION_PATCH=${TARGET_DUCKDB_VERSION_PATCH})
endif()

add_definitions(-DDUCKDB_EXTENSION_NAME=${EXTENSION_NAME})

if (DEFINED DUCKDB_EXTENSION_API_VERSION_UNSTABLE)
    add_definitions(-DDUCKDB_EXTENSION_API_VERSION_UNSTABLE=${DUCKDB_EXTENSION_API_VERSION_UNSTABLE})
endif()

###
#LTO
###
if (CMAKE_BUILD_TYPE STREQUAL "Release" AND CMAKE_VERSION VERSION_GREATER_EQUAL "3.9")
    cmake_policy(SET CMP0069 NEW)
    set(CMAKE_POLICY_DEFAULT_CMP0069 NEW)

    include(CheckIPOSupported)
    check_ipo_supported(RESULT LTO_SUPPORTED OUTPUT LTO_ERROR)
    if(LTO_SUPPORTED)
        message(STATUS "LTO enabled")
        set(CMAKE_INTERPROCEDURAL_OPTIMIZATION TRUE)
    else()
        message(STATUS "LTO not available: <${LTO_ERROR}>")
    endif()
endif()

###
# Hide symbols
###
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_C_VISIBILITY_PRESET hidden)
set(VISIBILITY_INLINES_HIDDEN ON)
set(CMAKE_C_FLAGS_RELEASE "${CMAKE_C_FLAGS_RELEASE} -s")
set(CMAKE_CXX_FLAGS_RELEASE "${CMAKE_CXX_FLAGS_RELEASE} -s")

###
# Build
###
# Create Extension library
set(EXTENSION_SOURCES
        src/inet_extension.cpp
        src/inet_html.cpp
        src/inet_html_table.cpp
        src/inet_ipaddress.cpp
)

if (DUCKDB_WASM_EXTENSION)
    add_library(${EXTENSION_NAME} STATIC ${EXTENSION_SOURCES})
else()
    add_library(${EXTENSION_NAME} SHARED ${EXTENSION_SOURCES})
endif()
# for building with DuckDB - FIXME - this needs to be reconciled somehow
# build_loadable_extension_capi_unstable(${EXTENSION_NAME} ${EXTENSION_SOURCES})


# Include own headers
target_include_directories(${EXTENSION_NAME} PRIVATE src)

# Include DuckDB C API headers
target_include_directories(${EXTENSION_NAME} PRIVATE duckdb_capi)

# Include DuckDB C++ API headers
target_include_directories(${EXTENSION_NAME} PRIVATE duckdb-cpp-api)
