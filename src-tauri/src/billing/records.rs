//! jfself limits individual date queries. Disjoint inclusive windows keep
//! ordinary pagination and full exports in the same newest-first order.
use super::*;

trait Pages {
    fn page(
        &mut self,
        query: &ValidatedBillingRecordQuery,
        page: u32,
        size: u32,
    ) -> impl std::future::Future<Output = Result<BillingTable, BillingError>> + Send;
}

struct Remote<'a> {
    session: &'a mut BillingSession,
    spec: &'a BillingRecordSpec,
}
impl Pages for Remote<'_> {
    async fn page(
        &mut self,
        query: &ValidatedBillingRecordQuery,
        page: u32,
        size: u32,
    ) -> Result<BillingTable, BillingError> {
        fetch_record_page(self.session, query, self.spec, page, size).await
    }
}

pub(super) async fn query(
    session: &mut BillingSession,
    query: &ValidatedBillingRecordQuery,
    spec: &BillingRecordSpec,
) -> Result<BillingTable, BillingError> {
    collect(&mut Remote { session, spec }, query).await
}

fn windows(query: &ValidatedBillingRecordQuery) -> Vec<ValidatedBillingRecordQuery> {
    let start = chrono::NaiveDate::parse_from_str(
        query.start_date.as_deref().expect("validated date"),
        "%Y-%m-%d",
    )
    .expect("validated date");
    let mut end = chrono::NaiveDate::parse_from_str(
        query.end_date.as_deref().expect("validated date"),
        "%Y-%m-%d",
    )
    .expect("validated date");
    let mut result = Vec::new();
    loop {
        let first = end
            .checked_sub_signed(chrono::Duration::days(59))
            .unwrap_or(start)
            .max(start);
        let mut part = query.clone();
        part.start_date = Some(first.to_string());
        part.end_date = Some(end.to_string());
        result.push(part);
        if first == start {
            break;
        }
        end = first.pred_opt().expect("after start");
    }
    result
}

fn changed() -> BillingError {
    BillingError::Protocol("查询期间记录发生变化或返回不完整，请重新查询".into())
}

fn check_page(table: &BillingTable, page: u32, size: u32, total: u64) -> Result<(), BillingError> {
    let expected = total
        .saturating_sub((u64::from(page) - 1) * u64::from(size))
        .min(u64::from(size));
    if table.total != total || table.rows.len() as u64 != expected {
        return Err(changed());
    }
    Ok(())
}

async fn collect(
    source: &mut impl Pages,
    query: &ValidatedBillingRecordQuery,
) -> Result<BillingTable, BillingError> {
    let parts = windows(query);
    if parts.len() == 1 && !query.all {
        let table = source.page(&parts[0], query.page, query.page_size).await?;
        check_page(&table, query.page, query.page_size, table.total)?;
        return Ok(table);
    }
    // Only load a small head page per window to discover totals. Normal paging
    // must not download entire years of usage or inherit the CSV export cap.
    let mut heads = Vec::new();
    let mut combined = BillingTable::default();
    for part in &parts {
        let head = source.page(part, 1, 10).await?;
        check_page(&head, 1, 10, head.total)?;
        combined.total = combined.total.checked_add(head.total).ok_or_else(changed)?;
        if query.all && combined.total > MAX_FULL_EXPORT_ROWS {
            return Err(BillingError::InvalidRequest(
                "记录超过单次完整导出的上限，请缩小日期范围；仍可分页查看".into(),
            ));
        }
        heads.push(head);
    }
    combined.summary = summaries(&heads);
    let mut offset = if query.all {
        0
    } else {
        (u64::from(query.page) - 1) * u64::from(query.page_size)
    };
    let mut remaining = if query.all {
        combined.total
    } else {
        u64::from(query.page_size)
    };
    for (part, head) in parts.iter().zip(&heads) {
        if offset >= head.total {
            offset -= head.total;
            continue;
        }
        let end = head.total.min(offset.saturating_add(remaining));
        while offset < end {
            let page = u32::try_from(offset / 100 + 1).map_err(|_| changed())?;
            let table = source.page(part, page, 100).await?;
            check_page(&table, page, 100, head.total)?;
            let within = (offset % 100) as usize;
            let count = (end - offset).min(table.rows.len() as u64 - within as u64) as usize;
            combined
                .rows
                .extend(table.rows.into_iter().skip(within).take(count));
            offset += count as u64;
            remaining -= count as u64;
        }
        if remaining == 0 {
            break;
        }
        offset = 0;
    }
    Ok(combined)
}

// Exact decimal addition: never accumulate billing money with binary floats.
fn add_decimal(left: &str, right: &str) -> Option<String> {
    fn parts(value: &str) -> Option<(i128, u32)> {
        let value = value.trim();
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if fraction.len() > 18 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let negative = whole.starts_with('-');
        let digits = format!("{}{}", whole.strip_prefix('-').unwrap_or(whole), fraction);
        let number = digits.parse::<i128>().ok()?;
        Some((
            if negative { -number } else { number },
            fraction.len() as u32,
        ))
    }
    let (a, sa) = parts(left)?;
    let (b, sb) = parts(right)?;
    let scale = sa.max(sb);
    let value = a
        .checked_mul(10i128.checked_pow(scale - sa)?)?
        .checked_add(b.checked_mul(10i128.checked_pow(scale - sb)?)?)?;
    let factor = 10i128.checked_pow(scale)?;
    let absolute = value.checked_abs()?;
    let mut text = if scale == 0 {
        absolute.to_string()
    } else {
        format!(
            "{}.{:0width$}",
            absolute / factor,
            absolute % factor,
            width = scale as usize
        )
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
    };
    if value < 0 {
        text.insert(0, '-');
    }
    Some(text)
}

fn summaries(tables: &[BillingTable]) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    // A missing/unknown subtotal must not be presented as a complete total.
    for key in tables
        .iter()
        .flat_map(|table| table.summary.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let sum = tables.iter().try_fold("0".to_string(), |sum, table| {
            if table.total == 0 && !table.summary.contains_key(key) {
                Some(sum)
            } else {
                add_decimal(&sum, table.summary.get(key)?)
            }
        });
        if let Some(value) = sum {
            result.insert(key.clone(), value);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ValidatedBillingRecordQuery {
        ValidatedBillingRecordQuery {
            kind: BillingRecordKind::Usage,
            page: 1,
            page_size: 20,
            start_date: Some("2023-12-31".into()),
            end_date: Some("2024-05-01".into()),
            year: None,
            all: false,
        }
    }
    struct Fixture {
        parts: Vec<ValidatedBillingRecordQuery>,
        totals: Vec<u64>,
        fail: bool,
        changed: bool,
        calls: Vec<(usize, u32, u32)>,
    }
    impl Fixture {
        fn new(query: &ValidatedBillingRecordQuery, totals: Vec<u64>) -> Self {
            Self {
                parts: windows(query),
                totals,
                fail: false,
                changed: false,
                calls: Vec::new(),
            }
        }
    }
    impl Pages for Fixture {
        async fn page(
            &mut self,
            query: &ValidatedBillingRecordQuery,
            page: u32,
            size: u32,
        ) -> Result<BillingTable, BillingError> {
            let part = self
                .parts
                .iter()
                .position(|part| part.start_date == query.start_date)
                .unwrap();
            self.calls.push((part, page, size));
            if self.fail && size == 100 {
                return Err(BillingError::Protocol("fixture failure".into()));
            }
            let total = self.totals[part];
            let start = u64::from(page - 1) * u64::from(size);
            Ok(BillingTable {
                total: total + u64::from(self.changed && size == 100),
                rows: (start..(start + u64::from(size)).min(total))
                    .map(|row| BTreeMap::from([("id".into(), format!("{part}:{row}"))]))
                    .collect(),
                summary: BTreeMap::from([("金额(元)".into(), "0.1".into())]),
            })
        }
    }

    #[test]
    fn windows_cover_leap_day_and_year_boundary_once() {
        let query = request();
        let parts = windows(&query);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].end_date, query.end_date);
        assert_eq!(parts.last().unwrap().start_date, query.start_date);
        for (index, part) in parts.iter().enumerate() {
            let start =
                chrono::NaiveDate::parse_from_str(part.start_date.as_deref().unwrap(), "%Y-%m-%d")
                    .unwrap();
            let end =
                chrono::NaiveDate::parse_from_str(part.end_date.as_deref().unwrap(), "%Y-%m-%d")
                    .unwrap();
            assert!((0..60).contains(&(end - start).num_days()));
            if index + 1 < parts.len() {
                assert_eq!(
                    parts[index + 1].end_date.as_deref(),
                    Some(start.pred_opt().unwrap().to_string().as_str())
                );
            }
        }
        let mut one = query.clone();
        one.start_date = one.end_date.clone();
        assert_eq!(windows(&one).len(), 1);
    }

    #[test]
    fn short_ranges_keep_single_request_pagination() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut query = request();
            query.start_date = Some("2024-04-01".into());
            query.page = 2;
            let mut source = Fixture::new(&query, vec![25]);
            let table = collect(&mut source, &query).await.unwrap();
            assert_eq!(table.rows.len(), 5);
            assert_eq!(table.rows[0]["id"], "0:20");
            assert_eq!(source.calls, vec![(0, 2, 20)]);
        });
    }

    #[test]
    fn pagination_crosses_windows_without_fetching_all_rows() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut query = request();
            let mut source = Fixture::new(&query, vec![17, 104, 0]);
            let first = collect(&mut source, &query).await.unwrap();
            assert_eq!(first.total, 121);
            assert_eq!(first.rows.len(), 20);
            assert_eq!(first.rows[16]["id"], "0:16");
            assert_eq!(first.rows[17]["id"], "1:0");
            assert_eq!(first.summary["金额(元)"], "0.3");
            assert!(!source.calls.iter().any(|(_, page, _)| *page > 1));
            query.page = 6;
            let last = collect(&mut source, &query).await.unwrap();
            assert_eq!(last.rows.len(), 20);
            assert_eq!(last.rows[0]["id"], "1:83");
            assert_eq!(last.rows[19]["id"], "1:102");
            query.page = 7;
            assert_eq!(
                collect(&mut source, &query).await.unwrap().rows[0]["id"],
                "1:103"
            );
            query.page = 8;
            assert!(collect(&mut source, &query).await.unwrap().rows.is_empty());
        });
    }

    #[test]
    fn exports_cover_all_pages_and_do_not_return_partial_results() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut query = request();
            query.all = true;
            let mut source = Fixture::new(&query, vec![17, 104, 0]);
            let all = collect(&mut source, &query).await.unwrap();
            assert_eq!(all.rows.len(), 121);
            assert_eq!(all.rows[120]["id"], "1:103");
            source.fail = true;
            assert!(collect(&mut source, &query).await.is_err());
            source.fail = false;
            source.changed = true;
            assert!(collect(&mut source, &query).await.is_err());
            let mut huge = Fixture::new(&query, vec![25_001, 25_001, 0]);
            assert!(collect(&mut huge, &query).await.is_err());
            query.all = false;
            assert_eq!(collect(&mut huge, &query).await.unwrap().total, 50_002);
        });
    }

    #[test]
    fn summaries_use_exact_decimals_and_omit_incomplete_totals() {
        assert_eq!(add_decimal("12.01", "-0.2").as_deref(), Some("11.81"));
        assert_eq!(add_decimal("0.1", "0.2").as_deref(), Some("0.3"));
        assert!(add_decimal("未知", "1").is_none());
        let mut first = BillingTable {
            total: 1,
            summary: BTreeMap::from([("金额(元)".into(), "1.2".into())]),
            ..Default::default()
        };
        assert!(summaries(&[
            first.clone(),
            BillingTable {
                total: 1,
                ..Default::default()
            }
        ])
        .is_empty());
        first.rows = vec![BTreeMap::new(), BTreeMap::new()];
        assert!(check_page(&first, 1, 10, 1).is_err());
    }
}
