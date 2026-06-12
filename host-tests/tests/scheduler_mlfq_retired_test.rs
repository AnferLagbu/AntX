//! I-35: 调度器无 MLFQ/CFS 冗余 — 静态契约测试
//!
//! 验证 maintenance-2026-06-11.md 中 I-35 验收:
//!   "调度器模块数 ≤ 2 (CFS + RT)"
//!   "文档明确调度策略选择"

use std::fs;
use std::path::Path;

const SCHEDULER: &str = "src/kernel/framework/proc/scheduler.rs";

#[test]
fn test_add_to_run_queue_routes_to_cfs() {
    // 历史 bug: add_to_run_queue 把 pid 推到 queues[0] (MLFQ), 但 schedule() 只读 cfs_rq.
    // 修复后 add_to_run_queue 必须委托给 cfs_enqueue, 否则新进程永远不会被调度.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(SCHEDULER);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    // 提取 add_to_run_queue 函数体 (简化查找, 假设函数在 30 行内)
    let needle = "pub fn add_to_run_queue(&self, pid: Pid) {";
    let start = src.find(needle)
        .unwrap_or_else(|| panic!("未找到 add_to_run_queue 定义"));
    let body = &src[start..src.len().min(start + 600)];

    // 函数体必须包含 cfs_enqueue 委托调用
    assert!(
        body.contains("self.cfs_enqueue(pid)"),
        "add_to_run_queue 必须重定向到 cfs_enqueue (I-35); body: {}",
        &body[..body.find('}').unwrap_or(body.len())]
    );

    // 反向: 禁止直接 push_back 到 queues[0]
    let body_until_close = &body[..body.find('}').unwrap_or(body.len())];
    assert!(
        !body_until_close.contains("queues[0].lock().push_back"),
        "add_to_run_queue 不应再直接 push 到 MLFQ queues[0] (I-35)"
    );
}

#[test]
fn test_scheduler_doc_lists_three_policies() {
    // 模块顶部 doc 必须明确列出 DL/RT/CFS 三策略, 不再提 MLFQ 作为活跃实现
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(SCHEDULER);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    // 取文件前 2KB
    let head = &src[..src.len().min(2048)];
    for p in &["DL", "RT", "CFS"] {
        assert!(head.contains(p), "调度器 doc 应列出策略 {} (I-35)", p);
    }
    // 必须明确 MLFQ 已退役
    assert!(
        head.contains("MLFQ 已退役") || head.contains("MLFQ retired"),
        "调度器 doc 应明确 MLFQ 已退役 (I-35)"
    );
}

#[test]
fn test_pick_cfs_task_only_reads_cfs_rq() {
    // schedule() 的 pick 链 (DL → RT → CFS) 不应从 queues[] 数组读取
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(SCHEDULER);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    // pick_cfs_task 函数体不应涉及 queues[]
    if let Some(start) = src.find("fn pick_cfs_task") {
        let body = &src[start..src.len().min(start + 1500)];
        let body_end = body.find("\n    fn ").unwrap_or(body.len());
        let body = &body[..body_end];
        assert!(
            !body.contains("queues["),
            "pick_cfs_task 不应从 MLFQ queues[] 读取 (I-35)"
        );
    }
}
