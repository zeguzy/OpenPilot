use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    Foreground,
    Background {
        description: String,
        focus: Vec<String>,
        estimated_files: Vec<PathBuf>,
    },
}

const BACKGROUND_WORDS: &[&str] = &[
    "refactor",
    "implement",
    "fix",
    "rewrite",
    "create",
    "build",
    "add",
    "migrate",
    "optimize",
    "delete",
    "remove",
    "update",
    "write",
    "generate",
    "重构",
    "实现",
    "修复",
    "重写",
    "创建",
    "构建",
    "添加",
    "迁移",
    "优化",
    "删除",
    "移除",
    "更新",
    "编写",
    "生成",
    "修改",
    "改造",
    "升级",
    "调整",
    "整理",
    "完善",
    "处理",
    "部署",
    "安装",
    "配置",
    "测试",
];

const FOREGROUND_WORDS: &[&str] = &[
    "what",
    "how",
    "why",
    "explain",
    "where",
    "who",
    "when",
    "show",
    "list",
    "status",
    "tell",
    "describe",
    "什么",
    "怎么",
    "为什么",
    "如何",
    "哪里",
    "哪个",
    "谁",
    "何时",
    "解释",
    "说明",
    "看一下",
    "查一下",
    "状态",
    "进度",
    "怎么样",
    "多少",
];

fn contains_word(message: &str, word: &str) -> bool {
    if word.chars().any(|c| c.is_ascii_alphanumeric()) {
        message
            .split(|c: char| !c.is_alphanumeric())
            .any(|token| token.eq_ignore_ascii_case(word))
    } else {
        message.contains(word)
    }
}

#[must_use]
pub fn route(message: &str, _context: &str) -> RouteDecision {
    let has_background = BACKGROUND_WORDS.iter().any(|w| contains_word(message, w));
    let has_foreground = FOREGROUND_WORDS.iter().any(|w| contains_word(message, w));

    if has_background && !has_foreground {
        RouteDecision::Background {
            description: message.to_string(),
            focus: Vec::new(),
            estimated_files: Vec::new(),
        }
    } else {
        RouteDecision::Foreground
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_routes_foreground() {
        let decision = route("what does this function do?", "");
        assert_eq!(decision, RouteDecision::Foreground);
    }

    #[test]
    fn how_question_routes_foreground() {
        let decision = route("how does the auth flow work?", "");
        assert_eq!(decision, RouteDecision::Foreground);
    }

    #[test]
    fn refactor_routes_background() {
        let decision = route("refactor the auth module to OAuth2", "");
        assert!(matches!(decision, RouteDecision::Background { .. }));
    }

    #[test]
    fn implement_routes_background() {
        let decision = route("implement user registration", "");
        assert!(matches!(decision, RouteDecision::Background { .. }));
    }

    #[test]
    fn fix_routes_background() {
        let decision = route("fix the login bug", "");
        assert!(matches!(decision, RouteDecision::Background { .. }));
    }

    #[test]
    fn plain_statement_routes_foreground() {
        let decision = route("hello world", "");
        assert_eq!(decision, RouteDecision::Foreground);
    }

    #[test]
    fn mixed_message_with_question_word_routes_foreground() {
        let decision = route("how should I implement the fix?", "");
        assert_eq!(decision, RouteDecision::Foreground);
    }

    #[test]
    fn background_carries_description() {
        let decision = route("create the user service", "");
        match decision {
            RouteDecision::Background { description, .. } => {
                assert_eq!(description, "create the user service");
            }
            RouteDecision::Foreground => panic!("expected Background"),
        }
    }

    #[test]
    fn chinese_refactor_routes_background() {
        let decision = route("重构 auth 模块", "");
        assert!(matches!(decision, RouteDecision::Background { .. }));
    }

    #[test]
    fn chinese_fix_routes_background() {
        let decision = route("修复登录 bug", "");
        assert!(matches!(decision, RouteDecision::Background { .. }));
    }

    #[test]
    fn chinese_implement_routes_background() {
        let decision = route("实现用户注册功能", "");
        assert!(matches!(decision, RouteDecision::Background { .. }));
    }

    #[test]
    fn chinese_question_routes_foreground() {
        let decision = route("这个函数是做什么的", "");
        assert_eq!(decision, RouteDecision::Foreground);
    }

    #[test]
    fn chinese_status_query_routes_foreground() {
        let decision = route("任务进度怎么样了", "");
        assert_eq!(decision, RouteDecision::Foreground);
    }
}
