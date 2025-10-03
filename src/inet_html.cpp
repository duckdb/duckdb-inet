#include "inet_html.hpp"
#include "inet_html_table.hpp"
#include "duckdb/duckdb_stable.hpp"

#include <ctype.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

static bool codepoint_to_utf8(uint32_t cp, uint32_t *sz, char *c) {
	if (cp <= 0x7F) {
		*sz = 1;
		c[0] = cp;
	} else if (cp <= 0x7FF) {
		*sz = 2;
		c[0] = (cp >> 6) + 192;
		c[1] = (cp & 63) + 128;
	} else if (0xd800 <= cp && cp <= 0xdfff) {
		// invalid block of utf
		return false;
	} else if (cp <= 0xFFFF) {
		*sz = 3;
		c[0] = (cp >> 12) + 224;
		c[1] = ((cp >> 6) & 63) + 128;
		c[2] = (cp & 63) + 128;
	} else if (cp <= 0x10FFFF) {
		*sz = 4;
		c[0] = (cp >> 18) + 240;
		c[1] = ((cp >> 12) & 63) + 128;
		c[2] = ((cp >> 6) & 63) + 128;
		c[3] = (cp & 63) + 128;
	} else {
		return false;
	}
	return true;
}

static bool decode_codepoint(uint32_t cp, uint32_t *sz, char *c) {

	// Line-feed character
	if (cp == 0x0D) {
		memcpy(c, "\\r", 2);
		*sz = 2;
		return true;
	}

	// Replacement character
	if (cp == 0x00 || (0xD800 <= cp && cp <= 0xDFFF) || 0x10FFFF < cp) {
		// Return the replacement character
		memcpy(c, "\xEF\xBF\xBD", 3);
		*sz = 3;
		return true;
	}

	// Special character references
	if (0x80 <= cp && cp <= 0x9F) {
		static const char *map[] = {
			"\xE2\x82\xAC", // EURO SIGN
		    "\\x81",		// UNDEFINED
			"\xE2\x80\x9A", // SINGLE LOW-9 QUOTATION MARK
			"\xC6\x92",		// LATIN SMALL LETTER F WITH HOOK
			"\xE2\x80\x9E", // DOUBLE LOW-9 QUOTATION MARK
			"\xE2\x80\xA6", // HORIZONTAL ELLIPSIS
			"\xE2\x80\xA0", // DAGGER
			"\xE2\x80\xA1", // DOUBLE DAGGER
			"\xCB\x86",		// MODIFIER LETTER CIRCUMFLEX ACCENT
			"\xE2\x80\xB0", // PER MILLE SIGN
			"\xC5\xA0",		// LATIN CAPITAL LETTER S WITH CARON
			"\xE2\x80\xB9", // SINGLE LEFT-POINTING ANGLE QUOTATION MARK
			"\xC5\x92",		// LATIN CAPITAL LIGATURE O
			"\\x8d",		// UNDEFINED
			"\xC5\xBD",		// LATIN CAPITAL LETTER Z WITH CARON
			"\\x8f",		// UNDEFINED
			"\\x90",		// UNDEFINED
			"\xE2\x80\x98", // LEFT SINGLE QUOTATION MARK
			"\xE2\x80\x99", // RIGHT SINGLE QUOTATION MARK
			"\xE2\x80\x9C", // LEFT DOUBLE QUOTATION MARK
			"\xE2\x80\x9D", // RIGHT DOUBLE QUOTATION MARK
			"\xE2\x80\xA2", // BULLET
			"\xE2\x80\x93", // EN DASH
			"\xE2\x80\x94", // EM DASH
			"\xCB\x9C",		// SMALL TILDE
			"\xE2\x84\xA2", // TRADE MARK SIGN
			"\xC5\xA1",		// LATIN SMALL LETTER S WITH CARON
			"\xE2\x80\xBA", // SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
			"\xC5\x93",		// LATIN SMALL LIGATURE OE
			"\\x9d",		// UNDEFINED
			"\xC5\xBE",		// LATIN SMALL LETTER Z WITH CARON
			"\xC5\xB8"		// LATIN CAPITAL LETTER Y WITH DIAERESIS
		};
		const char *str = map[cp - 0x80];
		size_t len = strlen(str);
		memcpy(c, str, len);
		*sz = (uint32_t)len;
		return true;
	}

	// Invalid codepoints to be skipped
	if (0x1 <= cp && cp <= 0x8) {
		return false;
	}
	if (0x000E <= cp && cp <= 0x001F) {
		return false;
	}
	if (cp == 0x7F) {
		return false;
	}
	if (0xFDD0 <= cp && cp <= 0xFDEF) {
		return false;
	}
	if (cp == 0xb) {
		return false;
	}

	// Others, noncharacters
	if (cp == 0xFFFE || cp == 0xFFFF || cp == 0x1FFFE || cp == 0x1FFFF || cp == 0x2FFFE || cp == 0x2FFFF ||
	    cp == 0x3FFFE || cp == 0x3FFFF || cp == 0x4FFFE || cp == 0x4FFFF || cp == 0x5FFFE || cp == 0x5FFFF ||
	    cp == 0x6FFFE || cp == 0x6FFFF || cp == 0x7FFFE || cp == 0x7FFFF || cp == 0x8FFFE || cp == 0x8FFFF ||
	    cp == 0x9FFFE || cp == 0x9FFFF || cp == 0xAFFFE || cp == 0xAFFFF || cp == 0xBFFFE || cp == 0xBFFFF ||
	    cp == 0xCFFFE || cp == 0xCFFFF || cp == 0xDFFFE || cp == 0xDFFFF || cp == 0xEFFFE || cp == 0xEFFFF ||
	    cp == 0xFFFFE || cp == 0xFFFFF || cp == 0x10FFFE || cp == 0x10FFFF) {
		return false;
	}

	// Try to convert to utf-8
	return codepoint_to_utf8(cp, sz, c);
}

int64_t strtoll_non_null_terminated(const char *str, const char *end, const char **num_end, int base) {
	idx_t pos = 0;
	int64_t result;
	if (base == 10) {
		result = duckdb_stable::StringUtil::ToSigned(str, end - str, pos);
	} else if (base == 16) {
		result = static_cast<int64_t>(duckdb_stable::StringUtil::FromHex(str, end - str, pos));
	} else {
		throw std::runtime_error("Unsupported base");
	}
	*num_end = str + pos;
	return result;
}

static const char *decode_entity(const char *beg, const char *end, uint32_t *cp1, uint32_t *cp2) {
	const char *pos = beg;

	if (pos < end && *pos == '&') {
		pos++; // Skip '&'

		// Is this a numeric entity?
		if (pos < end && *pos == '#') {
			pos++; // Skip '#'
			int base = 10;
			if (pos < end && (*pos == 'x' || *pos == 'X')) {
				pos++; // Skip 'x' or 'X'
				base = 16;
			}

			const char *num_end = NULL;
			int64_t value = strtoll_non_null_terminated(pos, end, &num_end, base);
			if (num_end != pos && value >= 0) {
				if (value > UINT32_MAX) {
					value = 0; // Out of range, use replacement character
				}

				// Check for semicolon
				if (num_end == end) {
					// If no semicolon, but not followed by a number, still parseable
					*cp1 = (uint32_t)value;
					*cp2 = 0;
					return num_end;
				}
				if (*num_end == ';') {
					// Valid numeric with semicolon
					*cp1 = (uint32_t)value;
					*cp2 = 0;
					return num_end + 1;
				}
				if (base == 10 && !isdigit((unsigned char)*num_end)) {
					// If no semicolon, but not followed by a number, still parseable
					*cp1 = (uint32_t)value;
					*cp2 = 0;
					return num_end;
				}
				if (base == 16 && !isxdigit((unsigned char)*num_end)) {
					// If no semicolon, but not followed by a number, still parseable
					*cp1 = (uint32_t)value;
					*cp2 = 0;
					return num_end;
				}
			}
		}
		// Is this a named entity?
		else if (pos < end && isalnum((unsigned char)*pos)) {
			const char *name_beg = pos;
			while (pos < end && isalnum((unsigned char)*pos)) {
				pos++;
			}
			size_t len = pos - name_beg;
			if (len > 0) {
				INET_HTMLEntity *entity = NULL;
				if (pos < end && *pos == ';') {
					entity = inet_html_entity_lookup(name_beg, len + 1);
					if (entity) {
						// Valid named entity with semicolon
						*cp1 = entity->codepoints[0];
						*cp2 = entity->codepoints[1];
						return pos + 1;
					}
				} else if (pos == end || !isalnum((unsigned char)*pos)) {
					// No semicolon, try to look for longest match
					do {
						entity = inet_html_entity_lookup(name_beg, len);
						if (entity) {
							*cp1 = entity->codepoints[0];
							*cp2 = entity->codepoints[1];
							return name_beg + len;
						}
						len--;
					} while (len > 0);
				}
			}
		}
	}

	// Fallback, not a valid entity
	*cp1 = (unsigned char)(*beg);
	*cp2 = 0;
	return beg + 1;
}

typedef void (*html_decode_handle_func)(char *data, size_t size, void *ctx);

static void decode_html_impl(const char *data, size_t size, html_decode_handle_func handler, void *ctx) {

	const char *end = data + size;
	const char *pos = data;

	while (pos < end) {

		uint32_t code[2] = {0, 0};
		char text_data[8] = {0, 0, 0, 0, 0, 0, 0, 0};
		size_t text_size = 0;

		pos = decode_entity(pos, end, &code[0], &code[1]);

		for (int i = 0; i < 2; i++) {
			if (i == 1 && code[i] == 0) {
				// This isn't super clean - if the second codepoint is 0, skip it.
				break;
			}
			uint32_t len = 0;
			decode_codepoint(code[i], &len, text_data + text_size);
			text_size += len;
		}

		// Emit the decoded text
		handler(text_data, text_size, ctx);
	}
}

static void html_entity_get_size_handler(char *data, size_t size, void *ctx) {
	size_t *result_size = (size_t *)ctx;
	*result_size += size;
}

static void html_entity_replace_handler(char *data, size_t size, void *ctx) {
	char **result_data = (char **)ctx;
	memcpy(*result_data, data, size);
	*result_data += size;
}

size_t inet_html_unescaped_get_required_size(const char *input_data, size_t input_size) {
	// Compute the result size
	size_t result_size = 0;
	decode_html_impl(input_data, input_size, html_entity_get_size_handler, &result_size);
	return result_size;
}

void inet_html_unescape(const char *input_data, size_t input_size, char *result_data, size_t result_size) {
	// Now parse again and fill the result string with the unescaped data
	decode_html_impl(input_data, input_size, html_entity_replace_handler, &result_data);
}
