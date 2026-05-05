# AntX 项目文档索引

> **最后更新**: 2026-05-06 | **规范版本**: v2.0

## 文档目录结构

```
docs/
├── README.md                    # 本文件 (文档索引)
├── ai-autonomous-development-spec.md  # AI自主开发规范 [必读]
├── implementation-report.md         # 最近实施报告
├── CODE_STYLE.md                 # 代码风格指南
│
├── development/                  # 开发文档
│   ├── kernel-architecture.md    # 内核架构设计 [v2.0 已更新]
│   ├── hvfs-disk.md             # HvFS 磁盘持久化 [v2.0 已更新]
│   ├── smart-persistent-storage.md # Smart Mount 设计
│   ├── memory-management.md      # 内存管理
│   ├── syscall.md               # 系统调用接口
│   ├── process-session.md        # 进程与会话
│   ├── pwid-model.md            # PWID 权限模型
│   ├── klog-system.md           # 日志系统
│   └── ...                      # 其他模块文档
│
├── issues/                       # 问题记录
│   ├── stability-issues.md      # 稳定性问题
│   └── user-mode-gpf.md         # 用户态 Page Fault
│
├── plans/                        # 计划文档
│   └── infrastructure-strengthening-plan.md
│
├── progress/                     # 进度跟踪
│   ├── current-tasks.md         # 当前任务
│   ├── milestones.md            # 里程碑
│   └── changelog.md             # 变更日志
│
└── reports/                      # 报告存档
    └── stability-report-*.md    # 稳定性报告
```

## 快速导航

### 如果你是 AI 助手

1. **先读**: `ai-autonomous-development-spec.md` (开发规范)
2. **再读**: `implementation-report.md` (最近做了什么)
3. **参考**: `development/*.md` (具体技术细节)

### 如果你是人类开发者

1. **了解架构**: `development/kernel-architecture.md`
2. **理解文件系统**: `development/hvfs-disk.md`
3. **查看进度**: `progress/current-tasks.md`
4. **报告问题**: `issues/` 目录下对应文件

## 文档状态总览

| 文档 | 最后更新 | 状态 | 说明 |
|------|----------|------|------|
| ai-autonomous-development-spec.md | 2026-05-06 | ✅ 最新 | v2.0 精简版 |
| implementation-report.md | 2026-05-06 | ✅ 最新 | Smart Mount 实施 |
| kernel-architecture.md | 2026-05-06 | ✅ 已更新 | 含 Smart Mount |
| hvfs-disk.md | 2026-05-06 | ✅ 已更新 | 含 FFI 导出列表 |
| smart-persistent-storage.md | 2026-05-06 | ⚠️ 需检查 | 可能需要微调 |
| memory-management.md | 待确认 | ⚠️ 可能过时 | 需对比代码 |
| syscall.md | 待确认 | ⚠️ 可能过时 | 需统计当前 syscall 数 |

## 文档维护规范

**何时更新文档**:
- 完成新功能实现后
- 修改架构后
- 发现文档与代码不一致时

**更新要求**:
1. 在文件头标注最后更新日期
2. 标注变更的版本号
3. 保持格式一致 (Markdown)

---

**维护者**: AI Assistant (遵循 ai-autonomous-development-spec.md v2.0)
