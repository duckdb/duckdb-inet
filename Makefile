.PHONY: clean clean_all

PROJ_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

# Main extension configuration
EXTENSION_NAME=inet

# Set to 1 to enable Unstable API (binaries will only work on TARGET_DUCKDB_VERSION, forwards compatibility will be broken)
# WARNING: When set to 1, the duckdb_extension.h from the TARGET_DUCKDB_VERSION must be used, using any other version of
#          the header is unsafe.
USE_UNSTABLE_C_API=0

# The DuckDB version to target
TARGET_DUCKDB_VERSION=v1.5.6
DUCKDB_TEST_VERSION=2.0.0.dev2610011535

all: configure release

# Include makefiles from DuckDB
include extension-ci-tools/makefiles/c_api_extensions/base.Makefile
include extension-ci-tools/makefiles/c_api_extensions/c_cpp.Makefile

configure: venv platform extension_version

debug: build_extension_library_debug build_extension_with_metadata_debug
release: build_extension_library_release build_extension_with_metadata_release

test: test_debug
test_debug: test_extension_debug
test_release: test_extension_release

ifeq ($(CI_UPLOAD_ALL_EXTENSIONS),true)
release: stage_ci_artifact

.PHONY: stage_ci_artifact
stage_ci_artifact: build_extension_with_metadata_release
	$(PYTHON_BIN) scripts/ci_artifacts.py stage --build-directory "$(EXTENSION_BUILD_PATH)/release" --filename "$(EXTENSION_FILENAME)" --duckdb-version "$(DUCKDB_VERSION)" --platform "$(DUCKDB_PLATFORM)"
endif

ifneq ($(CI_EXTENSION_ARTIFACT_DIR),)
check_configure: configure
test_extension_release_internal: restore_ci_artifact

.PHONY: restore_ci_artifact
restore_ci_artifact: configure
	$(PYTHON_BIN) scripts/ci_artifacts.py restore --build-directory "$(EXTENSION_BUILD_PATH)/release" --filename "$(EXTENSION_FILENAME)" --duckdb-version "$(DUCKDB_VERSION)" --platform "$(DUCKDB_PLATFORM)" --repository "$(CI_EXTENSION_ARTIFACT_DIR)"
endif

clean: clean_build clean_cmake
clean_all: clean clean_configure
