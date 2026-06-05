/// One of the five specialist roles (from tao-of-coding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Explorer,
    Oracle,
    Librarian,
    Fixer,
    Designer,
}

impl Role {
    /// All five roles in a fixed order (used by routing and parsing).
    pub fn all() -> [Role; 5] {
        [
            Role::Explorer,
            Role::Oracle,
            Role::Librarian,
            Role::Fixer,
            Role::Designer,
        ]
    }

    /// Lowercase identifier.
    pub fn name(&self) -> &'static str {
        match self {
            Role::Explorer => "explorer",
            Role::Oracle => "oracle",
            Role::Librarian => "librarian",
            Role::Fixer => "fixer",
            Role::Designer => "designer",
        }
    }

    /// One-line role description, listed in the routing prompt.
    pub fn description(&self) -> &'static str {
        match self {
            Role::Explorer => "結構洞察，快速掃描專案結構、理解檔案關聯與依賴。",
            Role::Oracle => "架構專家，擅長重構、決策分析與技術取捨。",
            Role::Librarian => "文件專家，負責撰寫文件、翻譯與註解。",
            Role::Fixer => "實作專家，程式碼修正、單元測試補全、語法修正，高效交付可運作的程式。",
            Role::Designer => "設計專家，負責 UI/UX 與前端體驗。",
        }
    }

    /// Role system prompt prepended when executing as this role.
    pub fn card(&self) -> &'static str {
        match self {
            Role::Explorer => {
                "你是 Explorer，結構洞察專家。負責快速掃描專案結構、追蹤依賴關係，揭開陌生程式碼的面貌。"
            }
            Role::Oracle => {
                "你是 Oracle，架構專家。擅長重構、決策分析與技術取捨；當架構混亂或 Bug 難解時提供可執行方案。"
            }
            Role::Librarian => {
                "你是 Librarian，文件專家。負責撰寫文件、API 註解與翻譯，條理分明。"
            }
            Role::Fixer => {
                "你是 Fixer，實作專家。負責程式碼修正、單元測試補全、語法修正，以最高效率交付可運作的程式。"
            }
            Role::Designer => {
                "你是 Designer，設計專家。負責 UI/UX 介面結構、互動與視覺一致性。"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let names: Vec<&str> = Role::all().iter().map(|r| r.name()).collect();
        let mut deduped = names.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(names.len(), deduped.len());
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn cards_and_descriptions_non_empty() {
        for r in Role::all() {
            assert!(!r.card().is_empty());
            assert!(!r.description().is_empty());
        }
    }
}
