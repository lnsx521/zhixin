//! 手机号段归属地查询：内嵌全国号段区间表（138,821 个区间，覆盖移动/联通/电信）。
//! 数据来源：开源号段数据集（2026-11 生成），查询纯本地、不联网。

use std::sync::OnceLock;

const RANGES_TSV: &str = include_str!("../resources/phone_ranges.tsv");

struct Range {
    start: u32,
    end: u32,
    city: String,
    isp: String,
}

static TABLE: OnceLock<Vec<Range>> = OnceLock::new();

fn table() -> &'static [Range] {
    TABLE.get_or_init(|| {
        RANGES_TSV
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let span = parts.next()?;
                let city = parts.next()?.to_string();
                let isp = parts.next()?.to_string();
                let (start, end) = span.split_once('-')?;
                Some(Range {
                    start: start.parse().ok()?,
                    end: end.parse().ok()?,
                    city,
                    isp,
                })
            })
            .collect()
    })
}

/// 查询号码归属地："城市 · 运营商"。号码里提取不出 7 位有效号段或查不到 → None。
pub fn lookup(number: &str) -> Option<String> {
    let digits: String = number.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 7 {
        return None;
    }
    let prefix: u32 = digits.get(..7)?.parse().ok()?;
    let ranges = table();
    let index = ranges.partition_point(|r| r.start <= prefix);
    let candidate = ranges.get(index.checked_sub(1)?)?;
    if candidate.start <= prefix && prefix <= candidate.end {
        Some(format!("{} · {}", candidate.city, candidate.isp))
    } else {
        None
    }
}
