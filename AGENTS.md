# AGENTS.md

## 概览

AntX 是 Framekernel 双子树内核：framework/ 是 TCB（允许 unsafe），services/ 是 100% safe Rust。开始任何任务前先读完本文，再去读列出的文档。

## 架构责任分离

开发时严格遵循框内核架构的责任分离原则：framework/ 负责硬件抽象与机制（MMU、中断、DMA、上下文切换、原语），services/ 负责策略与业务（系统调用分发、进程策略、文件系统实现、驱动集成）。能放 services/ 的别放 framework/，需要直接操作硬件或必须 unsafe 的放 framework/ 并暴露为 safe API。新增驱动、跨层 bug 修复、新增 CPU 架构时两边都改。详见 `docs/explain/framekernel-dev-guide.md`。

## 必读文档

`docs/` 下所有文档：
`docs/README.md`
`docs/explain/framekernel-nature.md`
`docs/explain/framekernel-dev-guide.md`
`docs/plan/kernel-roadmap.md`

## CLAUDE.md

`CLAUDE.md` 全文。

## 框架与归属

详见 `docs/explain/framekernel-dev-guide.md`。

## 安全契约

详见 `docs/explain/framekernel-nature.md` 与 `docs/explain/framekernel-dev-guide.md`。

## 编码风格

详见 `CLAUDE.md` 中"外科手术式修改"与"简单优先"两条；详细规范文档待补。

## 构建与测试

`make ARCH=x86_64` 与 `make ARCH=aarch64`（双架构零 warning 零 error）。
`make test-host` 主机测试。
`scripts/qemu_boot_test.sh` 与 `scripts/qemu_debug.sh` QEMU 启动与 GDB。
`make test-all` 端到端。
`cargo miri test` 验证 unsafe，case 放 `miri-tests/`。
集成测试样板 `host-tests/tests/hvfs_test.rs` 与 `host-tests/tests/stress_test.rs`。
测试组织详见 `host-tests/Cargo.toml` 与 `host-tests/src/lib.rs`。

## 审计

`scripts/audit_services_boundary.py`
`scripts/audit_safety_coverage.py`
`scripts/audit_deadlock_matrix.py`
`ci/build.sh`
`ci/audit.sh`
任何一项失败视为本轮未完成。

## 预存问题

开发中遇到与本任务无关的预存问题（编译告警、死代码、未使用 import、过期 TODO、CI 脚本缺陷、文档与代码不一致）必须立即修复并补测试或更新文档。修复后重跑双架构编译、相关审计与相关测试。不接受留下 TODO 等下一轮、以不在本任务范围为由略过、删除有意义的测试以让编译通过。

## AI 常见踩坑

把 unsafe 写进 services/ 会编译失败，改用 framework 公开的 safe API。在 services/ 用 println! 在 no_std 下不可用，改用 klog::printk 或 framework 提供的日志 API。在中断上下文持有 Mutex 或分配 GFP_KERNEL 会死锁，中断路径只能持自旋锁并 disable IRQ。修 bug 时不要顺手清理无关代码，每一行改动都要能追溯到用户请求。在 services/ 直接 use kernel::framework::arch::x86_64 等内部模块会被边界审计拒绝，只能走顶层 re-export 的公共 API。
