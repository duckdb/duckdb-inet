#pragma once

#include <stddef.h>

size_t inet_html_unescaped_get_required_size(const char *input_data, size_t input_size);
void inet_html_unescape(const char *input_data, size_t input_size, char *result_data, size_t result_size);
