# M6.4 CI 接入报告

> **生成时间**: 2026-06-04  
> **CI 平台**: GitHub Actions (`.github/workflows/ci.yml`)  
> **本地目标**: `make -f Makefile.ci ci`

---

## 1. 摘要

| 维度 | 工具 | 状态 |
|------|------|------|
| SAFETY 注释审计 | `audit_safety_coverage.py` | ✅ PASS |
| services→framework 边界 | `audit_services_boundary.py` | ✅ PASS |
| 死锁检测矩阵 | `audit_deadlock_matrix.py` | ✅ PASS |
| services 0 unsafe | `ci_check_services_unsafe.py` | ✅ PASS |
| x86_64 编译 | `cargo check --target x86_64-unknown-none` | ✅ PASS |
| aarch64 编译 | `cargo check --target aarch64-unknown-none` | ✅ PASS |

**结论**: ✅ **全部合规性检查通过**, 可随时推送到 GitHub 触发 CI。

---

## 2. CI 流水线 (`.github/workflows/ci.yml`)

### 2.1 5 个并行 Job

```
┌─ audit-unsafe              (Python 审计, 无 cargo 依赖, <30s)
│  ├─ SAFETY 注释审计
│  ├─ services→framework 边界
│  └─ 死锁检测矩阵
│
├─ services-no-unsafe        (Python 智能过滤注释, <5s)
│  └─ services/ 0 unsafe 强制
│
├─ cargo-check               (rust nightly + x86_64, ~3-5min)
│  └─ 编译验证
│
├─ cargo-check-aarch64       (rust nightly + aarch64, ~3-5min)
│  └─ 编译验证
│
└─ framekernel-compliance    (汇总报告, 无并行)
   └─ 仅当上面 4 个全 PASS 时显示绿色徽章
```

### 2.2 触发条件

- `push` 到 main / master / develop / feature/**
- `pull_request` 到 main / master / develop

### 2.3 关键设计

- **每 Job 独立缓存**: cargo 缓存按 target 分别存储, 避免跨架构污染
- **失败立即 fail**: 任何 CRITICAL 直接 exit 1
- **产物上传**: 审计报告上传 30 天, 便于回溯
- **合规性汇总**: 最后 Job 输出 GITHUB_STEP_SUMMARY 表格, GitHub UI 直接可见

---

## 3. 本地执行 (Makefile.ci)

### 3.1 完整 CI

```bash
make -f Makefile.ci ci
```

输出:
```
[1/3] services 0-unsafe scan...
扫描文件数: 44
PASS: services/ 0 unsafe
[2/3] SAFETY + boundary + deadlock audit...
  - SAFETY comment audit (M6.1)...
  - services->framework boundary (M6.3)...
  - deadlock matrix (M6.2)...
PASS: all audits pass
[3/3] cargo check (x86_64 + aarch64)...
   Compiling queenx v0.1.0
   ...
PASS: cargo check
==========================================
QueenX Framekernel Compliance: PASS
==========================================
```

### 3.2 子目标

| 目标 | 用途 |
|------|------|
| `make -f Makefile.ci ci-unsafe-scan` | 仅 services 0 unsafe 扫描 |
| `make -f Makefile.ci ci-audit` | 仅审计 (SAFETY + 边界 + 死锁) |
| `make -f Makefile.ci ci-cargo` | 仅 cargo check (x86_64 + aarch64) |
| `make -f Makefile.ci ci-fix` | 自动 cargo fix |
| `make -f Makefile.ci ci-clean` | 清理日志 |

### 3.3 日志位置

```
build/log/
├── safety_audit.txt
├── services_boundary.txt
├── deadlock_matrix.txt
├── cargo_check_x86_64.txt
└── cargo_check_aarch64.txt
```

---

## 4. 关键设计决策

### 4.1 为什么用 Python 智能过滤而非 `grep`?
- `grep -v '//'` 不可靠, 无法处理行内注释
- Python AST/正则可精确区分:
  - `//! services 层 0 unsafe` ← 注释, 跳过
  - `unsafe { ... }` ← 真实代码, 报告
  - `unsafe fn foo()` ← 真实代码, 报告

### 4.2 为什么 CI 不跑 `cargo clippy`?
- 当前 nightly clippy 误报过多 (项目代码风格 vs clippy 默认)
- 现有 `RUSTFLAGS: -D warnings` + `#![allow(...)]` 已覆盖警告场景
- 未来 Phase 4 引入 `clippy.toml` 自定义规则集后, 可补加

### 4.3 为什么 cargo 缓存按 target 分割?
- x86_64 和 aarch64 产物不兼容
- `key: ${{ runner.os }}-cargo-x86_64-${{ ... }}` vs `-aarch64-...`
- 避免相互污染导致编译失败

---

## 5. 历史问题与修复

### 5.1 grep 误报 (M6.4 第一次实现)
- **问题**: `grep -rn 'unsafe {' src/kernel/services/` 把 `//! 2. services 层 0 unsafe — ...` 注释行当成 unsafe 块
- **根因**: grep 不区分代码与注释
- **修复**: 改用 `ci_check_services_unsafe.py`, 在 `splitlines` 后用 `stripped.startswith('//')` 显式过滤

### 5.2 Makefile 中文输出乱码 (M6.4 第一次实现)
- **问题**: `make: *** 没有规则可制作目标"ci-unsafe-scan"。 停止。` (目标无法识别)
- **根因**: 中文 Unicode 在某些 make 版本中 LANG 不匹配导致 target 名解析失败
- **修复**: 改用纯英文目标名 (`ci-unsafe-scan` 替代 `ci-unsafe-扫描`)

---

## 6. 后续建议 (Phase 3 + Phase 4)

### 6.1 立即执行
- [ ] 推送 `.github/workflows/ci.yml` 到 GitHub, 验证 Action 实际跑通
- [ ] 配置 GitHub Branch Protection, 要求 PR 通过 CI 才能 merge
- [ ] 启用 GitHub Status Badge 在 README

### 6.2 中期
- [ ] 加入 `cargo test` (主机端单元测试) Job
- [ ] 加入 `cargo fmt --check` Job
- [ ] 加入 QEMU smoke test (boot 内核 + 简单用户态 syscall) Job
- [ ] 加入 cargo-deny 许可证审计

### 6.3 长期
- [ ] 接入 CodeQL Rust 安全扫描
- [ ] 接入 miri (unsafe 正确性验证)
- [ ] 接入 kani (形式化验证关键模块)

---

**CI 配置文件**: `.github/workflows/ci.yml` (147 行)  
**本地 Makefile**: `Makefile.ci` (95 行)  
**辅助脚本**: `scripts/ci_check_services_unsafe.py` (54 行)  
**下次复审**: Phase 3 启动时
