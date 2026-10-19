use super::*;
use crate::services::harness::runs::TokenUsageSource;

pub struct UsageSample {
    pub provider: ModelProvider,
    pub model: ModelName,
    pub workflow: Option<Uuid>,
    pub agent: Uuid,
    pub input: u64,
    pub output: u64,
    pub source: Option<TokenUsageSource>,
    pub prices: Option<PriceRates>,
}

#[derive(Default)]
struct Total {
    calls: u64,
    completed: u64,
    reported: u64,
    estimated: u64,
    mixed: u64,
    input: u64,
    output: u64,
    cost: u128,
    unpriced: u64,
}
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Model(ModelProvider, ModelName),
    Workflow(Option<Uuid>),
}
pub fn aggregate(
    calls: Vec<UsageSample>,
    query: &UsageQuery,
) -> AppResult<(Vec<UsageGroup>, bool)> {
    if calls.len() > 10_000 {
        return Err(AppError::BadRequest(
            "Usage sample exceeds report bound".into(),
        ));
    }
    let mut groups = std::collections::BTreeMap::<GroupKey, Total>::new();
    let mut truncated = false;
    for call in calls {
        if query.agent.is_some_and(|id| id != call.agent)
            || query.workflow.is_some_and(|id| Some(id) != call.workflow)
        {
            continue;
        }
        let key = match query.grouping {
            UsageGrouping::Model => GroupKey::Model(call.provider.clone(), call.model.clone()),
            UsageGrouping::Workflow => GroupKey::Workflow(call.workflow),
        };
        if groups.len() >= 200 && !groups.contains_key(&key) {
            truncated = true;
            continue;
        }
        add_call(groups.entry(key).or_default(), call)?;
    }
    Ok((
        groups
            .into_iter()
            .map(|(key, total)| group(key, total))
            .collect(),
        truncated,
    ))
}
fn add_call(total: &mut Total, call: UsageSample) -> AppResult<()> {
    let input = call.input;
    let output = call.output;
    if input > i32::MAX as u64 || output > i32::MAX as u64 {
        return Err(AppError::Internal(
            "Usage counter exceeds supported bound".into(),
        ));
    }
    total.calls += 1;
    total.input += input;
    total.output += output;
    total.completed += u64::from(call.source.is_some());
    match call.source {
        Some(TokenUsageSource::Reported) => total.reported += 1,
        Some(TokenUsageSource::Estimated) => total.estimated += 1,
        Some(TokenUsageSource::Mixed) => total.mixed += 1,
        None => {}
    }
    if let Some(prices) = call.prices {
        let input_price = prices.input.micros() as u128;
        let output_price = prices.output.micros() as u128;
        total.cost = input_price
            .checked_mul(input as u128)
            .and_then(|v| {
                output_price
                    .checked_mul(output as u128)
                    .and_then(|o| v.checked_add(o))
            })
            .and_then(|v| total.cost.checked_add(v))
            .ok_or_else(|| AppError::Internal("Usage cost overflow".into()))?;
    } else {
        total.unpriced += 1;
    }
    Ok(())
}
fn group(key: GroupKey, total: Total) -> UsageGroup {
    let mut group = UsageGroup {
        provider: None,
        model: None,
        workflow: None,
        calls: total.calls,
        completed_calls: total.completed,
        unconfirmed_calls: total.calls - total.completed,
        reported_calls: total.reported,
        estimated_calls: total.estimated,
        mixed_calls: total.mixed,
        input_tokens: total.input,
        output_tokens: total.output,
        unpriced_calls: total.unpriced,
        priced_subtotal_usd: (total.unpriced < total.calls).then(|| {
            format!(
                "{}.{:012}",
                total.cost / 1_000_000_000_000,
                total.cost % 1_000_000_000_000
            )
        }),
    };
    match key {
        GroupKey::Model(provider, model) => {
            group.provider = Some(provider);
            group.model = Some(model);
        }
        GroupKey::Workflow(workflow) => group.workflow = workflow,
    }
    group
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> UsageQuery {
        let end = Utc::now();
        UsageQuery {
            scope: UsageScope {
                company: Uuid::new_v4(),
                actor: Uuid::new_v4(),
            },
            start: end - Duration::days(1),
            end,
            grouping: UsageGrouping::Workflow,
            agent: None,
            workflow: None,
        }
    }
    fn sample(source: Option<TokenUsageSource>) -> UsageSample {
        UsageSample {
            provider: "openai".into(),
            model: "model".into(),
            workflow: None,
            agent: Uuid::nil(),
            input: 1,
            output: 1,
            source,
            prices: Some(PriceRates {
                input: TokenPrice::parse("0.000001").unwrap(),
                output: TokenPrice::parse("0").unwrap(),
            }),
        }
    }
    #[test]
    fn model_usage_confidence_partial_prices_and_small_costs_survive_grouping() {
        let mut unpriced = sample(Some(TokenUsageSource::Reported));
        unpriced.prices = None;
        let (groups, truncated) = aggregate(
            vec![
                sample(None),
                sample(Some(TokenUsageSource::Estimated)),
                sample(Some(TokenUsageSource::Mixed)),
                unpriced,
            ],
            &query(),
        )
        .unwrap();
        assert!(!truncated);
        let group = &groups[0];
        assert_eq!(
            (group.calls, group.completed_calls, group.unconfirmed_calls),
            (4, 3, 1)
        );
        assert_eq!(
            (
                group.reported_calls,
                group.estimated_calls,
                group.mixed_calls
            ),
            (1, 1, 1)
        );
        assert_eq!((group.input_tokens, group.output_tokens), (4, 4));
        assert_eq!(group.unpriced_calls, 1);
        assert_eq!(group.priced_subtotal_usd.as_deref(), Some("0.000000000003"));
    }
    #[test]
    fn model_usage_cost_at_all_supported_bounds_is_exact() {
        let samples = (0..10000)
            .map(|_| {
                let mut row = sample(Some(TokenUsageSource::Reported));
                row.input = i32::MAX as u64;
                row.output = i32::MAX as u64;
                row.prices = Some(PriceRates {
                    input: TokenPrice::parse("1000000").unwrap(),
                    output: TokenPrice::parse("1000000").unwrap(),
                });
                row
            })
            .collect();
        let (groups, _) = aggregate(samples, &query()).unwrap();
        assert_eq!(
            groups[0].priced_subtotal_usd.as_deref(),
            Some("42949672940000.000000000000")
        );
    }
    #[test]
    fn model_usage_group_bound_and_filters_are_explicit() {
        let samples = (0..201)
            .map(|index| {
                let mut row = sample(None);
                row.workflow = Some(Uuid::from_u128(index + 1));
                row
            })
            .collect();
        let (groups, truncated) = aggregate(samples, &query()).unwrap();
        assert_eq!(groups.len(), 200);
        assert!(truncated);
        let filtered = UsageQuery {
            agent: Some(Uuid::new_v4()),
            ..query()
        };
        assert!(
            aggregate(vec![sample(None)], &filtered)
                .unwrap()
                .0
                .is_empty()
        );
    }
}
