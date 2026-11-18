use super::*;

pub(super) fn handle(state: &mut FakeState, operation: &str, body: &Value) -> Answer {
    let tenant = body["tenant"].as_str().unwrap_or_default().to_owned();
    let namespace = body["namespace"].as_str().unwrap_or_default();
    let matches = |owner: &str, item: &Value| {
        owner == namespace
            && item["observation_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .is_some_and(|id| {
                    state.subjects.get(&(tenant.clone(), id)) == Some(&body["subject"])
                })
            && state
                .namespaces
                .get(&(tenant.clone(), namespace.to_owned()))
                == Some(&true)
    };
    if operation.ends_with("/list") {
        let after = body["after"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok());
        let limit = body["limit"].as_u64().unwrap_or(100) as usize;
        if !(1..=100).contains(&limit) {
            return Err((StatusCode::BAD_REQUEST, "invalid_request"));
        }
        let mut items:Vec<Value>=state.observations.iter().filter(|(owner,item)|matches(owner,item))
            .filter_map(|(_,item)| {
                let id=Uuid::parse_str(item["observation_id"].as_str()?).ok()?;
                after.is_none_or(|after|id>after).then(||json!({"observation_id":id,"revision":item["revision"],"access_level":item["access_level"]}))
            }).collect();
        items.sort_by(|left, right| {
            left["observation_id"]
                .as_str()
                .cmp(&right["observation_id"].as_str())
        });
        let more = items.len() > limit;
        items.truncate(limit);
        let next = if more {
            items.last().map(|item| item["observation_id"].clone())
        } else {
            None
        };
        return Ok(json!({"items":items,"next_after":next}));
    }
    let wanted: std::collections::BTreeSet<_> = body["observation_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().and_then(|id| Uuid::parse_str(id).ok()))
        .collect();
    if wanted.is_empty()
        || wanted.len() > 100
        || Some(wanted.len()) != body["observation_ids"].as_array().map(Vec::len)
    {
        return Err((StatusCode::BAD_REQUEST, "invalid_request"));
    }
    let deleted: Vec<_> = state
        .observations
        .iter()
        .filter(|(owner, item)| matches(owner, item))
        .filter_map(|(_, item)| Uuid::parse_str(item["observation_id"].as_str()?).ok())
        .filter(|id| wanted.contains(id))
        .collect();
    let subjects = &state.subjects;
    state.observations.retain(|(owner, item)| {
        owner != namespace
            || !item["observation_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .is_some_and(|id| {
                    deleted.contains(&id)
                        && subjects.get(&(tenant.clone(), id)) == Some(&body["subject"])
                })
    });
    for id in &deleted {
        state.subjects.remove(&(tenant.clone(), *id));
    }
    let mut deleted = deleted;
    deleted.sort_unstable();
    Ok(json!({"deleted":deleted}))
}
