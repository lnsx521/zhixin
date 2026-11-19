//! 号段运营商与号码类型识别（纯规则表，约 5KB，无城市数据）。
//!
//! 注意：号段规则反映的是号码的**原始归属**，携号转网用户会显示原号段运营商——
//! 这是本地规则的天然限制（2026-11-19 老板确认接受）。

/// 按号码内容生成描述（如"中国移动 · 手机号"）；识别不出来返回 None。
pub fn describe(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let desc = type_desc(&digits)?;
    let carrier = match digits.get(0..3)? {
        // 移动
        "134" | "135" | "136" | "137" | "138" | "139" | "147" | "148" | "150" | "151"
        | "152" | "157" | "158" | "159" | "172" | "178" | "182" | "183" | "184"
        | "187" | "188" | "195" | "197" | "198" => "中国移动",
        // 联通
        "130" | "131" | "132" | "145" | "146" | "155" | "156" | "166" | "167" | "175"
        | "176" | "185" | "186" | "196" => "中国联通",
        // 电信
        "133" | "149" | "153" | "173" | "174" | "177" | "180" | "181" | "189" | "190"
        | "191" | "193" | "199" => "中国电信",
        // 虚拟运营商（170/171 等按前三四位分布，此处粗判为虚拟运营商）
        "162" | "165" | "167" | "170" | "171" => "虚拟运营商",
        _ => return Some(type_only(&digits).to_string()),
    };
    Some(format!("{carrier} · {desc}"))
}

fn type_only(digits: &str) -> &'static str {
    match type_desc(digits) {
        Some(d) => d,
        None => "号码",
    }
}

fn type_desc(digits: &str) -> Option<&'static str> {
    if (digits.len() == 10 || digits.len() == 12)
        && (digits.starts_with("400") || digits.starts_with("800"))
    {
        return Some("服务号");
    }
    match digits.len() {
        11 if digits.starts_with('1') => Some("手机号"),
        _ => {
            if digits.starts_with('0') {
                Some("座机")
            } else if digits.starts_with("95") || digits.starts_with("96") {
                Some("服务号")
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn mobile_carrier() {
        assert_eq!(describe("13856781234").as_deref(), Some("中国移动 · 手机号"));
        assert_eq!(describe("18612345678").as_deref(), Some("中国联通 · 手机号"));
        assert_eq!(describe("18912345678").as_deref(), Some("中国电信 · 手机号"));
    }

    #[test]
    fn landline_and_service() {
        assert_eq!(describe("02164228888").as_deref(), Some("座机"));
        assert!(describe("95123456").unwrap().contains("服务号"));
        assert!(describe("4001234567").unwrap().contains("服务号"));
    }
}
