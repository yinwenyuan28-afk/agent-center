//! Tauri 构建脚本。
//!
//! ## 为什么要手动声明 `rerun-if-changed=../dist`
//!
//! `tauri_build::build()` 生成的监控项只有这几条（实测）：
//!
//! ```text
//! cargo:rerun-if-env-changed=TAURI_CONFIG
//! cargo:rerun-if-changed=tauri.conf.json
//! cargo:rerun-if-changed=capabilities
//! ```
//!
//! **没有 `../dist`。** 而 `frontendDist` 指向的正是那个目录。
//!
//! 后果：`npm run build` 更新了 dist 之后，cargo 认为前端资源没变化，
//! 继续复用上一次构建时嵌进 exe 的那份前端。表现是「改了前端、构建成功、
//! 打开 exe 看到的还是旧界面」，而且 exe 大小一个字节都不变。
//!
//! 这个坑排查了很久：dev 模式正常（直接读源码）、CLI 正常（不走前端），
//! 只有 release exe 一直是旧界面。加上这行之后 dist 变化会触发重新嵌入。

fn main() {
    // 前端产物变了要重新嵌入资源
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
