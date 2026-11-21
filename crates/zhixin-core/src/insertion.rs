//! 决定"要不要把这个验证码写进当前输入框，写什么"。
//!
//! ★ 这一步是整个预填功能里**最危险**的地方：写错就是动了用户已经输好的内容。
//!   所以规则刻意保守 —— 拿不准就不动，用户至少还有剪贴板可以粘。
//!
//! 写入策略（从保守到主动）：
//!
//! 1. 字段是空的 / 只有空白 → 直接写入
//! 2. 字段里已经就是这个验证码 → 不动
//! 3. 字段里已经包含这个验证码（开头或结尾）→ 不动，别重复追加
//! 4. 用户手打了前几位 → 补全成完整的
//! 5. 其余情况 → **用验证码整体覆盖**（早期版本是"追加"，但 UIA 读到的
//!    placeholder 会被当成已有内容拼进去，故改为不拼接、只写验证码）
//!
//! 计划里同时给出两种机制要用的内容：
//!
//! * `value` —— 用 `ValuePattern.SetValue` 时整体写入的完整值
//! * `typed` —— 用模拟键盘逐字键入时**只需要敲的字符**
//!   （补全时只敲缺的那几位；追加时就是整段验证码）
//!
//! 手动写入使用 [`plan`]；显式开启的自动写入只能使用更保守的 [`plan_automatic`]。

/// 写入决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertionDecision {
    /// 可以写。
    Write,
    /// 不要碰这个字段。
    DoNotTouch,
}

/// 一次写入的完整计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertionPlan {
    pub decision: InsertionDecision,
    /// 直接写入（`SetValue`）时要写的完整值（`DoNotTouch` 时为 `None`）。
    pub value: Option<String>,
    /// 模拟键盘逐字键入时要敲的字符（`DoNotTouch` 时为 `None`）。
    ///
    /// 补全时只有缺的那几位；追加时是整段验证码。
    pub typed: Option<String>,
    /// 人话原因，会显示在候选条上，也会进日志。
    pub reason: String,
}

impl InsertionPlan {
    pub fn should_write(&self) -> bool {
        self.decision == InsertionDecision::Write
    }

    fn do_not_touch(reason: impl Into<String>) -> Self {
        Self {
            decision: InsertionDecision::DoNotTouch,
            value: None,
            typed: None,
            reason: reason.into(),
        }
    }

    fn write(
        value: impl Into<String>,
        typed: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            decision: InsertionDecision::Write,
            value: Some(value.into()),
            typed: Some(typed.into()),
            reason: reason.into(),
        }
    }
}

/// 根据字段**当前**内容和验证码，决定怎么写。
///
/// 调用方应该在用户点击的那一刻重新读一次当前值再调它 ——
/// 从"探测到光标"到"用户点击"之间，用户可能已经打了字。
pub fn plan(current_value: Option<&str>, code: Option<&str>) -> InsertionPlan {
    let Some(code) = code.filter(|code| !code.is_empty()) else {
        return InsertionPlan::do_not_touch("没有可用的验证码");
    };

    let Some(current_value) = current_value else {
        return InsertionPlan::write(code, code, "字段是空的，直接写入");
    };

    if current_value.is_empty() {
        return InsertionPlan::write(code, code, "字段是空的，直接写入");
    }

    let trimmed = current_value.trim();

    if trimmed.is_empty() {
        return InsertionPlan::write(code, code, "字段里只有空白");
    }

    if trimmed == code {
        return InsertionPlan::do_not_touch("验证码已经在字段里了");
    }

    // 字段里已经含有一段验证码（在开头或结尾）→ 别重复追加
    if trimmed.starts_with(code) || trimmed.ends_with(code) {
        return InsertionPlan::do_not_touch("字段里已经有验证码了");
    }

    // 用户手打了前几位（这是验证码输入框里最常见的情况）→ 补全成完整的
    if let Some(tail) = code.strip_prefix(trimmed) {
        return InsertionPlan::write(code, tail, "用户已输入前几位，补全");
    }

    // 其余情况 → 用验证码整体覆盖，不拼接 current。
    // ★ 绝不把 current 写回去：UIA 的 CurrentValue 对 WebView2/富文本/浏览器输入框
    //   返回的往往是 placeholder 文案而非用户真实输入，拼进去会把提示文字固化成真文本，
    //   且受控输入框状态不同步、用户删不掉。验证码输入框里本就该只有验证码。
    InsertionPlan::write(code, code, "字段里有其他内容，用验证码覆盖")
}

/// 自动填入不盲写、不追加普通文本；密码/只读/焦点检查由 UIA 执行层负责。
pub fn plan_automatic(current_value: Option<&str>, code: Option<&str>) -> InsertionPlan {
    let Some(current) = current_value else {
        return InsertionPlan::do_not_touch("无法读取输入框，跳过自动填入");
    };
    let Some(code) = code.filter(|code| !code.is_empty()) else {
        return InsertionPlan::do_not_touch("没有可用的验证码");
    };
    let trimmed = current.trim();
    if !trimmed.is_empty() && (trimmed == code || !code.starts_with(trimmed)) {
        return InsertionPlan::do_not_touch("自动填入仅处理空白或验证码前缀");
    }
    plan(Some(current), Some(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 自动只填已知空白或补全验证码前缀() {
        for current in [
            None,
            Some("hello"),
            Some("123456"),
            Some("abc123456"),
            Some("1234567"),
        ] {
            assert!(!plan_automatic(current, Some("123456")).should_write());
        }
        assert!(!plan_automatic(Some(""), None).should_write());
        for current in ["", " \t", "123"] {
            let result = plan_automatic(Some(current), Some("123456"));
            assert!(result.should_write());
            assert_eq!(result.value.as_deref(), Some("123456"));
        }
        assert_eq!(
            plan_automatic(Some("123"), Some("123456")).typed.as_deref(),
            Some("456")
        );
    }

    fn reason_of(current: Option<&str>, code: Option<&str>) -> String {
        plan(current, code).reason
    }

    #[test]
    fn 空字段直接写入() {
        let plan = plan(Some(""), Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
        assert_eq!(plan.typed.as_deref(), Some("123456"));
        assert_eq!(plan.reason, "字段是空的，直接写入");
    }

    #[test]
    fn 字段为_none_也当作空() {
        let plan = plan(None, Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
        assert_eq!(plan.typed.as_deref(), Some("123456"));
    }

    #[test]
    fn 只有空白也写入() {
        let plan = plan(Some("   \n  "), Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.reason, "字段里只有空白");
    }

    #[test]
    fn 用户打了前几位就补全() {
        for partial in ["1", "123", "1234"] {
            let plan = plan(Some(partial), Some("123456"));

            assert!(plan.should_write(), "前几位 {partial} 应该补全");
            assert_eq!(plan.value.as_deref(), Some("123456"));
            // 模拟键入时只敲缺的那几位，避免把用户已打的重复一遍
            assert_eq!(plan.typed.as_deref(), Some(&"123456"[partial.len()..]));
            assert_eq!(plan.reason, "用户已输入前几位，补全");
        }
    }

    #[test]
    fn 前几位带空格也认() {
        let plan = plan(Some("  123  "), Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
        assert_eq!(plan.typed.as_deref(), Some("456"));
    }

    #[test]
    fn 验证码已经在字段里了就不动() {
        let plan = plan(Some("123456"), Some("123456"));

        assert!(!plan.should_write());
        assert_eq!(plan.reason, "验证码已经在字段里了");
    }

    #[test]
    fn 已有其他内容时用验证码覆盖() {
        // 早期"不覆盖"→"追加"→现在"覆盖"：绝不把 UIA 读到的内容写回去
        let plan = plan(Some("hello world"), Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
        assert_eq!(plan.typed.as_deref(), Some("123456"));
        assert_eq!(plan.reason, "字段里有其他内容，用验证码覆盖");
    }

    #[test]
    fn placeholder_文案被当成已有内容时只写验证码() {
        // ★ 回归测试：WebView2 输入框（如 WorkBuddy 对话栏）的 UIA CurrentValue
        //   返回的是 placeholder，绝不能把它固化成真实文本
        let plan = plan(Some("今天帮你做些什么? @添加上下文，/调用技能与指令"), Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
        assert_eq!(plan.typed.as_deref(), Some("123456"));
    }

    #[test]
    fn 上一个验证码会被覆盖而不是累加() {
        let plan = plan(Some("111111"), Some("123456"));

        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
    }

    #[test]
    fn 没有验证码时不动() {
        assert!(!plan(Some(""), None).should_write());
        assert_eq!(reason_of(Some(""), None), "没有可用的验证码");
        assert_eq!(reason_of(Some(""), Some("")), "没有可用的验证码");
    }

    #[test]
    fn 字段更长但以验证码开头时不动() {
        // "123456 已过期" —— 字段里已经有码了，别又追加一遍
        let plan = plan(Some("123456 已过期"), Some("123456"));

        assert!(!plan.should_write());
        assert_eq!(plan.reason, "字段里已经有验证码了");
    }

    #[test]
    fn 字段以验证码结尾时也不重复追加() {
        let plan = plan(Some("收到 123456"), Some("123456"));

        assert!(!plan.should_write());
        assert_eq!(plan.reason, "字段里已经有验证码了");
    }

    #[test]
    fn 只有空白和验证码组合的情况() {
        // 字段是 " 123456 " → trim 后等于验证码，不动
        let plan = plan(Some(" 123456 "), Some("123456"));

        assert!(!plan.should_write());
        assert_eq!(plan.reason, "验证码已经在字段里了");
    }

    #[test]
    fn 两种机制写入的值始终一致() {
        // 无论走 SetValue 还是逐字键入，写进去的都应该是纯验证码，
        // 这样两种模式的结果才一致，也才不会把 UIA 读到的不可靠内容带进去。
        for current in ["hello ", "验证码：", "abc", "今天帮你做些什么? @添加上下文"] {
            let plan = plan(Some(current), Some("123456"));

            assert_eq!(plan.decision, InsertionDecision::Write);
            assert_eq!(plan.typed.as_deref(), Some("123456"));
            assert_eq!(plan.value.as_deref(), Some("123456"));
        }
    }

    // ── 压力测试：各种输入框怪状态 ────────────────────────────
    // 核心不变量：写进去的值只可能是「纯验证码」或「验证码前缀补全」，
    // 任何情况下都不得包含 UIA 读到的 current（可能是 placeholder / 默认文案）。

    #[test]
    fn 压力_common_placeholder_形态一律覆盖() {
        let placeholders = [
            "今天帮你做些什么? @添加上下文，/调用技能与指令", // WorkBuddy 对话栏（本次 bug）
            "搜索消息",                                  // 搜索框 placeholder
            "请输入验证码",                              // 验证码框 placeholder
            "Message",                                   // 英文输入框
            "Type your message here...",
            "手机号码 / 邮箱",
        ];
        for p in placeholders {
            let plan = plan(Some(p), Some("123456"));
            assert!(plan.should_write(), "placeholder「{p}」应该覆盖写入");
            assert_eq!(plan.value.as_deref(), Some("123456"), "不得把 placeholder 写回去");
            assert!(!plan.value.as_ref().unwrap().contains(p));
        }
    }

    #[test]
    fn 压力_多行和特殊字符一律覆盖() {
        let weird = [
            "第一行\n第二行",
            "a\tb\tc",
            "🎉🎉🎉",
            "—— — ——",
            "\\n\\t literal",
            "https://example.com/?a=1&b=2",
            "   \u{00a0}\u{3000}  ", // 不间断空格 + 全角空格：trim 后为空
        ];
        for w in weird {
            let plan = plan(Some(w), Some("123456"));
            match plan.decision {
                InsertionDecision::Write => {
                    assert_eq!(plan.value.as_deref(), Some("123456"));
                }
                InsertionDecision::DoNotTouch => {
                    // 只允许"已经有验证码"这类原因，不允许异常
                    assert!(plan.reason.contains("验证码"));
                }
            }
        }
    }

    #[test]
    fn 压力_极长输入不会爆也不会拼进去() {
        let long = "x".repeat(10_000);
        let plan = plan(Some(&long), Some("123456"));
        assert!(plan.should_write());
        assert_eq!(plan.value.as_deref(), Some("123456"));
    }

    #[test]
    fn 压力_各种验证码位数和字符() {
        for code in ["1", "12", "123456", "12345678", "ABC123", "000000"] {
            // 空框
            let p = plan(Some(""), Some(code));
            assert_eq!(p.value.as_deref(), Some(code));
            // 有 placeholder
            let p = plan(Some("请输入内容"), Some(code));
            assert_eq!(p.value.as_deref(), Some(code));
            // 部分前缀补全
            if code.len() > 2 {
                let partial = &code[..code.len() - 1];
                let p = plan(Some(partial), Some(code));
                assert_eq!(p.value.as_deref(), Some(code));
                assert_eq!(p.typed.as_deref(), Some(&code[code.len() - 1..]));
            }
        }
    }

    #[test]
    fn 压力_none和全空白始终只写验证码() {
        assert_eq!(plan(None, Some("123456")).value.as_deref(), Some("123456"));
        for blank in ["", " ", "\t", "\n", "  \n\t  "] {
            assert_eq!(plan(Some(blank), Some("123456")).value.as_deref(), Some("123456"));
        }
    }

    #[test]
    fn 压力_已含验证码时不重复写入() {
        for current in ["123456", " 123456 ", "收到 123456", "123456 已过期"] {
            let plan = plan(Some(current), Some("123456"));
            assert!(!plan.should_write(), "「{current}」已含验证码，不应再写");
            assert!(plan.value.is_none());
        }
    }
}
