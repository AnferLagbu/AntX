/* ============================================================
 * sys_arch.c — lwIP 平台移植层实现
 *
 * 将 lwIP 的 OS 抽象层 (信号量/邮箱/线程/临界区/时间)
 * 映射到 AntX (QueenX) 内核的现有基础设施:
 *   - 信号量  → mutex_t
 *   - 邮箱    → 环形缓冲 + 信号量
 *   - 临界区  → cli/sti
 *   - 时间    → timer_ticks (100Hz)
 * ============================================================ */

#include "lwip/arch.h"
#include "lwip/opt.h"
#include "lwip/sys.h"
#include "lwip/err.h"
#include "sys_arch.h"
#include "spinlock.h"
#include "klog.h"
#include "proc.h"
#include "mutex.h"

/* ---- 全局变量 ---- */
int errno;

/* ---- 全局 tick 计数器 ---- */
static volatile u32_t g_sys_ticks = 0;

/* ============================================================
 * 初始化
 * ============================================================ */
void sys_init(void)
{
    g_sys_ticks = 0;
    klog_net("sys_arch ready");
}

/* ============================================================
 * 时间
 * ============================================================ */
u32_t sys_now(void)
{
    return g_sys_ticks * 10;  /* 100Hz → ms */
}

/* 由 timer ISR 调用 */
void sys_tick_inc(void)
{
    g_sys_ticks++;
}

/* ============================================================
 * 信号量 (基于 QX Mutex) — NO_SYS=0 时使用
 * ============================================================ */
#if !NO_SYS
err_t sys_sem_new(sys_sem_t *sem, u8_t count)
{
    mutex_init(sem);
    (void)count;
    return ERR_OK;
}

void sys_sem_free(sys_sem_t *sem)
{
    (void)sem;
}

void sys_sem_signal(sys_sem_t *sem)
{
    mutex_unlock(sem);
}

u32_t sys_arch_sem_wait(sys_sem_t *sem, u32_t timeout)
{
    if (timeout == 0) {
        /* 无限等待 */
        mutex_lock(sem);
        return 0;
    }

    /* 带超时等待 */
    u32_t start = sys_now();
    while (1) {
        if (mutex_trylock(sem)) {
            return sys_now() - start;
        }
        if (sys_now() - start >= timeout) {
            return SYS_ARCH_TIMEOUT;
        }
        __asm__ volatile("pause" ::: "memory");
    }
}

int sys_sem_valid(sys_sem_t *sem)
{
    return sem != NULL;
}

void sys_sem_set_invalid(sys_sem_t *sem)
{
    (void)sem;
}

/* ============================================================
 * 互斥锁
 * ============================================================ */
err_t sys_mutex_new(sys_mutex_t *mutex)
{
    mutex_init(mutex);
    return ERR_OK;
}

void sys_mutex_free(sys_mutex_t *mutex)
{
    (void)mutex;
}

void sys_mutex_lock(sys_mutex_t *mutex)
{
    mutex_lock(mutex);
}

void sys_mutex_unlock(sys_mutex_t *mutex)
{
    mutex_unlock(mutex);
}

/* ============================================================
 * 邮箱
 * ============================================================ */
err_t sys_mbox_new(sys_mbox_t *mbox, int size)
{
    (void)size;
    mbox->head = 0;
    mbox->tail = 0;
    mbox->count = 0;
    mutex_init(&mbox->lock);
    mutex_init(&mbox->sem_full);
    mutex_init(&mbox->sem_empty);

    mutex_lock(&mbox->sem_empty);

    return ERR_OK;
}

void sys_mbox_free(sys_mbox_t *mbox)
{
    (void)mbox;
}

void sys_mbox_post(sys_mbox_t *mbox, void *msg)
{
    mutex_lock(&mbox->lock);

    /* 等待空位 */
    while (mbox->count >= SYS_MBOX_SIZE) {
        mutex_unlock(&mbox->lock);
        mutex_lock(&mbox->sem_empty);
        mutex_lock(&mbox->lock);
    }

    mbox->messages[mbox->tail] = msg;
    mbox->tail = (mbox->tail + 1) % SYS_MBOX_SIZE;
    mbox->count++;

    mutex_unlock(&mbox->lock);
    mutex_unlock(&mbox->sem_full);
}

err_t sys_mbox_trypost(sys_mbox_t *mbox, void *msg)
{
    if (!mutex_trylock(&mbox->lock)) {
        return ERR_MEM;
    }

    if (mbox->count >= SYS_MBOX_SIZE) {
        mutex_unlock(&mbox->lock);
        return ERR_MEM;
    }

    mbox->messages[mbox->tail] = msg;
    mbox->tail = (mbox->tail + 1) % SYS_MBOX_SIZE;
    mbox->count++;

    mutex_unlock(&mbox->lock);
    mutex_unlock(&mbox->sem_full);

    return ERR_OK;
}

u32_t sys_arch_mbox_fetch(sys_mbox_t *mbox, void **msg, u32_t timeout)
{
    u32_t start = sys_now();

    while (1) {
        mutex_lock(&mbox->lock);

        if (mbox->count > 0) {
            *msg = mbox->messages[mbox->head];
            mbox->head = (mbox->head + 1) % SYS_MBOX_SIZE;
            mbox->count--;

            mutex_unlock(&mbox->lock);
            mutex_unlock(&mbox->sem_empty);
            return sys_now() - start;
        }

        mutex_unlock(&mbox->lock);

        if (timeout > 0 && (sys_now() - start) >= timeout) {
            return SYS_ARCH_TIMEOUT;
        }

        /* 等待新消息 */
        mutex_lock(&mbox->sem_full);
    }
}

u32_t sys_arch_mbox_tryfetch(sys_mbox_t *mbox, void **msg)
{
    if (!mutex_trylock(&mbox->lock)) {
        return SYS_MBOX_EMPTY;
    }

    if (mbox->count > 0) {
        *msg = mbox->messages[mbox->head];
        mbox->head = (mbox->head + 1) % SYS_MBOX_SIZE;
        mbox->count--;
        mutex_unlock(&mbox->lock);
        mutex_unlock(&mbox->sem_empty);
        return 0;
    }

    mutex_unlock(&mbox->lock);
    return SYS_MBOX_EMPTY;
}

/* fromisr — same as posting since we're in kernel space */
err_t sys_mbox_trypost_fromisr(sys_mbox_t *mbox, void *msg)
{
    return sys_mbox_trypost(mbox, msg);
}

int sys_mbox_valid(sys_mbox_t *mbox)
{
    return mbox != NULL;
}

void sys_mbox_set_invalid(sys_mbox_t *mbox)
{
    (void)mbox;
}

/* ============================================================
 * 线程
 * ============================================================ */
sys_thread_t sys_thread_new(const char *name, void (*thread)(void *arg),
                            void *arg, int stacksize, int prio)
{
    (void)name;
    (void)thread;
    (void)arg;
    (void)stacksize;
    (void)prio;

    /*
     * Phase 2: 使用 proc_create_internal() 创建内核线程
     * 当前 lwIP 先运行在单线程模式 (tcpip_thread 主循环)
     */
    return 0;
}

#endif /* !NO_SYS */

/* ============================================================
 * 临界区保护 (中断禁用/恢复)
 * ============================================================ */
sys_prot_t sys_arch_protect(void)
{
    sys_prot_t flags;
    __asm__ volatile(
        "pushfq\n\t"
        "popq %0\n\t"
        "cli"
        : "=r"(flags)
        :: "memory"
    );
    return flags;
}

void sys_arch_unprotect(sys_prot_t flags)
{
    if (flags & (1UL << 9)) {
        __asm__ volatile("sti" ::: "memory");
    }
}

/* ---- stdlib stubs ---- */
long strtol(const char *str, char **endptr, int base)
{
    long n = 0;
    int neg = 0;
    if (*str == '-') { neg = 1; str++; }
    while (*str >= '0' && *str <= '9') {
        n = n * ((base) ? base : 10) + (*str - '0');
        str++;
    }
    if (endptr) *endptr = (char *)str;
    return neg ? -n : n;
}

int atoi(const char *str)
{
    int n = 0;
    while (*str >= '0' && *str <= '9') { n = n * 10 + (*str - '0'); str++; }
    return n;
}

/* ---- inet stubs (shim to lwIP's ip4addr_ntoa) ---- */
const char *inet_ntoa(void *addr)
{
    extern const char *ip4addr_ntoa(const void *addr);
    return ip4addr_ntoa(addr);
}

const char *inet_ntoa_r(void *addr, char *buf, int buflen)
{
    extern const char *ip4addr_ntoa_r(const void *addr, char *buf, int buflen);
    return ip4addr_ntoa_r(addr, buf, buflen);
}
