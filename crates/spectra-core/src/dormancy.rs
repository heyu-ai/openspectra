//! change 的休眠判定（oracle 3.0.0 `drift --json` 與 `instructions apply --json` 共用的
//! `dormancy` 物件）。規則與實證見 `docs/reverse-engineering/drift.md` 的 Dormancy 一節。
//!
//! 判定只看兩個量：`created` 距今的日曆天數（本地時區），以及最後一個碰到 change 目錄的
//! commit 的 **author** 時間距今經過的整天數。建立超過 5 天且 3 天以上沒有 commit 就是
//! `triggered`。

use std::path::Path;
use std::process::Command;

use chrono::{Local, NaiveDate, Utc};
use serde::Serialize;

/// `dormancy` 物件，欄位順序照 oracle 的 struct 序列化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dormancy {
    pub status: &'static str,
    pub reason: &'static str,
    pub age_days: Option<i64>,
    pub idle_days: Option<i64>,
}

/// change 目錄的 git 歷史狀態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    /// 找不到 repo。
    NoRepo,
    /// repo 存在但無法執行 `git log`。
    Unavailable,
    /// change 目錄（經 symlink 解析後）不在 work tree 內。
    Outside,
    /// 時間戳無法解析。
    InvalidTimestamp,
    /// 最後一個碰到 change 目錄的 commit 的 author 時間（UNIX 秒）；沒有則為 `None`。
    LastCommit(Option<i64>),
}

const AGE_DAYS: i64 = 5;
const IDLE_DAYS: i64 = 3;

fn verdict(
    status: &'static str,
    reason: &'static str,
    age_days: Option<i64>,
    idle_days: Option<i64>,
) -> Dormancy {
    Dormancy {
        status,
        reason,
        age_days,
        idle_days,
    }
}

/// 純判定（oracle 的決策表，依序評估）。`today` 是本地日期，`now` 是 UNIX 秒。
pub fn decide(created: Option<&str>, today: NaiveDate, now: i64, history: History) -> Dormancy {
    let Some(created) = created.and_then(|c| NaiveDate::parse_from_str(c, "%Y-%m-%d").ok()) else {
        return verdict("unknown", "created date missing or invalid", None, None);
    };
    let age = (today - created).num_days();
    if age < 0 {
        return verdict("unknown", "created date is in the future", Some(age), None);
    }
    let last = match history {
        History::NoRepo => return verdict("unknown", "Git unavailable", Some(age), None),
        History::Unavailable => {
            return verdict("unknown", "Git history unavailable", Some(age), None)
        }
        History::Outside => {
            return verdict(
                "unknown",
                "change directory outside Git work directory",
                Some(age),
                None,
            )
        }
        History::InvalidTimestamp => {
            return verdict("unknown", "invalid Git timestamp", Some(age), None)
        }
        History::LastCommit(last) => last,
    };
    let Some(last) = last else {
        return if age > AGE_DAYS {
            verdict(
                "triggered",
                "older than five days; no directory commits",
                Some(age),
                None,
            )
        } else {
            verdict(
                "fresh",
                "age or idle threshold not reached",
                Some(age),
                None,
            )
        };
    };
    if last > now {
        return verdict(
            "unknown",
            "commit timestamp invalid or in the future",
            Some(age),
            None,
        );
    }
    let idle = (now - last) / 86_400;
    if age > AGE_DAYS && idle >= IDLE_DAYS {
        verdict(
            "triggered",
            "older than five days; no directory commit in three days",
            Some(age),
            Some(idle),
        )
    } else {
        verdict(
            "fresh",
            "age or idle threshold not reached",
            Some(age),
            Some(idle),
        )
    }
}

/// 讀 git 歷史。repo 必須真的能開啟（壞掉的 `.git` 目錄算沒有 repo，探測 dorm-nogit）；
/// git 本身無法執行時，專案上層有 `.git` 就是 `Unavailable`（oracle 以 libgit2 開 repo、
/// 以 git 執行檔讀歷史），否則是 `NoRepo`。
pub fn history(project_root: &Path, change_dir: &Path) -> History {
    let toplevel = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let work_dir = match toplevel {
        Err(_) => {
            let has_git = project_root
                .ancestors()
                .any(|dir| dir.join(".git").exists());
            return if has_git {
                History::Unavailable
            } else {
                History::NoRepo
            };
        }
        Ok(out) if !out.status.success() => return History::NoRepo,
        Ok(out) => std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()),
    };
    let (Ok(work_dir), Ok(change_dir)) = (work_dir.canonicalize(), change_dir.canonicalize())
    else {
        return History::Outside;
    };
    let Ok(relative) = change_dir.strip_prefix(&work_dir) else {
        return History::Outside;
    };
    let pathspec = format!(":(top,literal){}", relative.to_string_lossy());
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(&work_dir)
        .args(["log", "-1", "--format=%at", "--", &pathspec])
        .output();
    let Ok(output) = output else {
        return History::Unavailable;
    };
    if !output.status.success() {
        // 還沒有任何 commit 的 repo：oracle 視為沒有碰到目錄的 commit。
        let unborn = Command::new("git")
            .arg("-C")
            .arg(&work_dir)
            .args(["rev-parse", "--verify", "-q", "HEAD"])
            .output()
            .is_ok_and(|o| !o.status.success());
        return if unborn {
            History::LastCommit(None)
        } else {
            History::Unavailable
        };
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        return History::LastCommit(None);
    }
    text.parse().map_or(History::InvalidTimestamp, |ts| {
        History::LastCommit(Some(ts))
    })
}

/// 目前時間下的判定。
pub fn evaluate(project_root: &Path, change_dir: &Path, created: Option<&str>) -> Dormancy {
    decide(
        created,
        Local::now().date_naive(),
        Utc::now().timestamp(),
        history(project_root, change_dir),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
    }

    /// 2026-09-28T12:00:00Z
    const NOW: i64 = 1_790_596_800;

    fn d(
        created: Option<&str>,
        history: History,
    ) -> (&'static str, &'static str, Option<i64>, Option<i64>) {
        let v = decide(created, today(), NOW, history);
        (v.status, v.reason, v.age_days, v.idle_days)
    }

    /// oracle 3.0.0 的決策表（探測 p12–p14）。
    #[test]
    fn decision_table_matches_oracle() {
        let commit = |days_ago: i64| History::LastCommit(Some(NOW - days_ago));
        assert_eq!(
            d(None, commit(0)),
            ("unknown", "created date missing or invalid", None, None)
        );
        assert_eq!(
            d(Some("notadate"), commit(0)),
            ("unknown", "created date missing or invalid", None, None)
        );
        assert_eq!(
            d(Some("2026-09-01T10:00:00Z"), commit(0)),
            ("unknown", "created date missing or invalid", None, None)
        );
        assert_eq!(
            d(Some("2026-10-01"), commit(0)),
            ("unknown", "created date is in the future", Some(-3), None)
        );
        assert_eq!(
            d(Some("2026-9-1"), History::NoRepo),
            ("unknown", "Git unavailable", Some(27), None)
        );
        assert_eq!(
            d(Some("2026-09-01"), History::Unavailable),
            ("unknown", "Git history unavailable", Some(27), None)
        );
        assert_eq!(
            d(Some("2026-09-01"), History::Outside),
            (
                "unknown",
                "change directory outside Git work directory",
                Some(27),
                None
            )
        );
        assert_eq!(
            d(Some("2026-09-01"), History::LastCommit(Some(NOW + 60))),
            (
                "unknown",
                "commit timestamp invalid or in the future",
                Some(27),
                None
            )
        );
        assert_eq!(
            d(Some("2026-09-01"), History::LastCommit(None)),
            (
                "triggered",
                "older than five days; no directory commits",
                Some(27),
                None
            )
        );
        assert_eq!(
            d(Some("2026-09-23"), History::LastCommit(None)),
            ("fresh", "age or idle threshold not reached", Some(5), None)
        );
        // 邊界：age 5／idle 10 → fresh；age 6／idle 3 → triggered；age 6／1.96 天 → fresh。
        assert_eq!(
            d(Some("2026-09-23"), commit(10 * DAY)),
            (
                "fresh",
                "age or idle threshold not reached",
                Some(5),
                Some(10)
            )
        );
        assert_eq!(
            d(Some("2026-09-22"), commit(3 * DAY + 120)),
            (
                "triggered",
                "older than five days; no directory commit in three days",
                Some(6),
                Some(3)
            )
        );
        assert_eq!(
            d(Some("2026-09-22"), commit(3 * DAY - 120)),
            (
                "fresh",
                "age or idle threshold not reached",
                Some(6),
                Some(2)
            )
        );
        assert_eq!(
            d(Some("2026-09-22"), commit(196 * DAY / 100)),
            (
                "fresh",
                "age or idle threshold not reached",
                Some(6),
                Some(1)
            )
        );
    }
}
