//! 审核报告生成。
//!
//! 支持 Markdown / JSON / SARIF / CSV 四种输出格式。

use std::collections::BTreeMap;

use cr_core::scoring::{HealthGrade, SeverityCounts};
use cr_core::{Finding, Severity};

/// 报告输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportFormat {
    /// Markdown 格式（适合 PR 评论）。
    Markdown,
    /// JSON 格式（适合程序消费）。
    Json,
    /// SARIF 格式（兼容 GitHub Advanced Security）。
    Sarif,
    /// CSV 格式（适合 Excel / Google Sheets 导入，便于多文件结果筛选排序）。
    Csv,
}

impl ReportFormat {
    /// 从字符串解析格式。
    ///
    /// # Errors
    ///
    /// 当格式名称不被识别时返回错误。
    pub fn parse(s: &str) -> Result<Self, UnknownFormatError> {
        match s.to_lowercase().as_str() {
            "markdown" | "md" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            "sarif" => Ok(Self::Sarif),
            "csv" => Ok(Self::Csv),
            other => Err(UnknownFormatError(other.to_string())),
        }
    }
}

/// 未知格式错误。
#[derive(Debug, thiserror::Error)]
#[error("未知报告格式: {0}（支持: markdown, json, sarif, csv）")]
pub struct UnknownFormatError(pub String);

/// 报告渲染上下文，聚合所有需要展示的信息。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RenderContext {
    /// 全部审核发现。
    pub findings: Vec<Finding>,
    /// 严重度统计。
    pub severity_counts: SeverityCounts,
    /// 健康度评分（0-100）。
    pub health_score: f64,
    /// 健康度等级。
    pub health_grade: HealthGrade,
    /// 分支名。
    pub branch: String,
    /// 是否发生降级。
    pub degraded: bool,
    /// 跳过的文件（不支持的类型）。
    ///
    /// 这些文件因类型不受支持而未经过审核（例如 `pom.xml`）。
    /// 此字段不影响 `health_score`，仅用于审计追溯。
    pub skipped_files: Vec<String>,
    /// 被配置排除而忽略的文件（exclude 模式匹配）。
    ///
    /// 这些文件出现在 `--files` 或 `--manifest` 中，但匹配了 `project.exclude`
    /// 配置中的 glob 模式，因此未经过审核。
    /// 此字段不影响 `health_score`，仅用于告知用户哪些文件被配置排除。
    pub ignored_files: Vec<String>,
}

impl RenderContext {
    /// 创建渲染上下文。
    #[must_use]
    pub fn new(
        findings: Vec<Finding>,
        severity_counts: SeverityCounts,
        health_score: f64,
        health_grade: HealthGrade,
        branch: String,
        degraded: bool,
    ) -> Self {
        Self {
            findings,
            severity_counts,
            health_score,
            health_grade,
            branch,
            degraded,
            skipped_files: Vec::new(),
            ignored_files: Vec::new(),
        }
    }

    /// 设置跳过的文件列表（构建器模式）。
    ///
    /// 设置后，将在报告中显示跳过计数和文件列表。
    /// 不影响 `health_score`。
    #[must_use]
    pub fn with_skipped_files(mut self, files: Vec<String>) -> Self {
        self.skipped_files = files;
        self
    }

    /// 设置被配置排除而忽略的文件列表（构建器模式）。
    ///
    /// 这些文件出现在 `--files` 或 `--manifest` 中，但匹配了 `project.exclude`
    /// 配置中的 glob 模式，因此未经过审核。
    /// 不影响 `health_score`。
    #[must_use]
    pub fn with_ignored_files(mut self, files: Vec<String>) -> Self {
        self.ignored_files = files;
        self
    }
}

/// 根据格式渲染报告。
#[must_use]
pub fn render(ctx: &RenderContext, format: ReportFormat) -> String {
    match format {
        ReportFormat::Markdown => render_markdown(ctx),
        ReportFormat::Json => render_json(ctx),
        ReportFormat::Sarif => render_sarif(&ctx.findings),
        ReportFormat::Csv => render_csv(&ctx.findings),
    }
}

/// 将审核结果渲染为结构化 Markdown 报告。
#[must_use]
pub fn render_markdown(ctx: &RenderContext) -> String {
    let mut out = String::with_capacity(4096);

    // ── 标题与门禁结论 ──
    out.push_str("# CodeRoughcollie 审核报告\n\n");

    // ── 执行摘要 ──
    render_summary(&mut out, ctx);

    // ── 跳过文件报告 ──
    render_skipped_files(&mut out, ctx);

    // ── 忽略文件报告（被配置排除） ──
    render_ignored_files(&mut out, ctx);

    // ── 降级警告 ──
    if ctx.degraded {
        out.push_str("> ⚠️ **EXPLAIN 降级**：数据库连接不可用，已自动回退为静态分析。\n\n");
    }

    if ctx.findings.is_empty() {
        out.push_str("✅ **审核通过，未发现问题。**\n\n");
        return out;
    }

    // ── 文件级问题明细 ──
    render_file_details(&mut out, &ctx.findings);

    // ── 规则命中统计 ──
    render_rule_stats(&mut out, &ctx.findings);

    out
}

fn render_summary(out: &mut String, ctx: &RenderContext) {
    out.push_str("## 执行摘要\n\n");
    out.push_str("| 指标 | 值 |\n");
    out.push_str("|------|----|\n");

    let grade_icon = ctx.health_grade.icon();
    let grade_label = ctx.health_grade.as_str();
    out.push_str(&format!("| 健康度评分 | {grade_icon} **{:.0}/100**（{grade_label}） |\n", ctx.health_score));

    let total = ctx.severity_counts.total();
    let c = ctx.severity_counts.critical;
    let w = ctx.severity_counts.warning;
    let i = ctx.severity_counts.info;

    out.push_str(&format!("| 问题总数 | **{total}**（🔴 Critical: {c} / 🟡 Warning: {w} / 🔵 Info: {i}） |\n"));
    out.push_str(&format!("| 审核分支 | `{}` |\n", ctx.branch));

    let gate = if c > 0 {
        "🚫 **阻断** — 存在 Critical 问题，建议修复后再合并"
    } else if total > 0 {
        "⚠️ **通过（有警告）** — 无 Critical 问题，建议评估 Warning 后合并"
    } else {
        "✅ **通过** — 未发现问题"
    };
    out.push_str(&format!("| 门禁结论 | {gate} |\n"));
    if !ctx.skipped_files.is_empty() {
        out.push_str(&format!("| 跳过（不支持类型） | **{}** |\n", ctx.skipped_files.len()));
    }
    if !ctx.ignored_files.is_empty() {
        out.push_str(&format!("| Ignored（被配置排除） | **{}** |\n", ctx.ignored_files.len()));
    }
    out.push('\n');
}

fn render_skipped_files(out: &mut String, ctx: &RenderContext) {
    if ctx.skipped_files.is_empty() {
        return;
    }

    out.push_str("### ⏭️ 跳过的文件（不支持类型）\n\n");

    if ctx.skipped_files.len() > 10 {
        for f in ctx.skipped_files.iter().take(10) {
            out.push_str(&format!("- {f}\n"));
        }
        out.push_str(&format!("（共 {} 个，已省略其余）\n", ctx.skipped_files.len()));
    } else {
        for f in &ctx.skipped_files {
            out.push_str(&format!("- {f}\n"));
        }
    }

    out.push('\n');
}

fn render_ignored_files(out: &mut String, ctx: &RenderContext) {
    if ctx.ignored_files.is_empty() {
        return;
    }

    out.push_str("### ⏭️ Ignored（被配置排除）\n\n");

    for f in &ctx.ignored_files {
        out.push_str(&format!("- {f}\n"));
    }

    out.push('\n');
}

fn render_file_details(out: &mut String, findings: &[Finding]) {
    out.push_str("## 问题明细\n\n");

    // 按文件路径分组
    let mut by_file: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in findings {
        by_file.entry(f.file_path.as_str()).or_default().push(f);
    }

    for (file_path, file_findings) in &by_file {
        // 文件头部：严重度汇总
        let mut critical = 0usize;
        let mut warning = 0usize;
        let mut info = 0usize;
        for f in file_findings {
            match f.severity {
                Severity::Critical => critical += 1,
                Severity::Warning => warning += 1,
                Severity::Info => info += 1,
                _ => {}
            }
        }

        let mut badges = Vec::new();
        if critical > 0 {
            badges.push(format!("🔴 {critical} Critical"));
        }
        if warning > 0 {
            badges.push(format!("🟡 {warning} Warning"));
        }
        if info > 0 {
            badges.push(format!("🔵 {info} Info"));
        }

        out.push_str(&format!("### 📄 `{file_path}` — {}\n\n", badges.join(" / ")));

        // 按严重度排序：Critical > Warning > Info
        let mut sorted: Vec<&&Finding> = file_findings.iter().collect();
        sorted.sort_by_key(|f| match f.severity {
            Severity::Critical => 0u8,
            Severity::Warning => 1,
            Severity::Info => 2,
            _ => 3,
        });

        for f in &sorted {
            out.push_str(&format!("#### {} [{}] {}\n\n", f.severity.icon(), f.rule_id, f.title,));

            // 位置信息
            if let Some(line) = f.node_line {
                out.push_str(&format!("**位置**: `{file_path}:{line}`"));
                if let Some(ref node_type) = f.node_type {
                    out.push_str(&format!("（{node_type}）"));
                }
                out.push_str("\n\n");
            } else {
                out.push_str(&format!("**位置**: `{file_path}`\n\n"));
            }

            // 代码片段
            if let Some(ref snippet) = f.code_snippet {
                out.push_str(&format!("```sql\n{snippet}\n```\n\n"));
            }

            out.push_str(&format!("**说明**: {}\n\n", f.detail));

            if let Some(ref suggestion) = f.suggestion {
                out.push_str(&format!("**建议**: {suggestion}\n\n"));
            }

            out.push_str("---\n\n");
        }
    }
}

fn render_rule_stats(out: &mut String, findings: &[Finding]) {
    out.push_str("## 规则命中统计\n\n");
    out.push_str("| 规则 ID | 命中次数 | 严重度 |\n");
    out.push_str("|---------|---------|--------|\n");

    let mut by_rule: BTreeMap<&str, (usize, Severity)> = BTreeMap::new();
    for f in findings {
        let entry = by_rule.entry(f.rule_id.as_str()).or_insert((0, f.severity));
        entry.0 += 1;
    }

    for (rule_id, (count, severity)) in &by_rule {
        out.push_str(&format!("| `{rule_id}` | {count} | {} {} |\n", severity.icon(), severity.as_str(),));
    }
    out.push('\n');
}

/// 将审核上下文渲染为 JSON 报告。
///
/// 包含 `findings` 和 `skipped_files` 两个顶层字段。
#[must_use]
pub fn render_json(ctx: &RenderContext) -> String {
    let output = serde_json::json!({
        "findings": ctx.findings,
        "skipped_files": ctx.skipped_files,
        "ignored_files": ctx.ignored_files,
    });
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

/// 将 Finding 列表渲染为 SARIF 报告（GitHub Advanced Security 兼容）。
#[must_use]
pub fn render_sarif(findings: &[Finding]) -> String {
    let results: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| {
            serde_json::json!({
                            "ruleId": f.rule_id,
                            "level": match f.severity {
                                cr_core::Severity::Critical => "error",
                                cr_core::Severity::Warning => "warning",
                                _ => "note",
                            },
                            "message": { "text": &f.detail },
                            "locations": [{
                                "physicalLocation": {
                                    "artifactLocation": { "uri": &f.file_path },
                                    "region": f.node_line.map(|l| serde_json::json!({ "startLine": l }))
            }
                            }]
                        })
        })
        .collect();

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "CodeRoughcollie",
                    "version": env!("CARGO_PKG_VERSION")
                }
            },
            "results": results
        }]
    });

    serde_json::to_string_pretty(&sarif).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

/// 将 Finding 列表渲染为 CSV 报告。
///
/// CSV 列：file, line, rule_id, severity, category, title, detail, node_type, suggestion, code_snippet。
/// 包含表头行，字段按 RFC 4180 转义（含逗号/引号/换行的字段加双引号）。
#[must_use]
pub fn render_csv(findings: &[Finding]) -> String {
    let mut out = String::with_capacity(findings.len() * 256);

    // 表头
    out.push_str("file,line,rule_id,severity,category,title,detail,node_type,suggestion,code_snippet\n");

    for f in findings {
        push_csv_field(&mut out, &f.file_path);
        out.push(',');
        if let Some(line) = f.node_line {
            out.push_str(&line.to_string());
        }
        out.push(',');
        push_csv_field(&mut out, &f.rule_id);
        out.push(',');
        push_csv_field(&mut out, f.severity.as_str());
        out.push(',');
        push_csv_field(&mut out, format_category(f.category));
        out.push(',');
        push_csv_field(&mut out, &f.title);
        out.push(',');
        push_csv_field(&mut out, &f.detail);
        out.push(',');
        if let Some(nt) = &f.node_type {
            push_csv_field(&mut out, nt);
        }
        out.push(',');
        if let Some(s) = &f.suggestion {
            push_csv_field(&mut out, s);
        }
        out.push(',');
        if let Some(s) = &f.code_snippet {
            push_csv_field(&mut out, s);
        }
        out.push('\n');
    }

    out
}

// ── 多项目报告 ──

/// 单个项目的报告段。
#[derive(Debug, Clone)]
pub struct ProjectSection {
    /// 项目名称（map key）。
    pub name: String,
    /// 该项目的渲染上下文。
    pub ctx: RenderContext,
}

/// 多项目报告上下文，包含多个项目的审核结果。
#[derive(Debug, Clone)]
pub struct MultiProjectContext {
    /// 按项目排列的报告段。
    pub sections: Vec<ProjectSection>,
}

/// 渲染多项目报告。
#[must_use]
pub fn render_multi(ctx: &MultiProjectContext, format: ReportFormat) -> String {
    match format {
        ReportFormat::Markdown => render_multi_markdown(ctx),
        ReportFormat::Json => render_multi_json(ctx),
        ReportFormat::Sarif => render_multi_sarif(ctx),
        ReportFormat::Csv => render_multi_csv(ctx),
    }
}

fn render_multi_markdown(ctx: &MultiProjectContext) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str("# CodeRoughcollie 多项目审核报告\n\n");

    // ── 总览表 ──
    out.push_str("## 总览\n\n");
    out.push_str("| 项目 | 🔴 Critical | 🟡 Warning | 🔵 Info | 健康度 |\n");
    out.push_str("|------|------------|----------|---------|--------|\n");
    for section in &ctx.sections {
        let c = section.ctx.severity_counts.critical;
        let w = section.ctx.severity_counts.warning;
        let i = section.ctx.severity_counts.info;
        let score = section.ctx.health_score as u8;
        let grade = section.ctx.health_grade.as_str();
        out.push_str(&format!("| {} | {} | {} | {} | {} ({}) |\n", section.name, c, w, i, score, grade));
    }
    out.push('\n');

    // ── 各项目明细 ──
    for section in &ctx.sections {
        out.push_str(&format!("---\n\n# 项目: {}\n\n", section.name));
        render_summary(&mut out, &section.ctx);
        render_skipped_files(&mut out, &section.ctx);
        render_ignored_files(&mut out, &section.ctx);
        if section.ctx.degraded {
            out.push_str("> ⚠️ **EXPLAIN 降级**：数据库连接不可用，已自动回退为静态分析。\n\n");
        }
        if !section.ctx.findings.is_empty() {
            render_file_details(&mut out, &section.ctx.findings);
            render_rule_stats(&mut out, &section.ctx.findings);
        } else {
            out.push_str("✅ **审核通过，未发现问题。**\n\n");
        }
    }

    out
}

fn render_multi_json(ctx: &MultiProjectContext) -> String {
    let projects: Vec<serde_json::Value> = ctx
        .sections
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "findings": s.ctx.findings,
                "skipped_files": s.ctx.skipped_files,
                "ignored_files": s.ctx.ignored_files,
            })
        })
        .collect();

    let total_critical: usize = ctx.sections.iter().map(|s| s.ctx.severity_counts.critical).sum();
    let total_warning: usize = ctx.sections.iter().map(|s| s.ctx.severity_counts.warning).sum();
    let total_info: usize = ctx.sections.iter().map(|s| s.ctx.severity_counts.info).sum();

    let output = serde_json::json!({
        "projects": projects,
        "summary": {
            "project_count": ctx.sections.len(),
            "total_critical": total_critical,
            "total_warning": total_warning,
            "total_info": total_info,
        }
    });
    serde_json::to_string_pretty(&output).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn render_multi_sarif(ctx: &MultiProjectContext) -> String {
    let runs: Vec<serde_json::Value> = ctx
        .sections
        .iter()
        .map(|s| {
            let results: Vec<serde_json::Value> = s
                .ctx
                .findings
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "ruleId": f.rule_id,
                        "level": match f.severity {
                            cr_core::Severity::Critical => "error",
                            cr_core::Severity::Warning => "warning",
                            _ => "note",
                        },
                        "message": { "text": &f.detail },
                        "locations": [{
                            "physicalLocation": {
                                "artifactLocation": { "uri": &f.file_path },
                                "region": f.node_line.map(|l| serde_json::json!({ "startLine": l }))
                            }
                        }]
                    })
                })
                .collect();
            serde_json::json!({
                "tool": {
                    "driver": {
                        "name": "CodeRoughcollie",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                },
                "properties": { "project": &s.name },
                "results": results
            })
        })
        .collect();

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": runs
    });
    serde_json::to_string_pretty(&sarif).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn render_multi_csv(ctx: &MultiProjectContext) -> String {
    let mut out = String::with_capacity(ctx.sections.len() * 256);
    out.push_str("project,file,line,rule_id,severity,category,title,detail,node_type,suggestion,code_snippet\n");

    for section in &ctx.sections {
        for f in &section.ctx.findings {
            push_csv_field(&mut out, &section.name);
            out.push(',');
            push_csv_field(&mut out, &f.file_path);
            out.push(',');
            if let Some(line) = f.node_line {
                out.push_str(&line.to_string());
            }
            out.push(',');
            push_csv_field(&mut out, &f.rule_id);
            out.push(',');
            push_csv_field(&mut out, f.severity.as_str());
            out.push(',');
            push_csv_field(&mut out, format_category(f.category));
            out.push(',');
            push_csv_field(&mut out, &f.title);
            out.push(',');
            push_csv_field(&mut out, &f.detail);
            out.push(',');
            if let Some(nt) = &f.node_type {
                push_csv_field(&mut out, nt);
            }
            out.push(',');
            if let Some(s) = &f.suggestion {
                push_csv_field(&mut out, s);
            }
            out.push(',');
            if let Some(s) = &f.code_snippet {
                push_csv_field(&mut out, s);
            }
            out.push('\n');
        }
    }

    out
}

fn format_category(c: cr_core::DiagnosticCategory) -> &'static str {
    match c {
        cr_core::DiagnosticCategory::ScanEfficiency => "ScanEfficiency",
        cr_core::DiagnosticCategory::JoinStrategy => "JoinStrategy",
        cr_core::DiagnosticCategory::MemoryUsage => "MemoryUsage",
        cr_core::DiagnosticCategory::SortEfficiency => "SortEfficiency",
        cr_core::DiagnosticCategory::NetworkOverhead => "NetworkOverhead",
        cr_core::DiagnosticCategory::CostMisestimation => "CostMisestimation",
        cr_core::DiagnosticCategory::PushdownFailure => "PushdownFailure",
        cr_core::DiagnosticCategory::TypeMismatch => "TypeMismatch",
        cr_core::DiagnosticCategory::Vectorization => "Vectorization",
        cr_core::DiagnosticCategory::SubqueryStructure => "SubqueryStructure",
        cr_core::DiagnosticCategory::DistributionIssue => "DistributionIssue",
        cr_core::DiagnosticCategory::General => "General",
        _ => "Unknown",
    }
}

/// 将字段值写入 CSV 输出，必要时加双引号转义。
fn push_csv_field(out: &mut String, field: &str) {
    let needs_quoting = field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r');
    if needs_quoting {
        out.push('"');
        for ch in field.chars() {
            if ch == '"' {
                out.push_str("\"\"");
            } else {
                out.push(ch);
            }
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::{DiagnosticCategory, Severity};

    fn sample_ctx() -> RenderContext {
        let findings = vec![
            Finding::new(
                "SCAN-001",
                Severity::Critical,
                DiagnosticCategory::ScanEfficiency,
                "大表全表扫描",
                "表 users 未使用索引，预计扫描 1M 行",
                "src/sql/query.sql",
                None,
                Some(3),
                Some("Seq Scan".into()),
                Some("添加索引 idx_users_status".into()),
            ),
            Finding::new(
                "TYPE-001",
                Severity::Warning,
                DiagnosticCategory::TypeMismatch,
                "隐式类型转换",
                "WHERE id = '123' 导致类型转换",
                "src/sql/query.sql",
                None,
                None,
                None,
                None,
            ),
            Finding::new(
                "STATIC-SELECT-STAR",
                Severity::Warning,
                DiagnosticCategory::ScanEfficiency,
                "使用 SELECT * 的查询",
                "SELECT * 会检索所有列",
                "src/sql/orders.sql",
                Some("SELECT * FROM orders".into()),
                Some(5),
                Some("SelectStatement".into()),
                Some("明确列出需要的列名".into()),
            ),
        ];
        let severity_counts = cr_core::scoring::count_by_severity(&findings);
        let hs = cr_core::scoring::health_score(&findings);
        let hg = HealthGrade::from_score(hs);
        RenderContext::new(findings, severity_counts, hs, hg, "feature/test".into(), false)
    }

    #[test]
    fn test_render_markdown_empty() {
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false);
        let output = render_markdown(&ctx);
        assert!(output.contains("审核通过"));
    }

    #[test]
    fn test_render_markdown_with_findings() {
        let ctx = sample_ctx();
        let output = render_markdown(&ctx);
        // 摘要存在
        assert!(output.contains("执行摘要"));
        assert!(output.contains("门禁结论"));
        assert!(output.contains("🚫"));
        // 文件分组存在
        assert!(output.contains("src/sql/query.sql"));
        assert!(output.contains("src/sql/orders.sql"));
        // 规则统计存在
        assert!(output.contains("规则命中统计"));
        assert!(output.contains("SCAN-001"));
        assert!(output.contains("TYPE-001"));
        assert!(output.contains("STATIC-SELECT-STAR"));
        // 代码片段
        assert!(output.contains("SELECT * FROM orders"));
        // 行号
        assert!(output.contains(":5"));
        assert!(output.contains(":3"));
    }

    #[test]
    fn test_render_markdown_degraded() {
        let mut ctx = sample_ctx();
        ctx.degraded = true;
        let output = render_markdown(&ctx);
        assert!(output.contains("EXPLAIN 降级"));
    }

    #[test]
    fn test_render_markdown_skipped_files_empty() {
        // 空 skipped_files 不产生额外输出
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false);
        let output = render_markdown(&ctx);
        assert!(output.contains("审核通过"));
        assert!(!output.contains("跳过（不支持类型）"));
        assert!(!output.contains("⏭️"));
    }

    #[test]
    fn test_render_markdown_with_skipped_files() {
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false)
                .with_skipped_files(vec!["pom.xml".into(), "build.gradle".into()]);
        let output = render_markdown(&ctx);
        assert!(output.contains("跳过（不支持类型）"));
        assert!(output.contains("**2**"));
        assert!(output.contains("pom.xml"));
        assert!(output.contains("build.gradle"));
        assert!(output.contains("⏭️"));
    }

    #[test]
    fn test_render_markdown_skipped_files_long_list() {
        let files: Vec<String> = (1..=15).map(|i| format!("file{i}.xml")).collect();
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false)
                .with_skipped_files(files);
        let output = render_markdown(&ctx);
        assert!(output.contains("**15**"));
        assert!(output.contains("共 15 个，已省略其余"));
        // 只显示前 10 个文件
        assert!(output.contains("file1.xml"));
        assert!(output.contains("file10.xml"));
        assert!(!output.contains("file11.xml"));
    }

    #[test]
    fn test_render_markdown_ignored_files_empty() {
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false);
        let output = render_markdown(&ctx);
        assert!(!output.contains("Ignored"));
        assert!(!output.contains("被配置排除"));
    }

    #[test]
    fn test_render_markdown_with_ignored_files() {
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false)
                .with_ignored_files(vec!["src/test/foo.sql".into(), "mock/data.sql".into()]);
        let output = render_markdown(&ctx);
        assert!(output.contains("Ignored（被配置排除）"));
        assert!(output.contains("**2**"));
        assert!(output.contains("src/test/foo.sql"));
        assert!(output.contains("mock/data.sql"));
        assert!(output.contains("⏭️"));
    }

    #[test]
    fn test_render_json_ignored_files() {
        let mut ctx = sample_ctx();
        ctx.ignored_files = vec!["src/test/excluded.sql".into()];
        let output = render_json(&ctx);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
        assert_eq!(parsed["ignored_files"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["ignored_files"][0], "src/test/excluded.sql");
    }

    #[test]
    fn test_render_json_ignored_files_empty() {
        let ctx = sample_ctx();
        let output = render_json(&ctx);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed["ignored_files"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_render_json_skipped_files() {
        let mut ctx = sample_ctx();
        ctx.skipped_files = vec!["pom.xml".into()];
        let output = render_json(&ctx);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
        assert_eq!(parsed["findings"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["skipped_files"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["skipped_files"][0], "pom.xml");
    }

    #[test]
    fn test_render_json_valid() {
        let ctx = sample_ctx();
        let output = render_json(&ctx);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
        assert_eq!(parsed["findings"].as_array().unwrap().len(), 3);
        assert!(parsed["skipped_files"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_render_json_empty() {
        let ctx =
            RenderContext::new(vec![], SeverityCounts::default(), 100.0, HealthGrade::Excellent, "main".into(), false);
        let output = render_json(&ctx);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["findings"].as_array().unwrap().is_empty());
        assert!(parsed["skipped_files"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_render_sarif_version() {
        let output = render_sarif(&sample_ctx().findings);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["version"], "2.1.0");
        assert!(parsed["$schema"].as_str().unwrap().contains("sarif"));
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "CodeRoughcollie");
        // 验证 file_path 出现在 SARIF 输出中
        assert!(output.contains("src/sql/query.sql"));
    }

    #[test]
    fn test_render_sarif_empty() {
        let output = render_sarif(&[]);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed["runs"][0]["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_render_dispatches_correctly() {
        let ctx = sample_ctx();
        let md = render(&ctx, ReportFormat::Markdown);
        assert!(md.contains("SCAN-001"));

        let json = render(&ctx, ReportFormat::Json);
        assert!(json.contains("SCAN-001"));
        assert!(json.contains("skipped_files"));

        let sarif = render(&ctx, ReportFormat::Sarif);
        assert!(sarif.contains("SCAN-001"));

        let csv = render(&ctx, ReportFormat::Csv);
        assert!(csv.contains("SCAN-001"));
    }

    #[test]
    fn test_render_csv_header() {
        let output = render_csv(&[]);
        let header = output.lines().next().unwrap();
        assert!(header.starts_with("file,line,rule_id,severity,category"));
    }

    #[test]
    fn test_render_csv_empty() {
        let output = render_csv(&[]);
        assert_eq!(output.lines().count(), 1);
    }

    #[test]
    fn test_render_csv_with_findings() {
        let output = render_csv(&sample_ctx().findings);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 4);

        let first = lines[1];
        assert!(first.contains("SCAN-001"));
        assert!(first.contains("critical"));
        assert!(first.contains("src/sql/query.sql"));
        assert!(first.contains("3"));
        assert!(first.contains("ScanEfficiency"));

        let last = lines[3];
        assert!(last.contains("STATIC-SELECT-STAR"));
        assert!(last.contains("SELECT * FROM orders"));
    }

    #[test]
    fn test_render_csv_quoting() {
        let findings = vec![Finding::new(
            "TEST-QUOTE",
            Severity::Warning,
            DiagnosticCategory::General,
            "包含,逗号\"和换行\n的标题",
            "包含,逗号\"和换行\n的详情",
            "src/test.sql",
            None,
            None,
            None,
            None,
        )];
        let output = render_csv(&findings);
        assert!(output.contains("\"包含,逗号\"\"和换行\n的标题\""));
        assert!(output.contains("\"包含,逗号\"\"和换行\n的详情\""));
        assert!(output.contains("\"包含,逗号"));
    }

    #[test]
    fn test_report_format_parse_csv() {
        assert_eq!(ReportFormat::parse("csv").unwrap(), ReportFormat::Csv);
        assert_eq!(ReportFormat::parse("CSV").unwrap(), ReportFormat::Csv);
    }

    #[test]
    fn test_report_format_parse_markdown() {
        assert_eq!(ReportFormat::parse("markdown").unwrap(), ReportFormat::Markdown);
        assert_eq!(ReportFormat::parse("md").unwrap(), ReportFormat::Markdown);
        assert_eq!(ReportFormat::parse("MARKDOWN").unwrap(), ReportFormat::Markdown);
    }

    #[test]
    fn test_report_format_parse_json() {
        assert_eq!(ReportFormat::parse("json").unwrap(), ReportFormat::Json);
        assert_eq!(ReportFormat::parse("JSON").unwrap(), ReportFormat::Json);
    }

    #[test]
    fn test_report_format_parse_sarif() {
        assert_eq!(ReportFormat::parse("sarif").unwrap(), ReportFormat::Sarif);
        assert_eq!(ReportFormat::parse("SARIF").unwrap(), ReportFormat::Sarif);
    }

    #[test]
    fn test_report_format_parse_unknown() {
        assert!(ReportFormat::parse("html").is_err());
        assert!(ReportFormat::parse("").is_err());
        assert!(ReportFormat::parse("pdf").is_err());
    }

    // ── 多项目报告测试 ──

    fn sample_multi_ctx() -> MultiProjectContext {
        let findings_a = vec![Finding::new(
            "SCAN-001",
            Severity::Critical,
            DiagnosticCategory::ScanEfficiency,
            "大表全表扫描",
            "表 users 未使用索引",
            "src/sql/query.sql",
            None,
            Some(3),
            None,
            None,
        )];
        let counts_a = cr_core::scoring::count_by_severity(&findings_a);
        let hs_a = cr_core::scoring::health_score(&findings_a);
        let hg_a = HealthGrade::from_score(hs_a);
        let ctx_a = RenderContext::new(findings_a, counts_a, hs_a, hg_a, "main".into(), false);

        let findings_b = vec![Finding::new(
            "STATIC-SELECT-STAR",
            Severity::Warning,
            DiagnosticCategory::ScanEfficiency,
            "SELECT *",
            "避免 SELECT *",
            "src/sql/orders.sql",
            None,
            Some(1),
            None,
            None,
        )];
        let counts_b = cr_core::scoring::count_by_severity(&findings_b);
        let hs_b = cr_core::scoring::health_score(&findings_b);
        let hg_b = HealthGrade::from_score(hs_b);
        let ctx_b = RenderContext::new(findings_b, counts_b, hs_b, hg_b, "dev".into(), false);

        MultiProjectContext {
            sections: vec![
                ProjectSection { name: "order-service".into(), ctx: ctx_a },
                ProjectSection { name: "payment-service".into(), ctx: ctx_b },
            ],
        }
    }

    #[test]
    fn test_render_multi_markdown() {
        let ctx = sample_multi_ctx();
        let output = render_multi(&ctx, ReportFormat::Markdown);
        assert!(output.contains("多项目审核报告"));
        assert!(output.contains("总览"));
        assert!(output.contains("order-service"));
        assert!(output.contains("payment-service"));
        assert!(output.contains("项目: order-service"));
        assert!(output.contains("SCAN-001"));
        assert!(output.contains("STATIC-SELECT-STAR"));
    }

    #[test]
    fn test_render_multi_json() {
        let ctx = sample_multi_ctx();
        let output = render_multi(&ctx, ReportFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["projects"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["projects"][0]["name"], "order-service");
        assert_eq!(parsed["summary"]["project_count"], 2);
        assert_eq!(parsed["summary"]["total_critical"], 1);
        assert_eq!(parsed["summary"]["total_warning"], 1);
    }

    #[test]
    fn test_render_multi_csv() {
        let ctx = sample_multi_ctx();
        let output = render_multi(&ctx, ReportFormat::Csv);
        let header = output.lines().next().unwrap();
        assert!(header.starts_with("project,file,line"));
        let data_lines: Vec<&str> = output.lines().skip(1).collect();
        assert_eq!(data_lines.len(), 2);
        assert!(data_lines[0].contains("order-service"));
        assert!(data_lines[1].contains("payment-service"));
    }

    #[test]
    fn test_render_multi_sarif() {
        let ctx = sample_multi_ctx();
        let output = render_multi(&ctx, ReportFormat::Sarif);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let runs = parsed["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0]["properties"]["project"], "order-service");
        assert_eq!(runs[1]["properties"]["project"], "payment-service");
    }
}
