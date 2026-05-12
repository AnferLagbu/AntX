#include "string.h"

void *memset(void *s, int c, size_t n) {
    unsigned char *p = (unsigned char *)s;
    while (n--) *p++ = (unsigned char)c;
    return s;
}

void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    while (n--) *d++ = *s++;
    return dst;
}

void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    if (d < s) { while (n--) *d++ = *s++; }
    else { d += n - 1; s += n - 1; while (n--) *d-- = *s--; }
    return dst;
}

int memcmp(const void *s1, const void *s2, size_t n) {
    const unsigned char *p1 = (const unsigned char *)s1;
    const unsigned char *p2 = (const unsigned char *)s2;
    while (n--) {
        if (*p1 != *p2) return (int)*p1 - (int)*p2;
        p1++; p2++;
    }
    return 0;
}

size_t strlen(const char *s) {
    size_t n = 0;
    while (*s++) n++;
    return n;
}

int strcmp(const char *s1, const char *s2) {
    while (*s1 && *s2 && *s1 == *s2) { s1++; s2++; }
    return (unsigned char)*s1 - (unsigned char)*s2;
}

int strncmp(const char *s1, const char *s2, size_t n) {
    while (n && *s1 && *s2 && *s1 == *s2) { s1++; s2++; n--; }
    if (n == 0) return 0;
    return (unsigned char)*s1 - (unsigned char)*s2;
}

char *strcpy(char *dst, const char *src) {
    char *d = dst;
    while ((*d++ = *src++));
    return dst;
}

char *strncpy(char *dst, const char *src, size_t n) {
    char *d = dst;
    while (n && (*d++ = *src++)) n--;
    while (n--) *d++ = '\0';
    return dst;
}

char *strchr(const char *s, int c) {
    while (*s && *s != (char)c) s++;
    return *s == (char)c ? (char *)s : (void*)0;
}

char *strstr(const char *haystack, const char *needle) {
    size_t n = strlen(needle);
    if (n == 0) return (char *)haystack;
    while (*haystack) {
        if (strncmp(haystack, needle, n) == 0) return (char *)haystack;
        haystack++;
    }
    return (void*)0;
}

long strtol(const char *nptr, char **endptr, int base) {
    long result = 0;
    int sign = 1;
    while (*nptr == ' ') nptr++;
    if (*nptr == '-') { sign = -1; nptr++; }
    else if (*nptr == '+') nptr++;
    if (base == 0) base = (*nptr == '0') ? 8 : 10;
    if (base == 16 && *nptr == '0' && (nptr[1] == 'x' || nptr[1] == 'X')) nptr += 2;
    while (*nptr) {
        int digit = *nptr >= '0' && *nptr <= '9' ? *nptr - '0' :
                    *nptr >= 'a' && *nptr <= 'f' ? *nptr - 'a' + 10 :
                    *nptr >= 'A' && *nptr <= 'F' ? *nptr - 'A' + 10 : -1;
        if (digit < 0 || digit >= base) break;
        result = result * base + digit;
        nptr++;
    }
    if (endptr) *endptr = (char *)nptr;
    return result * sign;
}

unsigned long strtoul(const char *nptr, char **endptr, int base) {
    unsigned long result = 0;
    while (*nptr == ' ') nptr++;
    if (base == 0) base = (*nptr == '0') ? 8 : 10;
    if (base == 16 && *nptr == '0' && (nptr[1] == 'x' || nptr[1] == 'X')) nptr += 2;
    while (*nptr) {
        int digit = *nptr >= '0' && *nptr <= '9' ? *nptr - '0' :
                    *nptr >= 'a' && *nptr <= 'f' ? *nptr - 'a' + 10 :
                    *nptr >= 'A' && *nptr <= 'F' ? *nptr - 'A' + 10 : -1;
        if (digit < 0 || digit >= base) break;
        result = result * base + digit;
        nptr++;
    }
    if (endptr) *endptr = (char *)nptr;
    return result;
}

int snprintf(char *buf, size_t size, const char *fmt, ...) {
    (void)buf; (void)size; (void)fmt;
    return 0;
}
