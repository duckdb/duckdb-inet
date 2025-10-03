#pragma once

#include <stddef.h>
#include <stdint.h>

typedef struct INET_HTMLEntity INET_HTMLEntity;
struct INET_HTMLEntity {
	const char *name;
	uint32_t codepoints[2];
};
struct INET_HTMLEntity *inet_html_entity_lookup(const char *str, size_t len);
