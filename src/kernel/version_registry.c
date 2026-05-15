int version_register(const char *n, unsigned char a, unsigned char b, unsigned char c, const char *v, int t) { return 0; }
int version_set_status(const char *n, int s) { return 0; }
const void *version_query(const char *n) { return (void*)0; }
int version_get_registered_count(void) { return 0; }
void version_print_registry(void (*f)(const char*)) {}
int version_export_json(char *b, unsigned long s) { return 0; }
