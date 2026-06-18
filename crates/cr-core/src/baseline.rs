//! 基线对比逻辑。
//!
//! 记录复杂度基线、计算增量 Δ，支持与 `main` 分支比较。

use std::collections::HashMap;
use std::path::Path;

/// 文件级复杂度基线记录。
///
/// Key: 文件路径, Value: 复杂度分数。
pub type BaselineMap = HashMap<String, f64>;

/// 基线对比结果。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BaselineDiff {
    /// 文件路径。
    pub file_path: String,
    /// 基线分数（`None` 表示新增文件）。
    pub baseline_score: Option<f64>,
    /// 当前分数。
    pub current_score: f64,
    /// 增量 Δ（`None` 表示新增文件，`Some(diff)` 表示与基线比较的差值）。
    pub delta: Option<f64>,
}

/// 从 JSON 文件加载基线。
///
/// # Errors
///
/// 当文件无法读取或 JSON 格式无效时返回 `std::io::Error`。
pub fn load_baseline(path: &Path) -> Result<BaselineMap, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let baseline: BaselineMap =
        serde_json::from_str(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(baseline)
}

/// 保存基线到 JSON 文件。
///
/// # Errors
///
/// 当文件无法写入或序列化失败时返回 `std::io::Error`。
pub fn save_baseline(path: &Path, baseline: &BaselineMap) -> Result<(), std::io::Error> {
    let content =
        serde_json::to_string_pretty(baseline).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, content)
}

/// 比较当前复杂度与基线，返回增量列表。
///
/// 对于基线中已存在的文件，计算 `current - baseline` 作为 delta；
/// 对于新增文件（基线中不存在），`baseline_score` 和 `delta` 均为 `None`。
#[must_use]
pub fn compare_baseline(baseline: &BaselineMap, current: &HashMap<String, f64>) -> Vec<BaselineDiff> {
    let mut diffs = Vec::new();
    for (file_path, current_score) in current {
        match baseline.get(file_path) {
            Some(baseline_score) => {
                diffs.push(BaselineDiff {
                    file_path: file_path.clone(),
                    baseline_score: Some(*baseline_score),
                    current_score: *current_score,
                    delta: Some(current_score - baseline_score),
                });
            }
            None => {
                diffs.push(BaselineDiff {
                    file_path: file_path.clone(),
                    baseline_score: None,
                    current_score: *current_score,
                    delta: None,
                });
            }
        }
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_load_baseline_roundtrip() {
        let mut original = BaselineMap::new();
        original.insert("src/main.rs".into(), 15.0);
        original.insert("src/lib.rs".into(), 22.5);
        original.insert("tests/test.rs".into(), 8.0);

        let dir = std::env::temp_dir();
        let path = dir.join("test_baseline_roundtrip.json");

        // Clean up any leftover
        let _ = std::fs::remove_file(&path);

        save_baseline(&path, &original).unwrap();
        let loaded = load_baseline(&path).unwrap();

        assert_eq!(original, loaded);

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_load_empty_baseline() {
        let original = BaselineMap::new();
        let dir = std::env::temp_dir();
        let path = dir.join("test_baseline_empty.json");

        let _ = std::fs::remove_file(&path);
        save_baseline(&path, &original).unwrap();
        let loaded = load_baseline(&path).unwrap();
        assert!(loaded.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let path = std::env::temp_dir().join("nonexistent_baseline.json");
        let _ = std::fs::remove_file(&path);
        assert!(load_baseline(&path).is_err());
    }

    #[test]
    fn test_compare_baseline_existing_file() {
        let mut baseline = BaselineMap::new();
        baseline.insert("src/main.rs".into(), 10.0);

        let mut current = HashMap::new();
        current.insert("src/main.rs".into(), 15.0);

        let diffs = compare_baseline(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file_path, "src/main.rs");
        assert_eq!(diffs[0].baseline_score, Some(10.0));
        assert_eq!(diffs[0].current_score, 15.0);
        assert_eq!(diffs[0].delta, Some(5.0));
    }

    #[test]
    fn test_compare_baseline_new_file() {
        let baseline = BaselineMap::new();
        let mut current = HashMap::new();
        current.insert("new_file.rs".into(), 12.0);

        let diffs = compare_baseline(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file_path, "new_file.rs");
        assert_eq!(diffs[0].baseline_score, None);
        assert_eq!(diffs[0].current_score, 12.0);
        assert_eq!(diffs[0].delta, None);
    }

    #[test]
    fn test_compare_baseline_mixed() {
        let mut baseline = BaselineMap::new();
        baseline.insert("stable.rs".into(), 5.0);

        let mut current = HashMap::new();
        current.insert("stable.rs".into(), 5.0);
        current.insert("new.rs".into(), 3.0);

        let diffs = compare_baseline(&baseline, &current);
        assert_eq!(diffs.len(), 2);

        let stable = diffs.iter().find(|d| d.file_path == "stable.rs").unwrap();
        assert_eq!(stable.delta, Some(0.0));

        let new = diffs.iter().find(|d| d.file_path == "new.rs").unwrap();
        assert_eq!(new.baseline_score, None);
        assert_eq!(new.delta, None);
    }

    #[test]
    fn test_compare_baseline_no_current() {
        let mut baseline = BaselineMap::new();
        baseline.insert("orphan.rs".into(), 10.0);

        let current = HashMap::new();
        let diffs = compare_baseline(&baseline, &current);
        assert!(diffs.is_empty());
    }
}
