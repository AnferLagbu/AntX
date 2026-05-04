#ifndef QX_SYS_ARCH_H
#define QX_SYS_ARCH_H

/* ============================================================
 * lwIP sys_arch 移植层类型定义 — AntX (QueenX) 内核
 * ============================================================ */

#include "cc.h"
#include "mutex.h"
#include "rwlock.h"

/* ============================================================
 * 信号量 (基于 Mutex)
 * ============================================================ */
typedef mutex_t sys_sem_t;

/* ============================================================
 * 互斥锁 (递归)
 * ============================================================ */
typedef mutex_t sys_mutex_t;

/* ============================================================
 * 邮箱 (基于等待队列 + 环形缓冲)
 * ============================================================ */
#define SYS_MBOX_SIZE  32

typedef struct {
    void *messages[SYS_MBOX_SIZE];
    volatile int head;
    volatile int tail;
    volatile int count;
    sys_sem_t sem_empty;
    sys_sem_t sem_full;
    sys_mutex_t lock;
} sys_mbox_t;

/* ============================================================
 * 线程
 * ============================================================ */
typedef uint32_t sys_thread_t;

/* ============================================================
 * 临界区保护 (中断)
 * ============================================================ */
typedef uint64_t sys_prot_t;

/* ============================================================
 * 函数声明
 * ============================================================ */
void sys_init(void);

/* 信号量 */
err_t sys_sem_new(sys_sem_t *sem, u8_t count);
void  sys_sem_free(sys_sem_t *sem);
void  sys_sem_signal(sys_sem_t *sem);
u32_t sys_arch_sem_wait(sys_sem_t *sem, u32_t timeout);
int   sys_sem_valid(sys_sem_t *sem);
void  sys_sem_set_invalid(sys_sem_t *sem);

/* 互斥锁 */
err_t sys_mutex_new(sys_mutex_t *mutex);
void  sys_mutex_free(sys_mutex_t *mutex);
void  sys_mutex_lock(sys_mutex_t *mutex);
void  sys_mutex_unlock(sys_mutex_t *mutex);

/* 邮箱 */
err_t sys_mbox_new(sys_mbox_t *mbox, int size);
void  sys_mbox_free(sys_mbox_t *mbox);
void  sys_mbox_post(sys_mbox_t *mbox, void *msg);
err_t sys_mbox_trypost(sys_mbox_t *mbox, void *msg);
u32_t sys_arch_mbox_fetch(sys_mbox_t *mbox, void **msg, u32_t timeout);
u32_t sys_arch_mbox_tryfetch(sys_mbox_t *mbox, void **msg);
int   sys_mbox_valid(sys_mbox_t *mbox);
void  sys_mbox_set_invalid(sys_mbox_t *mbox);

/* 线程 */
sys_thread_t sys_thread_new(const char *name, void (*thread)(void *arg),
                            void *arg, int stacksize, int prio);

/* 临界区 */
sys_prot_t sys_arch_protect(void);
void       sys_arch_unprotect(sys_prot_t pval);

/* 时间 */
u32_t sys_now(void);

#endif /* QX_SYS_ARCH_H */
