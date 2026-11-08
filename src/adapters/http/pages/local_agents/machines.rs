use super::*;

pub(super) fn render(view: &Page<'_>) -> String {
    let company = view.company.id;
    let base = base(company);
    let cards=view.machines.iter().map(|machine| {
        let state=enum_value(&machine.state);
        let seen=machine.last_seen_at.map_or_else(||"Never".into(),|time|time.to_rfc3339());
        let controls=if machine.state!=MachineState::Revoked {
            let change=if machine.state==MachineState::Enabled{"disable"}else{"enable"};
            let change_form=form(&format!("{base}/machines/{}/state",machine.id),&format!(r#"<input type="hidden" name="change" value="{change}">"#),if change=="disable"{"Disable new work"}else{"Enable new work"});
            let revoke=form(&format!("{base}/machines/{}/state",machine.id),r#"<input type="hidden" name="change" value="revoke">"#,"Revoke machine credential");
            format!("{change_form}{revoke}")
        }else{String::new()};
        format!(r#"<article class="card card-border bg-base-100 p-4 space-y-2"><a class="link font-semibold" href="{base}?machine={id}">{name}</a><p>{state} · Last seen {seen}</p><p class="font-mono text-xs break-all">{id}</p><div class="flex flex-wrap gap-3">{controls}</div>{replacement}</article>"#,
            id=machine.id,name=text(&machine.name),seen=text(&seen),state=text(&state),
            replacement=form(&format!("{base}/pairings"),&format!(r#"<input type="hidden" name="replace" value="{}">"#,machine.id),"Create replacement pairing code"))
    }).collect::<String>();
    let next = if view.machines.len() == bounds::MAX_PAGE_RECORDS {
        view.machines.last().map_or_else(String::new, |machine| {
            format!(
                r#"<a class="link" href="{base}?after={}">More machines</a>"#,
                machine.id
            )
        })
    } else {
        String::new()
    };
    let pending=view.pairings.iter().filter(|pair|pair.consumed_at.is_none()&&pair.revoked_at.is_none()).map(|pair| {
        format!(r#"<li class="flex flex-wrap gap-3 items-center">Pairing expires {expires}{cancel}</li>"#,expires=text(&pair.expires_at.to_rfc3339()),
            cancel=form(&format!("{base}/pairings/{}/cancel",pair.id),"","Cancel pairing"))
    }).collect::<String>();
    let issued=view.pairing.map_or_else(String::new,|created|format!(r#"<div role="status" class="alert flex-col items-start"><p>Enter this one-time code in the Openbot plugin on the company machine. It expires at {expires}.</p><code class="select-all font-mono break-all">{code}</code></div>"#,
        expires=text(&created.attempt.expires_at.to_rfc3339()),code=text(created.code.expose_secret())));
    format!(
        r#"<section class="space-y-4"><h2 class="text-xl font-semibold">Company machines</h2><p>Keep the Cloudroom server, host daemon and Openbot plugin running. Disabling or revoking a machine prevents new work; it does not prove disconnected native work stopped.</p>{issued}{new_pair}<ul>{pending}</ul><div class="grid gap-4 md:grid-cols-2">{cards}</div>{next}{selected}</section>"#,
        new_pair = form(&format!("{base}/pairings"), "", "Create pairing code"),
        selected = selected(view)
    )
}

fn selected(view: &Page<'_>) -> String {
    let (Some(details), Some(runtime)) = (view.machine, view.runtime) else {
        return String::new();
    };
    let machine = &details.machine;
    let base = base(view.company.id);
    use crate::application::local_agents::protocol::ConnectionIssue;
    let connection = match runtime.last_issue {
        Some(ConnectionIssue::IncompatibleProtocol) => {
            "Incompatible plugin protocol · install the supported plugin build"
        }
        Some(ConnectionIssue::RecoveryRequired) => {
            "Journal recovery required · restore the original journal or revoke and pair again; do not repeat uncertain work"
        }
        Some(ConnectionIssue::ConnectionFailed) => {
            "Connection failed · inspect local plugin logs and test the catalog"
        }
        None if !runtime.connected => "Offline",
        None if runtime.reconciled => "Connected · protocol compatible",
        None => "Connected · handshake or reconciliation pending",
    };
    let test=view.test.map_or_else(String::new,|test|format!("<p role=\"status\">Catalog connection test: {}. Reload this page to reconcile its current result.</p>",text(&test.operation.outcome().map_or_else(||"queued".into(),|outcome|enum_value(&outcome)))));
    let catalog=details.capabilities.as_ref().map_or_else(||"<p>No verified catalog yet. Start the plugin and request a connection test.</p>".into(),|cap| {
        let providers=cap.catalog.providers.iter().map(|provider|format!("<li>{}: {} advertised models</li>",text(&enum_value(&provider.provider)),provider.models.len())).collect::<String>();
        let projects=cap.catalog.projects.iter().map(|project|format!(r#"<label class="flex gap-2 items-center"><input type="checkbox" class="checkbox" name="project" value="{id}"{checked}>{name}</label>"#,
            id=attr(project.as_str()),name=text(project.as_str()),checked=if runtime.projects.contains(project){" checked"}else{""})).collect::<String>();
        format!("<p>Catalog observed at {}. Advertised availability is not proof of a working provider login.</p><ul>{providers}</ul>{}",text(&cap.observed_at.to_rfc3339()),form(&format!("{base}/machines/{}/projects",machine.id),&projects,"Save enabled projects"))
    });
    format!(
        r#"<article class="card card-border p-4 space-y-3"><h3 class="font-semibold">{name}</h3><p>{connection}</p><p>{effects} unsettled native effects · {commands} outstanding commands</p>{diagnostics}<p>A connection test checks plugin reachability and catalog exchange. Only an explicit native task establishes provider execution. Offline work waits until its saved deadline; it never moves to another machine.</p>{test}{refresh}{catalog}</article>"#,
        name = text(&machine.name),
        effects = runtime.unsettled_effects,
        commands = runtime.outstanding_commands,
        diagnostics = diagnostics(runtime),
        refresh = form(
            &format!("{base}/machines/{}/test", machine.id),
            &format!(
                r#"<input type="hidden" name="request_id" value="{}">"#,
                request_id()
            ),
            "Test connection / Refresh catalog"
        )
    )
}

fn diagnostics(runtime: &MachineRuntime) -> String {
    let oldest = runtime
        .oldest_outstanding_at
        .map_or_else(|| "None".into(), |time| time.to_rfc3339());
    let success = runtime.last_native_success_at.map_or_else(
        || "No successful native task recorded".into(),
        |time| time.to_rfc3339(),
    );
    let pressure = if runtime.backpressured {
        "Admission paused at the server capacity bound"
    } else {
        "Server queue has admission capacity"
    };
    format!(
        "<p>{} permission waits · {} uncertain server outcomes · {} reconciliation gaps</p><p>Oldest outstanding command: {}. Last matching native success: {}.</p><p>{}. Local journal pressure is reported by the plugin status command.</p>",
        runtime.permission_waits,
        runtime.uncertain_outcomes,
        runtime.reconciliation_gaps,
        text(&oldest),
        text(&success),
        pressure
    )
}
