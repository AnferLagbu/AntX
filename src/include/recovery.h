#ifndef _RECOVERY_H
#define _RECOVERY_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

#define RECOVERY_DOMAIN_RAMFS   1
#define RECOVERY_DOMAIN_VFS     2
#define RECOVERY_DOMAIN_HVFS    3
#define RECOVERY_DOMAIN_DISKFS  4
#define RECOVERY_DOMAIN_NET     5
#define RECOVERY_DOMAIN_PROCFS  6

#define DOMAIN_STATE_ACTIVE       0
#define DOMAIN_STATE_FREEZING     1
#define DOMAIN_STATE_ROLLINGBACK  2
#define DOMAIN_STATE_RECOVERING   3
#define DOMAIN_STATE_QUARANTINED  4

#define MAX_CONSECUTIVE_FAILURES  5
#define DEFAULT_BARRIER_INTERVAL  100

/**
 * Register a recovery domain.
 * @param domain_id  unique domain identifier (1-63)
 * @return 0 on success, -1 on failure
 */
int32_t recovery_domain_register(uint64_t domain_id);

/**
 * Unregister a recovery domain (for test cleanup).
 * @param domain_id  domain to remove
 * @return 0 on success, -1 if not found
 */
int32_t recovery_domain_unregister(uint64_t domain_id);

/**
 * Advance barrier generations for all active domains.
 * Called from scheduler tick.
 */
void recovery_barrier_maintenance(void);

/**
 * Manually trigger a domain rollback for testing.
 * @param domain_id  domain to rollback
 * @param crash_fingerprint  0 = always allowed; non-zero = duplicate detection
 * @return 0 if rollback was attempted, -1 if refused (quarantined/backoff)
 */
int32_t recovery_test_rollback(uint64_t domain_id, uint64_t crash_fingerprint);

/**
 * Check if the panic handler has set the recovery flag.
 * @return 1 if panic occurred, 0 otherwise
 */
int32_t recovery_panic_flag_is_set(void);

/**
 * Clear the panic flag after successful recovery.
 */
void recovery_panic_flag_clear(void);

/**
 * Attempt domain-level recovery from the IDT exception handler.
 * Must be called BEFORE the kernel panic halt.
 * @return 0 if recovery succeeded, -1 if no domains, -2 if already attempted, -3 if lock busy
 */
int32_t recovery_try_recover_from_idt(void);

/**
 * Deliberately trigger a panic for end-to-end testing.
 * WARNING: This is noreturn — only call from a safe context.
 */
void recovery_trigger_panic(void) __attribute__((noreturn));

/**
 * Check if a recovery was attempted (post-recovery verification).
 */
int32_t recovery_was_attempted(void);

/**
 * Record a value into a domain's UndoLog (for testing).
 * @param domain_id  target domain
 * @param field_ptr  fake field address (just a key for dedup)
 * @param old_value  64-bit old value to record
 * @return 0 on success, -1 if domain not found
 */
int32_t recovery_undo_record(uint64_t domain_id, void *field_ptr, uint64_t old_value);

/**
 * Get current UndoLog entry count for a domain.
 * @param domain_id  target domain
 * @return entry count, -1 if domain not found
 */
int32_t recovery_undo_count(uint64_t domain_id);

#ifdef __cplusplus
}
#endif

#endif
