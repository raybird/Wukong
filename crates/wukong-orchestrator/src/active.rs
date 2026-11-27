//! 回合裡派出、還沒回報的 subagent（2026-11-27，#45 review-65e960a、review-623c436）。
//!
//! Telegram 的狀態泡泡、Web 的進度與排程的階段都用它決定顯示「悟空」還是「悟空·fixer」：
//! 顯示最近派出、還沒回報的那一個，都回報之後回到 primary agent（悟空）。

use crate::Role;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ActiveSubagents {
    running: Vec<Role>,
}

impl ActiveSubagents {
    pub fn dispatched(&mut self, role: Role) {
        self.running.push(role);
    }

    /// 同一個角色派出多次時，只移出最近派出的那一次。
    pub fn reported(&mut self, role: Role) {
        if let Some(i) = self.running.iter().rposition(|r| *r == role) {
            self.running.remove(i);
        }
    }

    /// 目前顯示的 subagent；`None` 是悟空自己在處理。
    pub fn current(&self) -> Option<Role> {
        self.running.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_the_latest_running_subagent_and_returns_to_wukong_after_reports() {
        let mut active = ActiveSubagents::default();
        assert_eq!(active.current(), None);

        active.dispatched(Role::Explorer);
        active.dispatched(Role::Fixer);
        assert_eq!(active.current(), Some(Role::Fixer));

        active.reported(Role::Fixer);
        assert_eq!(active.current(), Some(Role::Explorer));
        active.reported(Role::Explorer);
        assert_eq!(active.current(), None);
    }

    #[test]
    fn a_report_removes_only_the_latest_dispatch_of_that_role() {
        let mut active = ActiveSubagents::default();
        active.dispatched(Role::Fixer);
        active.dispatched(Role::Oracle);
        active.dispatched(Role::Fixer);

        active.reported(Role::Fixer);
        assert_eq!(active.current(), Some(Role::Oracle));
        active.reported(Role::Oracle);
        assert_eq!(active.current(), Some(Role::Fixer));
        // 沒派出過的角色回報時不影響
        active.reported(Role::Designer);
        assert_eq!(active.current(), Some(Role::Fixer));
    }
}
