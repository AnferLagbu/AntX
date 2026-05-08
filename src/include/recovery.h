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

#ifdef __cplusplus
}
#endif

#endif
